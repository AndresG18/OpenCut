use serde::{Deserialize, Serialize};
use time::RationalTime;

use crate::ClipFinderError;

/// Current on-disk schema written by agent integrations and read by OpenCut.
pub const REVIEW_PACKAGE_SCHEMA_VERSION: u32 = 1;

/// One contiguous, time-coded portion of a transcript.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start: RationalTime,
    pub end: RationalTime,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// Accepted duration bounds for a generated clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurationRange {
    pub min: RationalTime,
    pub target: RationalTime,
    pub max: RationalTime,
}

impl DurationRange {
    pub fn new(
        min: RationalTime,
        target: RationalTime,
        max: RationalTime,
    ) -> Result<Self, ClipFinderError> {
        let range = Self { min, target, max };
        range.validate()?;
        Ok(range)
    }

    pub(crate) fn validate(&self) -> Result<(), ClipFinderError> {
        let zero = RationalTime::new(0, 1).expect("one is a valid denominator");
        if !zero.lt(&self.min) || self.target.lt(&self.min) || self.max.lt(&self.target) {
            return Err(ClipFinderError::InvalidDurationRange);
        }
        Ok(())
    }
}

/// Output aspect ratio requested by a publishing profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AspectRatio {
    pub width: u32,
    pub height: u32,
}

/// Data-driven publishing profile. Platform-specific duration guidance lives
/// in configuration as values of this type, not in candidate-generation code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipProfile {
    pub id: String,
    pub label: String,
    pub duration: DurationRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<AspectRatio>,
    #[serde(default)]
    pub instructions: String,
}

/// One clip-finding run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipRequest {
    pub profile: ClipProfile,
    #[serde(default)]
    pub prompt: String,
    pub suggestion_limit: usize,
    pub candidate_limit: usize,
    /// Maximum permitted overlap with an already selected suggestion.
    pub max_overlap_percent: u8,
}

impl ClipRequest {
    pub fn validate(&self) -> Result<(), ClipFinderError> {
        if self.profile.id.trim().is_empty() || self.profile.label.trim().is_empty() {
            return Err(ClipFinderError::InvalidProfile);
        }
        self.profile.duration.validate()?;
        if self.suggestion_limit == 0 || self.candidate_limit == 0 {
            return Err(ClipFinderError::InvalidRequestLimit);
        }
        if self.max_overlap_percent > 100 {
            return Err(ClipFinderError::InvalidOverlapPercent);
        }
        Ok(())
    }
}

/// Stable identifier for a candidate within one transcript analysis run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateId(u64);

impl CandidateId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for CandidateId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A transcript-aligned window ready to be evaluated by an AI provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipCandidate {
    pub id: CandidateId,
    pub start: RationalTime,
    pub end: RationalTime,
    pub start_segment: usize,
    /// Exclusive segment index.
    pub end_segment: usize,
    pub transcript: String,
    pub word_count: usize,
}

impl ClipCandidate {
    pub fn duration(&self) -> Result<RationalTime, ClipFinderError> {
        Ok(self.end.sub(self.start)?)
    }
}

/// A bounded payload suitable for one external-model request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateBatch {
    pub candidates: Vec<ClipCandidate>,
    pub transcript_chars: usize,
}

/// Provider-returned quality dimensions. Keeping dimensions separate makes
/// ranking policy configurable without asking the model to rescore candidates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationScores {
    pub hook: f32,
    pub relevance: f32,
    pub coherence: f32,
    pub standalone: f32,
}

/// An external model's assessment of one candidate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateEvaluation {
    pub candidate_id: CandidateId,
    pub scores: EvaluationScores,
    pub title: String,
    pub reason: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Configurable weights used to turn dimensions into a review ordering.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankingWeights {
    pub hook: f32,
    pub relevance: f32,
    pub coherence: f32,
    pub standalone: f32,
}

impl Default for RankingWeights {
    fn default() -> Self {
        Self {
            hook: 0.3,
            relevance: 0.3,
            coherence: 0.2,
            standalone: 0.2,
        }
    }
}

impl RankingWeights {
    pub(crate) fn validate(self) -> Result<f32, ClipFinderError> {
        let values = [self.hook, self.relevance, self.coherence, self.standalone];
        if values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(ClipFinderError::InvalidRankingWeights);
        }
        let total = values.iter().sum::<f32>();
        if !total.is_finite() || total <= 0.0 {
            return Err(ClipFinderError::InvalidRankingWeights);
        }
        Ok(total)
    }
}

/// A validated, ranked item for human review. Accepting one can later become a
/// timeline command; rejection never mutates the source project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipSuggestion {
    pub candidate: ClipCandidate,
    pub title: String,
    pub reason: String,
    pub keywords: Vec<String>,
    pub scores: EvaluationScores,
    pub overall_score: f32,
}

/// Portable handoff between an external clip-finding agent and the editor.
///
/// This package contains suggestions only. It never mutates a project or
/// timeline until the user explicitly accepts an item in OpenCut.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewPackage {
    pub schema_version: u32,
    pub source_transcript: String,
    pub source_duration: RationalTime,
    pub request: ClipRequest,
    pub suggestions: Vec<ClipSuggestion>,
    pub generated_by: String,
}
