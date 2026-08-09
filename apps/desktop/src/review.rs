use clip_finder::{
    AspectRatio, CandidateEvaluation, ClipProfile, ClipRequest, ClipSuggestion, DurationRange,
    EvaluationScores, RankingWeights, TranscriptSegment, generate_candidates, rank_suggestions,
};
use clips::{Clip, ClipKind, VideoClip};
use ids::{AssetId, ClipId, TimelineId};
use time::RationalTime;
use timelines::Timeline as EditorTimeline;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReviewStatus {
    Pending,
    Accepted,
    Skipped,
}

#[derive(Clone, Debug)]
pub(crate) struct ReviewItem {
    pub suggestion: ClipSuggestion,
    pub status: ReviewStatus,
}

/// Shared model observed by both the AI review browser and the timeline panel.
pub(crate) struct ClipReview {
    pub profile_label: String,
    pub source_duration: RationalTime,
    pub items: Vec<ReviewItem>,
    pub timeline: EditorTimeline,
    next_clip_id: u64,
    source_asset_id: AssetId,
}

impl ClipReview {
    pub(crate) fn demo() -> Self {
        let source_duration = seconds(3 * 60 * 60);
        let transcript = (0..2_160)
            .map(|index| TranscriptSegment {
                start: seconds(index * 5),
                end: seconds(index * 5 + 5),
                text: format!(
                    "At this point in the conversation, the speaker develops practical lesson {index}."
                ),
                speaker: Some("Host".into()),
            })
            .collect::<Vec<_>>();
        let request = ClipRequest {
            profile: ClipProfile {
                id: "tiktok-rewards".into(),
                label: "TikTok 61s+".into(),
                duration: DurationRange::new(seconds(61), seconds(65), seconds(75))
                    .expect("demo duration profile is valid"),
                aspect_ratio: Some(AspectRatio {
                    width: 9,
                    height: 16,
                }),
                instructions: "Prioritize a clear hook and a complete payoff.".into(),
            },
            prompt: "Find practical lessons that stand alone for a new viewer.".into(),
            suggestion_limit: 3,
            candidate_limit: 300,
            max_overlap_percent: 35,
        };
        let candidates = generate_candidates(&transcript, &request)
            .expect("the built-in demo transcript is valid");
        let chosen = [30, 150, 270];
        let titles = [
            "The mistake that changed the strategy",
            "A simple framework for deciding faster",
            "Why consistency beats a perfect launch",
        ];
        let reasons = [
            "Strong opening tension followed by a self-contained lesson.",
            "Clear numbered framework with an actionable payoff.",
            "Relatable claim, concrete example, and memorable closing line.",
        ];
        let evaluations = chosen
            .into_iter()
            .enumerate()
            .map(|(index, candidate_index)| CandidateEvaluation {
                candidate_id: candidates[candidate_index].id,
                scores: EvaluationScores {
                    hook: 0.94 - index as f32 * 0.03,
                    relevance: 0.91 - index as f32 * 0.02,
                    coherence: 0.93,
                    standalone: 0.9 + index as f32 * 0.02,
                },
                title: titles[index].into(),
                reason: reasons[index].into(),
                keywords: vec!["creator lesson".into(), "business".into()],
            })
            .collect::<Vec<_>>();
        let suggestions = rank_suggestions(
            &candidates,
            &evaluations,
            &request,
            RankingWeights::default(),
        )
        .expect("the built-in demo evaluations are valid");

        Self {
            profile_label: request.profile.label,
            source_duration,
            items: suggestions
                .into_iter()
                .map(|suggestion| ReviewItem {
                    suggestion,
                    status: ReviewStatus::Pending,
                })
                .collect(),
            timeline: EditorTimeline::new(TimelineId(1), RationalTime::new(30, 1).unwrap()),
            next_clip_id: 1,
            source_asset_id: AssetId(1),
        }
    }

    pub(crate) fn accept(&mut self, index: usize) -> bool {
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        if item.status == ReviewStatus::Accepted {
            return false;
        }

        let candidate = &item.suggestion.candidate;
        let Ok(video) = VideoClip::new(self.source_asset_id, candidate.start) else {
            return false;
        };
        let Ok(clip) = Clip::flow(
            ClipId(self.next_clip_id),
            ClipKind::Video(video),
            candidate
                .duration()
                .expect("validated suggestions have valid durations"),
        ) else {
            return false;
        };
        if self
            .timeline
            .insert_clip(self.timeline.main_track_id(), clip)
            .is_err()
        {
            return false;
        }

        self.next_clip_id = self
            .next_clip_id
            .checked_add(1)
            .expect("review clip ids cannot realistically exhaust u64");
        item.status = ReviewStatus::Accepted;
        true
    }

    pub(crate) fn skip(&mut self, index: usize) -> bool {
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        if item.status != ReviewStatus::Pending {
            return false;
        }
        item.status = ReviewStatus::Skipped;
        true
    }

    pub(crate) fn accepted_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == ReviewStatus::Accepted)
            .count()
    }
}

pub(crate) fn format_timecode(time: RationalTime) -> String {
    let total_seconds = (time.numer() as f64 / time.denom() as f64).floor().max(0.0) as u64;
    let hours = total_seconds / 3_600;
    let minutes = total_seconds % 3_600 / 60;
    let seconds = total_seconds % 60;
    format!("{hours}:{minutes:02}:{seconds:02}")
}

fn seconds(value: i64) -> RationalTime {
    RationalTime::new(value, 1).expect("integer seconds always have a valid denominator")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepting_a_suggestion_updates_the_shared_timeline() {
        let mut review = ClipReview::demo();
        let duration = review.items[0].suggestion.candidate.duration().unwrap();

        assert!(review.accept(0));
        assert_eq!(review.accepted_count(), 1);
        assert_eq!(review.timeline.duration(), duration);
        assert!(!review.accept(0));
    }

    #[test]
    fn formats_long_source_timecodes() {
        assert_eq!(format_timecode(seconds(3_661)), "1:01:01");
    }
}
