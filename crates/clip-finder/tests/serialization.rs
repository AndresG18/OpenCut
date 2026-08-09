use clip_finder::{
    AspectRatio, CandidateId, ClipProfile, ClipRequest, DurationRange,
    REVIEW_PACKAGE_SCHEMA_VERSION, ReviewPackage, TranscriptSegment,
};
use time::RationalTime;

fn seconds(value: i64) -> RationalTime {
    RationalTime::new(value, 1).unwrap()
}

#[test]
fn request_and_transcript_contract_round_trip() {
    let request = ClipRequest {
        profile: ClipProfile {
            id: "tiktok-rewards".into(),
            label: "TikTok Creator Rewards".into(),
            duration: DurationRange::new(seconds(61), seconds(65), seconds(75)).unwrap(),
            aspect_ratio: Some(AspectRatio {
                width: 9,
                height: 16,
            }),
            instructions: "Prioritize a clear hook and a complete payoff.".into(),
        },
        prompt: "Find practical founder lessons".into(),
        suggestion_limit: 8,
        candidate_limit: 300,
        max_overlap_percent: 35,
    };
    let json = serde_json::to_string(&request).unwrap();
    let restored: ClipRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, request);

    let segment = TranscriptSegment {
        start: seconds(90),
        end: seconds(95),
        text: "The key lesson.".into(),
        speaker: Some("Host".into()),
    };
    let json = serde_json::to_string(&segment).unwrap();
    assert_eq!(
        serde_json::from_str::<TranscriptSegment>(&json).unwrap(),
        segment
    );
    assert_eq!(serde_json::to_string(&CandidateId::new(42)).unwrap(), "42");
}

#[test]
fn empty_review_package_contract_round_trips() {
    let package = ReviewPackage {
        schema_version: REVIEW_PACKAGE_SCHEMA_VERSION,
        source_transcript: "/tmp/transcript.json".into(),
        source_duration: seconds(10_800),
        request: ClipRequest {
            profile: ClipProfile {
                id: "reel".into(),
                label: "Quick Reel".into(),
                duration: DurationRange::new(seconds(15), seconds(20), seconds(30)).unwrap(),
                aspect_ratio: Some(AspectRatio {
                    width: 9,
                    height: 16,
                }),
                instructions: String::new(),
            },
            prompt: "Find surprising lessons".into(),
            suggestion_limit: 5,
            candidate_limit: 100,
            max_overlap_percent: 35,
        },
        suggestions: Vec::new(),
        generated_by: "test-agent".into(),
    };

    let json = serde_json::to_string(&package).unwrap();
    assert_eq!(
        serde_json::from_str::<ReviewPackage>(&json).unwrap(),
        package
    );
}
