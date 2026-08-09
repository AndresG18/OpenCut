use clip_finder::{
    CandidateEvaluation, ClipProfile, ClipRequest, DurationRange, EvaluationScores, RankingWeights,
    TranscriptSegment, generate_candidates, rank_suggestions,
};
use clips::{Clip, ClipKind, VideoClip};
use ids::{AssetId, ClipId, TimelineId};
use time::RationalTime;
use timelines::Timeline;

fn seconds(value: i64) -> RationalTime {
    RationalTime::new(value, 1).unwrap()
}

#[test]
fn accepted_suggestion_becomes_an_exact_source_range_clip() {
    let transcript = (0..20)
        .map(|index| TranscriptSegment {
            start: seconds(index * 5),
            end: seconds(index * 5 + 5),
            text: format!("Transcript segment {index}."),
            speaker: None,
        })
        .collect::<Vec<_>>();
    let request = ClipRequest {
        profile: ClipProfile {
            id: "one-minute".into(),
            label: "One minute".into(),
            duration: DurationRange::new(seconds(60), seconds(65), seconds(70)).unwrap(),
            aspect_ratio: None,
            instructions: String::new(),
        },
        prompt: "Find a complete lesson".into(),
        suggestion_limit: 1,
        candidate_limit: 50,
        max_overlap_percent: 50,
    };
    let candidates = generate_candidates(&transcript, &request).unwrap();
    let chosen = &candidates[3];
    let suggestions = rank_suggestions(
        &candidates,
        &[CandidateEvaluation {
            candidate_id: chosen.id,
            scores: EvaluationScores {
                hook: 0.9,
                relevance: 0.95,
                coherence: 0.9,
                standalone: 0.85,
            },
            title: "A complete lesson".into(),
            reason: "Clear setup and payoff".into(),
            keywords: vec!["lesson".into()],
        }],
        &request,
        RankingWeights::default(),
    )
    .unwrap();
    let accepted = &suggestions[0].candidate;

    let video = VideoClip::new(AssetId(7), accepted.start).unwrap();
    let clip = Clip::flow(
        ClipId(1),
        ClipKind::Video(video),
        accepted.duration().unwrap(),
    )
    .unwrap();
    let mut timeline = Timeline::new(TimelineId(1), RationalTime::new(30, 1).unwrap());
    let main_track = timeline.main_track_id();
    timeline.insert_clip(main_track, clip).unwrap();

    let inserted = timeline.track(main_track).unwrap().clips().first().unwrap();
    let ClipKind::Video(inserted_video) = &inserted.kind else {
        panic!("accepted clip must stay a video clip");
    };
    assert_eq!(inserted_video.asset_id, AssetId(7));
    assert_eq!(inserted_video.source_start, accepted.start);
    assert_eq!(inserted.duration(), accepted.duration().unwrap());
    assert_eq!(timeline.duration(), accepted.duration().unwrap());
}
