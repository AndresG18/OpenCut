use std::collections::{HashMap, HashSet};

use time::RationalTime;

use crate::{
    CandidateEvaluation, CandidateId, ClipCandidate, ClipFinderError, ClipRequest, ClipSuggestion,
    EvaluationScores, RankingWeights,
};

/// Validate external-model evaluations, rank them using local policy, and
/// remove near-duplicate windows for human review.
pub fn rank_suggestions(
    candidates: &[ClipCandidate],
    evaluations: &[CandidateEvaluation],
    request: &ClipRequest,
    weights: RankingWeights,
) -> Result<Vec<ClipSuggestion>, ClipFinderError> {
    request.validate()?;
    let weight_total = weights.validate()?;
    let by_id = candidates
        .iter()
        .map(|candidate| (candidate.id, candidate))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut ranked = Vec::with_capacity(evaluations.len());

    for evaluation in evaluations {
        let candidate = by_id
            .get(&evaluation.candidate_id)
            .ok_or(ClipFinderError::UnknownCandidate(evaluation.candidate_id))?;
        if !seen.insert(evaluation.candidate_id) {
            return Err(ClipFinderError::DuplicateEvaluation(
                evaluation.candidate_id,
            ));
        }
        validate_scores(evaluation.candidate_id, evaluation.scores)?;

        let overall_score = (evaluation.scores.hook * weights.hook
            + evaluation.scores.relevance * weights.relevance
            + evaluation.scores.coherence * weights.coherence
            + evaluation.scores.standalone * weights.standalone)
            / weight_total;

        ranked.push(ClipSuggestion {
            candidate: (*candidate).clone(),
            title: evaluation.title.clone(),
            reason: evaluation.reason.clone(),
            keywords: evaluation.keywords.clone(),
            scores: evaluation.scores,
            overall_score,
        });
    }

    ranked.sort_by(|left, right| {
        right
            .overall_score
            .total_cmp(&left.overall_score)
            .then_with(|| left.candidate.id.raw().cmp(&right.candidate.id.raw()))
    });

    let mut selected: Vec<ClipSuggestion> = Vec::with_capacity(request.suggestion_limit);
    for suggestion in ranked {
        let mut overlaps_selected = false;
        for existing in &selected {
            if exceeds_overlap(
                &suggestion.candidate,
                &existing.candidate,
                request.max_overlap_percent,
            )? {
                overlaps_selected = true;
                break;
            }
        }
        if !overlaps_selected {
            selected.push(suggestion);
            if selected.len() == request.suggestion_limit {
                break;
            }
        }
    }

    Ok(selected)
}

fn validate_scores(
    candidate_id: CandidateId,
    scores: EvaluationScores,
) -> Result<(), ClipFinderError> {
    for (field, value) in [
        ("hook", scores.hook),
        ("relevance", scores.relevance),
        ("coherence", scores.coherence),
        ("standalone", scores.standalone),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(ClipFinderError::InvalidEvaluationScore {
                candidate_id,
                field,
            });
        }
    }
    Ok(())
}

fn exceeds_overlap(
    left: &ClipCandidate,
    right: &ClipCandidate,
    max_overlap_percent: u8,
) -> Result<bool, ClipFinderError> {
    let start = if left.start.lt(&right.start) {
        right.start
    } else {
        left.start
    };
    let end = if left.end.lt(&right.end) {
        left.end
    } else {
        right.end
    };

    if !start.lt(&end) {
        return Ok(false);
    }

    let overlap = end.sub(start)?;
    let left_duration = left.duration()?;
    let right_duration = right.duration()?;
    let shorter = if left_duration.lt(&right_duration) {
        left_duration
    } else {
        right_duration
    };

    ratio_exceeds_percent(overlap, shorter, max_overlap_percent)
}

fn ratio_exceeds_percent(
    numerator: RationalTime,
    denominator: RationalTime,
    percent: u8,
) -> Result<bool, ClipFinderError> {
    let left = i128::from(numerator.numer())
        .checked_mul(100)
        .and_then(|value| value.checked_mul(i128::from(denominator.denom())))
        .ok_or(time::TimeError::Overflow)?;
    let right = i128::from(percent)
        .checked_mul(i128::from(denominator.numer()))
        .and_then(|value| value.checked_mul(i128::from(numerator.denom())))
        .ok_or(time::TimeError::Overflow)?;
    Ok(left > right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AspectRatio, ClipProfile, DurationRange, TranscriptSegment, generate_candidates};

    fn seconds(value: i64) -> RationalTime {
        RationalTime::new(value, 1).unwrap()
    }

    fn request() -> ClipRequest {
        ClipRequest {
            profile: ClipProfile {
                id: "short".into(),
                label: "Short".into(),
                duration: DurationRange::new(seconds(10), seconds(10), seconds(10)).unwrap(),
                aspect_ratio: Some(AspectRatio {
                    width: 9,
                    height: 16,
                }),
                instructions: String::new(),
            },
            prompt: "Find concise product lessons".into(),
            suggestion_limit: 3,
            candidate_limit: 20,
            max_overlap_percent: 40,
        }
    }

    fn candidates() -> Vec<ClipCandidate> {
        let transcript = (0..5)
            .map(|index| TranscriptSegment {
                start: seconds(index * 5),
                end: seconds(index * 5 + 5),
                text: format!("Thought {index}."),
                speaker: None,
            })
            .collect::<Vec<_>>();
        generate_candidates(&transcript, &request()).unwrap()
    }

    fn evaluation(candidate_id: CandidateId, score: f32) -> CandidateEvaluation {
        CandidateEvaluation {
            candidate_id,
            scores: EvaluationScores {
                hook: score,
                relevance: score,
                coherence: score,
                standalone: score,
            },
            title: format!("Candidate {}", candidate_id.raw()),
            reason: "Strong self-contained moment".into(),
            keywords: vec!["lesson".into()],
        }
    }

    #[test]
    fn ranking_filters_highly_overlapping_candidates() {
        let candidates = candidates();
        let evaluations = candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| evaluation(candidate.id, 1.0 - index as f32 * 0.1))
            .collect::<Vec<_>>();

        let ranked = rank_suggestions(
            &candidates,
            &evaluations,
            &request(),
            RankingWeights::default(),
        )
        .unwrap();

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].candidate.id, CandidateId::new(0));
        assert_eq!(ranked[1].candidate.id, CandidateId::new(2));
    }

    #[test]
    fn rejects_unknown_and_duplicate_evaluations() {
        let candidates = candidates();
        let unknown = evaluation(CandidateId::new(99), 0.5);
        assert_eq!(
            rank_suggestions(
                &candidates,
                &[unknown],
                &request(),
                RankingWeights::default()
            ),
            Err(ClipFinderError::UnknownCandidate(CandidateId::new(99)))
        );

        let duplicate = evaluation(candidates[0].id, 0.5);
        assert_eq!(
            rank_suggestions(
                &candidates,
                &[duplicate.clone(), duplicate],
                &request(),
                RankingWeights::default()
            ),
            Err(ClipFinderError::DuplicateEvaluation(candidates[0].id))
        );
    }

    #[test]
    fn ranking_weights_are_normalized() {
        let candidates = candidates();
        let mut evaluation = evaluation(candidates[0].id, 0.0);
        evaluation.scores.hook = 1.0;
        let ranked = rank_suggestions(
            &candidates,
            &[evaluation],
            &request(),
            RankingWeights {
                hook: 3.0,
                relevance: 1.0,
                coherence: 0.0,
                standalone: 0.0,
            },
        )
        .unwrap();
        assert_eq!(ranked[0].overall_score, 0.75);
    }
}
