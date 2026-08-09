//! Local MCP tools for transcript-driven social clip discovery.
//!
//! The MCP client supplies the creative judgment. This server owns file IO,
//! exact timestamps, bounded batching, validation, deterministic ranking, and
//! the portable review package consumed by OpenCut.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use clip_finder::{
    AspectRatio, CandidateBatch, CandidateEvaluation, CandidateId, ClipCandidate, ClipProfile,
    ClipRequest, DurationRange, EvaluationScores, REVIEW_PACKAGE_SCHEMA_VERSION, RankingWeights,
    ReviewPackage, TranscriptSegment, batch_candidates, generate_candidates, rank_suggestions,
};
use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    schemars::{self, JsonSchema},
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use time::RationalTime;

const MAX_TRANSCRIPT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CANDIDATES: usize = 2_000;
const MAX_SUGGESTIONS: usize = 100;
const MAX_BATCH_CANDIDATES: usize = 100;
const MAX_BATCH_CHARS: usize = 250_000;

#[derive(Debug, Default)]
struct ServerState {
    next_session: u64,
    active: Option<DiscoverySession>,
}

#[derive(Debug)]
struct DiscoverySession {
    id: String,
    source_transcript: String,
    source_duration: RationalTime,
    request: ClipRequest,
    candidates: Vec<ClipCandidate>,
    batches: Vec<CandidateBatch>,
    evaluations: HashMap<CandidateId, CandidateEvaluation>,
}

#[derive(Debug, Clone)]
pub struct OpenCutServer {
    tool_router: ToolRouter<Self>,
    state: Arc<Mutex<ServerState>>,
}

impl Default for OpenCutServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = tool_router)]
impl OpenCutServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            state: Arc::new(Mutex::new(ServerState::default())),
        }
    }

    #[tool(
        description = "Read an exact-millisecond transcript JSON file, generate bounded duration-constrained clip candidates, and start one local discovery session. Starting again replaces the active session."
    )]
    pub async fn start_clip_discovery(
        &self,
        Parameters(params): Parameters<StartClipDiscoveryParams>,
    ) -> Result<Json<StartClipDiscoveryOutput>, String> {
        validate_start_limits(&params)?;
        let (source_path, transcript) = load_transcript(&params.transcript_path)?;
        let source_duration = transcript
            .last()
            .map(|segment| segment.end)
            .ok_or_else(|| "the transcript has no segments".to_string())?;
        let request = params.to_request()?;
        let candidates = generate_candidates(&transcript, &request).map_err(error_string)?;
        if candidates.is_empty() {
            return Err(
                "no candidate fits the requested duration; widen the duration range or verify transcript coverage"
                    .into(),
            );
        }
        let batches = batch_candidates(
            &candidates,
            params.batch_candidate_limit,
            params.batch_transcript_char_limit,
        )
        .map_err(error_string)?;

        let mut state = self.lock_state()?;
        state.next_session = state
            .next_session
            .checked_add(1)
            .ok_or_else(|| "clip discovery session counter overflowed".to_string())?;
        let id = format!("clip-discovery-{}", state.next_session);
        let output = StartClipDiscoveryOutput {
            session_id: id.clone(),
            source_transcript: source_path.clone(),
            source_duration_ms: rational_to_millis(source_duration)?,
            candidate_count: candidates.len(),
            batch_count: batches.len(),
            suggestion_limit: request.suggestion_limit,
            next_action: "Call get_candidate_batch for every batch index, score every candidate, then submit_candidate_evaluations.".into(),
        };
        state.active = Some(DiscoverySession {
            id,
            source_transcript: source_path,
            source_duration,
            request,
            candidates,
            batches,
            evaluations: HashMap::new(),
        });

        Ok(Json(output))
    }

    #[tool(
        description = "Return one repeatable candidate batch with timestamps, transcript text, the user prompt, profile instructions, and the 0-to-1 scoring rubric. Batch indexes are zero-based."
    )]
    pub async fn get_candidate_batch(
        &self,
        Parameters(params): Parameters<GetCandidateBatchParams>,
    ) -> Result<Json<CandidateBatchOutput>, String> {
        let state = self.lock_state()?;
        let session = active_session(&state, &params.session_id)?;
        let batch = session.batches.get(params.batch_index).ok_or_else(|| {
            format!(
                "batch index {} is out of range; this session has {} batches",
                params.batch_index,
                session.batches.len()
            )
        })?;
        let candidates = batch
            .candidates
            .iter()
            .map(|candidate| {
                CandidateView::from_candidate(
                    candidate,
                    session.evaluations.contains_key(&candidate.id),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Json(CandidateBatchOutput {
            session_id: session.id.clone(),
            batch_index: params.batch_index,
            batch_count: session.batches.len(),
            prompt: session.request.prompt.clone(),
            profile_label: session.request.profile.label.clone(),
            profile_instructions: session.request.profile.instructions.clone(),
            rubric: ScoringRubric::default(),
            candidates,
        }))
    }

    #[tool(
        description = "Validate and store AI judgments for candidates in the active session. Scores must be between 0 and 1. Submissions are atomic and existing candidate evaluations cannot be silently replaced."
    )]
    pub async fn submit_candidate_evaluations(
        &self,
        Parameters(params): Parameters<SubmitCandidateEvaluationsParams>,
    ) -> Result<Json<SubmitCandidateEvaluationsOutput>, String> {
        if params.evaluations.is_empty() {
            return Err("submit at least one candidate evaluation".into());
        }

        let mut state = self.lock_state()?;
        let session = active_session_mut(&mut state, &params.session_id)?;
        let known_ids = session
            .candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect::<HashSet<_>>();
        let mut submitted_ids = HashSet::new();
        let mut validated = Vec::with_capacity(params.evaluations.len());

        for input in params.evaluations {
            let candidate_id = CandidateId::new(input.candidate_id);
            if !known_ids.contains(&candidate_id) {
                return Err(format!(
                    "candidate {} is not part of this session",
                    input.candidate_id
                ));
            }
            if session.evaluations.contains_key(&candidate_id) {
                return Err(format!(
                    "candidate {} already has an evaluation; start a new discovery session to rescore it",
                    input.candidate_id
                ));
            }
            if !submitted_ids.insert(candidate_id) {
                return Err(format!(
                    "candidate {} appears more than once in this submission",
                    input.candidate_id
                ));
            }
            validated.push(input.into_evaluation(candidate_id)?);
        }

        let accepted_count = validated.len();
        for evaluation in validated {
            session
                .evaluations
                .insert(evaluation.candidate_id, evaluation);
        }
        let evaluated_count = session.evaluations.len();
        let candidate_count = session.candidates.len();

        Ok(Json(SubmitCandidateEvaluationsOutput {
            session_id: session.id.clone(),
            accepted_count,
            evaluated_count,
            remaining_count: candidate_count.saturating_sub(evaluated_count),
            ready_to_finalize: evaluated_count == candidate_count,
        }))
    }

    #[tool(
        description = "Inspect progress for the active clip discovery session, including which zero-based batches still contain unevaluated candidates."
    )]
    pub async fn clip_discovery_status(
        &self,
        Parameters(params): Parameters<ClipDiscoveryStatusParams>,
    ) -> Result<Json<ClipDiscoveryStatusOutput>, String> {
        let state = self.lock_state()?;
        let session = active_session(&state, &params.session_id)?;
        let unevaluated_batch_indexes = session
            .batches
            .iter()
            .enumerate()
            .filter_map(|(index, batch)| {
                batch
                    .candidates
                    .iter()
                    .any(|candidate| !session.evaluations.contains_key(&candidate.id))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let evaluated_count = session.evaluations.len();

        Ok(Json(ClipDiscoveryStatusOutput {
            session_id: session.id.clone(),
            source_transcript: session.source_transcript.clone(),
            candidate_count: session.candidates.len(),
            evaluated_count,
            remaining_count: session.candidates.len().saturating_sub(evaluated_count),
            batch_count: session.batches.len(),
            unevaluated_batch_indexes,
            ready_to_finalize: evaluated_count == session.candidates.len(),
        }))
    }

    #[tool(
        description = "Rank and overlap-filter submitted evaluations locally, return the review queue, and optionally write an OpenCut review package JSON file. By default every candidate must be evaluated and an existing output file is never overwritten."
    )]
    pub async fn finalize_clip_discovery(
        &self,
        Parameters(params): Parameters<FinalizeClipDiscoveryParams>,
    ) -> Result<Json<FinalizeClipDiscoveryOutput>, String> {
        let state = self.lock_state()?;
        let session = active_session(&state, &params.session_id)?;
        let remaining_count = session
            .candidates
            .len()
            .saturating_sub(session.evaluations.len());
        if remaining_count > 0 && !params.allow_partial {
            return Err(format!(
                "{} candidates remain unevaluated; score every batch or explicitly set allow_partial to true",
                remaining_count
            ));
        }
        if session.evaluations.is_empty() {
            return Err("no candidate evaluations have been submitted".into());
        }

        let evaluations = session
            .candidates
            .iter()
            .filter_map(|candidate| session.evaluations.get(&candidate.id).cloned())
            .collect::<Vec<_>>();
        let weights = params
            .weights
            .map(RankingWeightsInput::into_weights)
            .unwrap_or_default();
        let suggestions =
            rank_suggestions(&session.candidates, &evaluations, &session.request, weights)
                .map_err(error_string)?;
        let package = ReviewPackage {
            schema_version: REVIEW_PACKAGE_SCHEMA_VERSION,
            source_transcript: session.source_transcript.clone(),
            source_duration: session.source_duration,
            request: session.request.clone(),
            suggestions: suggestions.clone(),
            generated_by: "opencut-mcp".into(),
        };
        let package_path = params
            .output_path
            .as_deref()
            .map(|path| {
                write_review_package(path, &session.source_transcript, &package, params.overwrite)
            })
            .transpose()?;
        let suggestion_views = suggestions
            .iter()
            .map(SuggestionView::from_suggestion)
            .collect::<Result<Vec<_>, _>>()?;
        let warning = (remaining_count > 0).then(|| {
            format!(
                "Finalized from a partial evaluation set; {} candidates were not scored.",
                remaining_count
            )
        });

        Ok(Json(FinalizeClipDiscoveryOutput {
            session_id: session.id.clone(),
            schema_version: REVIEW_PACKAGE_SCHEMA_VERSION,
            package_path,
            suggestion_count: suggestion_views.len(),
            warning,
            suggestions: suggestion_views,
            next_action: "Open the package in OpenCut for human review; accepting a suggestion is the only step that changes the timeline.".into(),
        }))
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, ServerState>, String> {
        self.state
            .lock()
            .map_err(|_| "the clip discovery session lock is poisoned".to_string())
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "opencut",
    version = "0.1.0",
    instructions = "Use these tools to turn a local exact-millisecond transcript into a bounded, AI-scored OpenCut review queue. Score candidates using your current agent model; OpenCut never asks for or stores the agent subscription credential."
)]
impl ServerHandler for OpenCutServer {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartClipDiscoveryParams {
    #[schemars(
        description = "Absolute or current-working-directory-relative path to transcript JSON. The file must contain an array, or an object with a segments array, of {start_ms,end_ms,text,speaker?} records."
    )]
    pub transcript_path: String,
    #[schemars(description = "What kinds of moments the user wants the agent to find.")]
    pub prompt: String,
    pub profile: ClipProfileInput,
    #[serde(default = "default_suggestion_limit")]
    pub suggestion_limit: usize,
    #[serde(default = "default_candidate_limit")]
    pub candidate_limit: usize,
    #[serde(default = "default_overlap_percent")]
    pub max_overlap_percent: u8,
    #[serde(default = "default_batch_candidate_limit")]
    pub batch_candidate_limit: usize,
    #[serde(default = "default_batch_char_limit")]
    pub batch_transcript_char_limit: usize,
}

impl StartClipDiscoveryParams {
    fn to_request(&self) -> Result<ClipRequest, String> {
        let aspect_ratio = match (self.profile.aspect_width, self.profile.aspect_height) {
            (None, None) => None,
            (Some(width), Some(height)) if width > 0 && height > 0 => {
                Some(AspectRatio { width, height })
            }
            _ => {
                return Err(
                    "aspect_width and aspect_height must either both be positive or both be omitted"
                        .into(),
                );
            }
        };
        let duration = DurationRange::new(
            millis_to_rational(self.profile.min_duration_ms)?,
            millis_to_rational(self.profile.target_duration_ms)?,
            millis_to_rational(self.profile.max_duration_ms)?,
        )
        .map_err(error_string)?;
        let request = ClipRequest {
            profile: ClipProfile {
                id: self.profile.id.trim().to_string(),
                label: self.profile.label.trim().to_string(),
                duration,
                aspect_ratio,
                instructions: self.profile.instructions.trim().to_string(),
            },
            prompt: self.prompt.trim().to_string(),
            suggestion_limit: self.suggestion_limit,
            candidate_limit: self.candidate_limit,
            max_overlap_percent: self.max_overlap_percent,
        };
        request.validate().map_err(error_string)?;
        Ok(request)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipProfileInput {
    #[schemars(description = "Stable lowercase profile id, such as tiktok-61s or quick-reel.")]
    pub id: String,
    pub label: String,
    pub min_duration_ms: i64,
    pub target_duration_ms: i64,
    pub max_duration_ms: i64,
    #[serde(default)]
    pub aspect_width: Option<u32>,
    #[serde(default)]
    pub aspect_height: Option<u32>,
    #[serde(default)]
    pub instructions: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct StartClipDiscoveryOutput {
    pub session_id: String,
    pub source_transcript: String,
    pub source_duration_ms: i64,
    pub candidate_count: usize,
    pub batch_count: usize,
    pub suggestion_limit: usize,
    pub next_action: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCandidateBatchParams {
    pub session_id: String,
    #[schemars(
        description = "Zero-based batch index. Repeating the same index returns the same candidates."
    )]
    pub batch_index: usize,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CandidateBatchOutput {
    pub session_id: String,
    pub batch_index: usize,
    pub batch_count: usize,
    pub prompt: String,
    pub profile_label: String,
    pub profile_instructions: String,
    pub rubric: ScoringRubric,
    pub candidates: Vec<CandidateView>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CandidateView {
    pub candidate_id: u64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub duration_ms: i64,
    pub transcript: String,
    pub word_count: usize,
    pub already_evaluated: bool,
}

impl CandidateView {
    fn from_candidate(candidate: &ClipCandidate, already_evaluated: bool) -> Result<Self, String> {
        Ok(Self {
            candidate_id: candidate.id.raw(),
            start_ms: rational_to_millis(candidate.start)?,
            end_ms: rational_to_millis(candidate.end)?,
            duration_ms: rational_to_millis(candidate.duration().map_err(error_string)?)?,
            transcript: candidate.transcript.clone(),
            word_count: candidate.word_count,
            already_evaluated,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ScoringRubric {
    pub hook: String,
    pub relevance: String,
    pub coherence: String,
    pub standalone: String,
}

impl ScoringRubric {
    fn default() -> Self {
        Self {
            hook: "0 to 1: strength of the opening in stopping a scroll".into(),
            relevance: "0 to 1: fit to the user's prompt and publishing profile".into(),
            coherence: "0 to 1: completeness and clarity of the idea".into(),
            standalone: "0 to 1: how well it works without surrounding context".into(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubmitCandidateEvaluationsParams {
    pub session_id: String,
    pub evaluations: Vec<CandidateEvaluationInput>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CandidateEvaluationInput {
    pub candidate_id: u64,
    pub hook: f32,
    pub relevance: f32,
    pub coherence: f32,
    pub standalone: f32,
    #[schemars(description = "Concise review-card title, at most 120 characters.")]
    pub title: String,
    #[schemars(
        description = "Specific reason the moment fits the request, at most 500 characters."
    )]
    pub reason: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

impl CandidateEvaluationInput {
    fn into_evaluation(self, candidate_id: CandidateId) -> Result<CandidateEvaluation, String> {
        for (field, value) in [
            ("hook", self.hook),
            ("relevance", self.relevance),
            ("coherence", self.coherence),
            ("standalone", self.standalone),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!(
                    "candidate {} has invalid {} score; use a number from 0 to 1",
                    candidate_id.raw(),
                    field
                ));
            }
        }
        let title = self.title.trim().to_string();
        let reason = self.reason.trim().to_string();
        if title.is_empty() || title.chars().count() > 120 {
            return Err(format!(
                "candidate {} title must contain 1 to 120 characters",
                candidate_id.raw()
            ));
        }
        if reason.is_empty() || reason.chars().count() > 500 {
            return Err(format!(
                "candidate {} reason must contain 1 to 500 characters",
                candidate_id.raw()
            ));
        }
        if self.keywords.len() > 12
            || self
                .keywords
                .iter()
                .any(|keyword| keyword.trim().is_empty() || keyword.chars().count() > 80)
        {
            return Err(format!(
                "candidate {} may have at most 12 non-empty keywords of at most 80 characters",
                candidate_id.raw()
            ));
        }

        Ok(CandidateEvaluation {
            candidate_id,
            scores: EvaluationScores {
                hook: self.hook,
                relevance: self.relevance,
                coherence: self.coherence,
                standalone: self.standalone,
            },
            title,
            reason,
            keywords: self
                .keywords
                .into_iter()
                .map(|keyword| keyword.trim().to_string())
                .collect(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SubmitCandidateEvaluationsOutput {
    pub session_id: String,
    pub accepted_count: usize,
    pub evaluated_count: usize,
    pub remaining_count: usize,
    pub ready_to_finalize: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipDiscoveryStatusParams {
    pub session_id: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ClipDiscoveryStatusOutput {
    pub session_id: String,
    pub source_transcript: String,
    pub candidate_count: usize,
    pub evaluated_count: usize,
    pub remaining_count: usize,
    pub batch_count: usize,
    pub unevaluated_batch_indexes: Vec<usize>,
    pub ready_to_finalize: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FinalizeClipDiscoveryParams {
    pub session_id: String,
    #[serde(default)]
    pub weights: Option<RankingWeightsInput>,
    #[schemars(
        description = "Optional absolute or current-working-directory-relative .json path. No file is written when omitted."
    )]
    #[serde(default)]
    pub output_path: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub allow_partial: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RankingWeightsInput {
    pub hook: f32,
    pub relevance: f32,
    pub coherence: f32,
    pub standalone: f32,
}

impl RankingWeightsInput {
    fn into_weights(self) -> RankingWeights {
        RankingWeights {
            hook: self.hook,
            relevance: self.relevance,
            coherence: self.coherence,
            standalone: self.standalone,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FinalizeClipDiscoveryOutput {
    pub session_id: String,
    pub schema_version: u32,
    pub package_path: Option<String>,
    pub suggestion_count: usize,
    pub warning: Option<String>,
    pub suggestions: Vec<SuggestionView>,
    pub next_action: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SuggestionView {
    pub candidate_id: u64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub duration_ms: i64,
    pub title: String,
    pub reason: String,
    pub keywords: Vec<String>,
    pub overall_score: f32,
}

impl SuggestionView {
    fn from_suggestion(suggestion: &clip_finder::ClipSuggestion) -> Result<Self, String> {
        Ok(Self {
            candidate_id: suggestion.candidate.id.raw(),
            start_ms: rational_to_millis(suggestion.candidate.start)?,
            end_ms: rational_to_millis(suggestion.candidate.end)?,
            duration_ms: rational_to_millis(
                suggestion.candidate.duration().map_err(error_string)?,
            )?,
            title: suggestion.title.clone(),
            reason: suggestion.reason.clone(),
            keywords: suggestion.keywords.clone(),
            overall_score: suggestion.overall_score,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TranscriptDocument {
    Segments(Vec<TranscriptSegmentInput>),
    Wrapped {
        segments: Vec<TranscriptSegmentInput>,
    },
}

impl TranscriptDocument {
    fn into_segments(self) -> Vec<TranscriptSegmentInput> {
        match self {
            Self::Segments(segments) | Self::Wrapped { segments } => segments,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptSegmentInput {
    #[serde(alias = "startMs")]
    start_ms: i64,
    #[serde(alias = "endMs")]
    end_ms: i64,
    text: String,
    #[serde(default)]
    speaker: Option<String>,
}

fn validate_start_limits(params: &StartClipDiscoveryParams) -> Result<(), String> {
    if params.prompt.trim().is_empty() {
        return Err("prompt must not be empty".into());
    }
    if params.suggestion_limit == 0 || params.suggestion_limit > MAX_SUGGESTIONS {
        return Err(format!(
            "suggestion_limit must be between 1 and {MAX_SUGGESTIONS}"
        ));
    }
    if params.candidate_limit == 0 || params.candidate_limit > MAX_CANDIDATES {
        return Err(format!(
            "candidate_limit must be between 1 and {MAX_CANDIDATES}"
        ));
    }
    if params.batch_candidate_limit == 0 || params.batch_candidate_limit > MAX_BATCH_CANDIDATES {
        return Err(format!(
            "batch_candidate_limit must be between 1 and {MAX_BATCH_CANDIDATES}"
        ));
    }
    if params.batch_transcript_char_limit == 0
        || params.batch_transcript_char_limit > MAX_BATCH_CHARS
    {
        return Err(format!(
            "batch_transcript_char_limit must be between 1 and {MAX_BATCH_CHARS}"
        ));
    }
    Ok(())
}

fn load_transcript(path: &str) -> Result<(String, Vec<TranscriptSegment>), String> {
    if path.trim().is_empty() {
        return Err("transcript_path must not be empty".into());
    }
    let path = PathBuf::from(path);
    let metadata = fs::metadata(&path).map_err(|error| {
        format!(
            "cannot read transcript metadata at {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!("transcript path is not a file: {}", path.display()));
    }
    if metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Err(format!(
            "transcript is larger than the {} MiB safety limit",
            MAX_TRANSCRIPT_BYTES / 1024 / 1024
        ));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("cannot resolve transcript path {}: {error}", path.display()))?;
    let contents = fs::read_to_string(&canonical)
        .map_err(|error| format!("cannot read transcript {}: {error}", canonical.display()))?;
    let document: TranscriptDocument = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "invalid transcript JSON at {}: {error}; expected start_ms, end_ms, text, and optional speaker",
            canonical.display()
        )
    })?;
    let segments = document
        .into_segments()
        .into_iter()
        .map(|segment| {
            Ok(TranscriptSegment {
                start: millis_to_rational(segment.start_ms)?,
                end: millis_to_rational(segment.end_ms)?,
                text: segment.text,
                speaker: segment.speaker,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok((canonical.to_string_lossy().into_owned(), segments))
}

fn active_session<'a>(
    state: &'a ServerState,
    requested_id: &str,
) -> Result<&'a DiscoverySession, String> {
    let session = state.active.as_ref().ok_or_else(|| {
        "no active clip discovery session; call start_clip_discovery first".to_string()
    })?;
    if session.id != requested_id {
        return Err(format!(
            "session {} is no longer active; the active session is {}",
            requested_id, session.id
        ));
    }
    Ok(session)
}

fn active_session_mut<'a>(
    state: &'a mut ServerState,
    requested_id: &str,
) -> Result<&'a mut DiscoverySession, String> {
    let session = state.active.as_mut().ok_or_else(|| {
        "no active clip discovery session; call start_clip_discovery first".to_string()
    })?;
    if session.id != requested_id {
        return Err(format!(
            "session {} is no longer active; the active session is {}",
            requested_id, session.id
        ));
    }
    Ok(session)
}

fn write_review_package(
    output_path: &str,
    source_transcript: &str,
    package: &ReviewPackage,
    overwrite: bool,
) -> Result<String, String> {
    if output_path.trim().is_empty() {
        return Err("output_path must not be empty when provided".into());
    }
    let path = PathBuf::from(output_path);
    if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err("output_path must end in .json".into());
    }
    if path.exists()
        && fs::canonicalize(&path).ok().as_deref() == Some(Path::new(source_transcript))
    {
        return Err("output_path must not replace the source transcript".into());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty() && !parent.is_dir())
    {
        return Err(format!(
            "output directory does not exist: {}",
            parent.display()
        ));
    }
    let bytes = serde_json::to_vec_pretty(package)
        .map_err(|error| format!("cannot serialize review package: {error}"))?;
    let mut options = OpenOptions::new();
    options.write(true);
    if overwrite {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(&path).map_err(|error| {
        if path.exists() && !overwrite {
            format!(
                "review package already exists at {}; set overwrite to true only with user approval",
                path.display()
            )
        } else {
            format!("cannot create review package at {}: {error}", path.display())
        }
    })?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|error| format!("cannot write review package at {}: {error}", path.display()))?;
    let resolved = fs::canonicalize(&path).unwrap_or(path);
    Ok(resolved.to_string_lossy().into_owned())
}

fn millis_to_rational(milliseconds: i64) -> Result<RationalTime, String> {
    RationalTime::new(milliseconds, 1_000).map_err(error_string)
}

fn rational_to_millis(time: RationalTime) -> Result<i64, String> {
    let scaled = i128::from(time.numer())
        .checked_mul(1_000)
        .ok_or_else(|| "timestamp overflows milliseconds".to_string())?;
    let denominator = i128::from(time.denom());
    if scaled % denominator != 0 {
        return Err("timestamp cannot be represented as exact milliseconds".into());
    }
    i64::try_from(scaled / denominator).map_err(|_| "timestamp overflows milliseconds".to_string())
}

fn error_string(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn default_suggestion_limit() -> usize {
    8
}

fn default_candidate_limit() -> usize {
    300
}

fn default_overlap_percent() -> u8 {
    35
}

fn default_batch_candidate_limit() -> usize {
    20
}

fn default_batch_char_limit() -> usize {
    40_000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(path: &Path) -> StartClipDiscoveryParams {
        StartClipDiscoveryParams {
            transcript_path: path.to_string_lossy().into_owned(),
            prompt: "Find practical lessons with a strong hook".into(),
            profile: ClipProfileInput {
                id: "quick-reel".into(),
                label: "Quick Reel".into(),
                min_duration_ms: 10_000,
                target_duration_ms: 15_000,
                max_duration_ms: 20_000,
                aspect_width: Some(9),
                aspect_height: Some(16),
                instructions: "Prefer a complete payoff.".into(),
            },
            suggestion_limit: 2,
            candidate_limit: 10,
            max_overlap_percent: 35,
            batch_candidate_limit: 2,
            batch_transcript_char_limit: 10_000,
        }
    }

    fn write_transcript(directory: &Path) -> PathBuf {
        let path = directory.join("transcript.json");
        let segments = (0..8)
            .map(|index| {
                serde_json::json!({
                    "start_ms": index * 5_000,
                    "end_ms": index * 5_000 + 5_000,
                    "text": format!("Practical lesson number {index} lands clearly."),
                    "speaker": "Host"
                })
            })
            .collect::<Vec<_>>();
        fs::write(&path, serde_json::to_vec(&segments).unwrap()).unwrap();
        path
    }

    fn evaluation(candidate_id: u64, score: f32) -> CandidateEvaluationInput {
        CandidateEvaluationInput {
            candidate_id,
            hook: score,
            relevance: score,
            coherence: score,
            standalone: score,
            title: format!("Lesson {candidate_id}"),
            reason: "It has a clear hook and complete takeaway.".into(),
            keywords: vec!["lesson".into()],
        }
    }

    #[tokio::test]
    async fn full_discovery_writes_a_review_package() {
        let directory = tempfile::tempdir().unwrap();
        let transcript_path = write_transcript(directory.path());
        let output_path = directory.path().join("review.json");
        let server = OpenCutServer::new();

        let started = server
            .start_clip_discovery(Parameters(params(&transcript_path)))
            .await
            .unwrap()
            .0;
        assert!(started.candidate_count > 0);
        assert!(started.batch_count > 1);

        for batch_index in 0..started.batch_count {
            let batch = server
                .get_candidate_batch(Parameters(GetCandidateBatchParams {
                    session_id: started.session_id.clone(),
                    batch_index,
                }))
                .await
                .unwrap()
                .0;
            server
                .submit_candidate_evaluations(Parameters(SubmitCandidateEvaluationsParams {
                    session_id: started.session_id.clone(),
                    evaluations: batch
                        .candidates
                        .iter()
                        .map(|candidate| evaluation(candidate.candidate_id, 0.8))
                        .collect(),
                }))
                .await
                .unwrap();
        }

        let finalized = server
            .finalize_clip_discovery(Parameters(FinalizeClipDiscoveryParams {
                session_id: started.session_id,
                weights: None,
                output_path: Some(output_path.to_string_lossy().into_owned()),
                overwrite: false,
                allow_partial: false,
            }))
            .await
            .unwrap()
            .0;
        assert_eq!(finalized.schema_version, REVIEW_PACKAGE_SCHEMA_VERSION);
        assert!(!finalized.suggestions.is_empty());
        let package: ReviewPackage =
            serde_json::from_slice(&fs::read(output_path).unwrap()).unwrap();
        assert_eq!(package.suggestions.len(), finalized.suggestion_count);
        assert_eq!(package.generated_by, "opencut-mcp");
    }

    #[tokio::test]
    async fn incomplete_and_duplicate_evaluations_are_safe() {
        let directory = tempfile::tempdir().unwrap();
        let transcript_path = write_transcript(directory.path());
        let server = OpenCutServer::new();
        let started = server
            .start_clip_discovery(Parameters(params(&transcript_path)))
            .await
            .unwrap()
            .0;
        let batch = server
            .get_candidate_batch(Parameters(GetCandidateBatchParams {
                session_id: started.session_id.clone(),
                batch_index: 0,
            }))
            .await
            .unwrap()
            .0;
        let candidate_id = batch.candidates[0].candidate_id;
        server
            .submit_candidate_evaluations(Parameters(SubmitCandidateEvaluationsParams {
                session_id: started.session_id.clone(),
                evaluations: vec![evaluation(candidate_id, 0.9)],
            }))
            .await
            .unwrap();
        let duplicate = server
            .submit_candidate_evaluations(Parameters(SubmitCandidateEvaluationsParams {
                session_id: started.session_id.clone(),
                evaluations: vec![evaluation(candidate_id, 0.7)],
            }))
            .await
            .err()
            .expect("duplicate evaluation must fail");
        assert!(duplicate.contains("already has an evaluation"));

        let incomplete = server
            .finalize_clip_discovery(Parameters(FinalizeClipDiscoveryParams {
                session_id: started.session_id,
                weights: None,
                output_path: None,
                overwrite: false,
                allow_partial: false,
            }))
            .await
            .err()
            .expect("incomplete finalization must fail");
        assert!(incomplete.contains("remain unevaluated"));
    }
}
