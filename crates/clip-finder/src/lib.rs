//! Transcript-driven clip discovery.
//!
//! This crate owns the deterministic part of the workflow:
//!
//! 1. validate a time-coded transcript;
//! 2. build duration-constrained candidates on transcript boundaries;
//! 3. batch candidates for an external scorer;
//! 4. validate scorer output and produce a non-redundant review queue.
//!
//! It deliberately does not choose an AI provider or transcribe media. A
//! local model, hosted model, or test scorer can all consume the same
//! [`ClipCandidate`] contract without changing editor data or exact timecodes.

mod candidate;
mod error;
mod model;
mod ranking;

pub use candidate::{batch_candidates, generate_candidates};
pub use error::ClipFinderError;
pub use model::{
    AspectRatio, CandidateBatch, CandidateEvaluation, CandidateId, ClipCandidate, ClipProfile,
    ClipRequest, ClipSuggestion, DurationRange, EvaluationScores, RankingWeights,
    TranscriptSegment,
};
pub use ranking::rank_suggestions;
