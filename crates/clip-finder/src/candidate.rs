use time::RationalTime;

use crate::{
    CandidateBatch, CandidateId, ClipCandidate, ClipFinderError, ClipRequest, TranscriptSegment,
};

#[derive(Clone, Copy)]
struct Window {
    id: CandidateId,
    start_segment: usize,
    end_segment: usize,
}

/// Generate transcript-aligned windows near the requested target duration.
///
/// At most one window is created for each possible starting segment. If the
/// transcript produces more windows than `candidate_limit`, they are sampled
/// evenly over the source so a multi-hour recording remains bounded without
/// biasing the beginning of the video.
pub fn generate_candidates(
    segments: &[TranscriptSegment],
    request: &ClipRequest,
) -> Result<Vec<ClipCandidate>, ClipFinderError> {
    request.validate()?;
    validate_transcript(segments)?;

    let duration = request.profile.duration;
    let mut windows = Vec::with_capacity(segments.len());

    for start_index in 0..segments.len() {
        let start = segments[start_index].start;
        let mut best: Option<(usize, RationalTime, bool, RationalTime)> = None;

        for (end_index, segment) in segments.iter().enumerate().skip(start_index) {
            let window_duration = segment.end.sub(start)?;
            if duration.max.lt(&window_duration) {
                break;
            }
            if window_duration.lt(&duration.min) {
                continue;
            }

            let distance = absolute_distance(window_duration, duration.target)?;
            let completes_thought = ends_thought(&segment.text);
            let replace = match best {
                None => true,
                Some((_, best_distance, best_completes_thought, best_duration)) => {
                    distance.lt(&best_distance)
                        || (distance == best_distance
                            && completes_thought
                            && !best_completes_thought)
                        || (distance == best_distance
                            && completes_thought == best_completes_thought
                            && best_duration.lt(&window_duration))
                }
            };

            if replace {
                best = Some((end_index + 1, distance, completes_thought, window_duration));
            }
        }

        if let Some((end_segment, _, _, _)) = best {
            windows.push(Window {
                id: CandidateId::new(start_index as u64),
                start_segment: start_index,
                end_segment,
            });
        }
    }

    let sampled = evenly_sample(&windows, request.candidate_limit);
    Ok(sampled
        .into_iter()
        .map(|window| build_candidate(segments, window))
        .collect())
}

/// Split candidates into bounded request payloads for an external scorer.
/// A single candidate larger than `max_transcript_chars` is kept intact in its
/// own batch; exact transcript text is never truncated silently.
pub fn batch_candidates(
    candidates: &[ClipCandidate],
    max_candidates: usize,
    max_transcript_chars: usize,
) -> Result<Vec<CandidateBatch>, ClipFinderError> {
    if max_candidates == 0 || max_transcript_chars == 0 {
        return Err(ClipFinderError::InvalidBatchLimit);
    }

    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_chars: usize = 0;

    for candidate in candidates {
        let candidate_chars = candidate.transcript.chars().count();
        let exceeds_count = current.len() == max_candidates;
        let exceeds_chars = !current.is_empty()
            && current_chars.saturating_add(candidate_chars) > max_transcript_chars;

        if exceeds_count || exceeds_chars {
            batches.push(CandidateBatch {
                candidates: std::mem::take(&mut current),
                transcript_chars: current_chars,
            });
            current_chars = 0;
        }

        current_chars = current_chars.saturating_add(candidate_chars);
        current.push(candidate.clone());
    }

    if !current.is_empty() {
        batches.push(CandidateBatch {
            candidates: current,
            transcript_chars: current_chars,
        });
    }

    Ok(batches)
}

fn validate_transcript(segments: &[TranscriptSegment]) -> Result<(), ClipFinderError> {
    if segments.is_empty() {
        return Err(ClipFinderError::EmptyTranscript);
    }

    let zero = RationalTime::new(0, 1).expect("one is a valid denominator");
    let mut previous_end = None;

    for (index, segment) in segments.iter().enumerate() {
        if segment.text.trim().is_empty() {
            return Err(ClipFinderError::EmptySegmentText { index });
        }
        if segment.start.lt(&zero) {
            return Err(ClipFinderError::NegativeSegmentTime { index });
        }
        if !segment.start.lt(&segment.end) {
            return Err(ClipFinderError::InvalidSegmentDuration { index });
        }
        if previous_end.is_some_and(|end: RationalTime| segment.start.lt(&end)) {
            return Err(ClipFinderError::SegmentsOutOfOrder { index });
        }
        previous_end = Some(segment.end);
    }

    Ok(())
}

fn absolute_distance(
    left: RationalTime,
    right: RationalTime,
) -> Result<RationalTime, ClipFinderError> {
    if left.lt(&right) {
        Ok(right.sub(left)?)
    } else {
        Ok(left.sub(right)?)
    }
}

fn ends_thought(text: &str) -> bool {
    matches!(text.trim_end().chars().last(), Some('.' | '!' | '?'))
}

fn evenly_sample(windows: &[Window], limit: usize) -> Vec<Window> {
    if windows.len() <= limit {
        return windows.to_vec();
    }
    if limit == 1 {
        return vec![windows[windows.len() / 2]];
    }

    (0..limit)
        .map(|sample_index| {
            let index = sample_index * (windows.len() - 1) / (limit - 1);
            windows[index]
        })
        .collect()
}

fn build_candidate(segments: &[TranscriptSegment], window: Window) -> ClipCandidate {
    let selected = &segments[window.start_segment..window.end_segment];
    let transcript = selected
        .iter()
        .map(|segment| segment.text.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let word_count = transcript.split_whitespace().count();

    ClipCandidate {
        id: window.id,
        start: selected[0].start,
        end: selected[selected.len() - 1].end,
        start_segment: window.start_segment,
        end_segment: window.end_segment,
        transcript,
        word_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AspectRatio, ClipProfile, DurationRange};

    fn seconds(value: i64) -> RationalTime {
        RationalTime::new(value, 1).unwrap()
    }

    fn segment(start: i64, end: i64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            start: seconds(start),
            end: seconds(end),
            text: text.into(),
            speaker: None,
        }
    }

    fn request(min: i64, target: i64, max: i64, candidate_limit: usize) -> ClipRequest {
        ClipRequest {
            profile: ClipProfile {
                id: "custom".into(),
                label: "Custom".into(),
                duration: DurationRange::new(seconds(min), seconds(target), seconds(max)).unwrap(),
                aspect_ratio: Some(AspectRatio {
                    width: 9,
                    height: 16,
                }),
                instructions: String::new(),
            },
            prompt: String::new(),
            suggestion_limit: 5,
            candidate_limit,
            max_overlap_percent: 50,
        }
    }

    #[test]
    fn windows_align_to_segments_and_prefer_complete_thoughts() {
        let transcript = vec![
            segment(0, 5, "Here is the setup"),
            segment(5, 10, "and an incomplete idea"),
            segment(10, 16, "that now lands."),
            segment(16, 22, "A new thought begins."),
        ];

        let candidates = generate_candidates(&transcript, &request(10, 15, 18, 20)).unwrap();
        let first = &candidates[0];
        assert_eq!(first.start, seconds(0));
        assert_eq!(first.end, seconds(16));
        assert_eq!(first.start_segment, 0);
        assert_eq!(first.end_segment, 3);
        assert_eq!(
            first.transcript,
            "Here is the setup and an incomplete idea that now lands."
        );
    }

    #[test]
    fn rejects_overlapping_segments() {
        let transcript = vec![segment(0, 5, "First."), segment(4, 8, "Second.")];
        assert_eq!(
            generate_candidates(&transcript, &request(3, 4, 8, 20)),
            Err(ClipFinderError::SegmentsOutOfOrder { index: 1 })
        );
    }

    #[test]
    fn evenly_samples_a_three_hour_transcript() {
        let transcript = (0..2_160)
            .map(|index| {
                let start = index * 5;
                segment(start, start + 5, &format!("Segment {index}."))
            })
            .collect::<Vec<_>>();

        let candidates = generate_candidates(&transcript, &request(55, 61, 67, 300)).unwrap();
        assert_eq!(candidates.len(), 300);
        assert_eq!(candidates.first().unwrap().start, seconds(0));
        assert!(seconds(10_700).lt(&candidates.last().unwrap().start));
        let range = request(55, 61, 67, 300).profile.duration;
        assert!(candidates.iter().all(|candidate| {
            let duration = candidate.duration().unwrap();
            !duration.lt(&range.min) && !range.max.lt(&duration)
        }));
    }

    #[test]
    fn batches_by_count_and_character_budget_without_truncating() {
        let transcript = (0..8)
            .map(|index| segment(index * 5, index * 5 + 5, "12345"))
            .collect::<Vec<_>>();
        let candidates = generate_candidates(&transcript, &request(5, 5, 5, 20)).unwrap();
        let batches = batch_candidates(&candidates, 3, 11).unwrap();

        assert_eq!(batches.len(), 4);
        assert!(batches.iter().all(|batch| batch.candidates.len() <= 2));
        assert_eq!(
            batches
                .iter()
                .map(|batch| batch.candidates.len())
                .sum::<usize>(),
            candidates.len()
        );
    }
}
