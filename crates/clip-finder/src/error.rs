use thiserror::Error;

use crate::CandidateId;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClipFinderError {
    #[error("the transcript has no segments")]
    EmptyTranscript,
    #[error("transcript segment {index} has empty text")]
    EmptySegmentText { index: usize },
    #[error("transcript segment {index} starts before zero")]
    NegativeSegmentTime { index: usize },
    #[error("transcript segment {index} must end after it starts")]
    InvalidSegmentDuration { index: usize },
    #[error("transcript segment {index} overlaps or precedes the previous segment")]
    SegmentsOutOfOrder { index: usize },
    #[error("clip durations must satisfy 0 < min <= target <= max")]
    InvalidDurationRange,
    #[error("clip profile id and label must not be empty")]
    InvalidProfile,
    #[error("the requested suggestion and candidate limits must be greater than zero")]
    InvalidRequestLimit,
    #[error("maximum overlap must be between 0 and 100 percent")]
    InvalidOverlapPercent,
    #[error("candidate batch limits must be greater than zero")]
    InvalidBatchLimit,
    #[error("evaluation references unknown candidate {0}")]
    UnknownCandidate(CandidateId),
    #[error("candidate {0} was evaluated more than once")]
    DuplicateEvaluation(CandidateId),
    #[error(
        "candidate {candidate_id} has an invalid {field} score; scores must be finite and between 0 and 1"
    )]
    InvalidEvaluationScore {
        candidate_id: CandidateId,
        field: &'static str,
    },
    #[error("ranking weights must be finite, non-negative, and have a positive total")]
    InvalidRankingWeights,
    #[error(transparent)]
    Time(#[from] time::TimeError),
}
