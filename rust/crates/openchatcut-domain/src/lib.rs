//! OpenChatCut's platform-independent project model and edit reducer.
//!
//! The crate deliberately contains no storage, networking, UI, or ID generation.
//! Callers generate stable IDs and persist idempotency receipts; this crate validates
//! those inputs and applies a transaction atomically using revision compare-and-swap.

#![recursion_limit = "256"]

mod agent;
mod caption;
mod error;
mod export;
mod hash;
mod id;
mod model;
mod motion_graphic;
mod nle_xml;
mod operation;
mod project_check;
mod reducer;
mod story;
mod subtitle;
mod transcript;
mod transcript_analysis;
mod validation;
mod workflow;

pub use agent::{
    AgentAnchorBias, AgentAnchorEdge, AgentAudioOperation, AgentCapabilityCall,
    AgentCapabilityError, AgentExportFormat, AgentGenerationKind, AgentTranscriptionEngine,
    validate_agent_capability_calls,
};
pub use caption::{
    CaptionPresetDescriptor, CaptionWordTimelineRange, active_caption_word_ranges,
    builtin_caption_preset, builtin_caption_presets, caption_timeline_range,
};
pub use error::DomainError;
pub use export::{
    BasicExportPlan, BasicExportSource, ExportFormat, ExportPlanError, ExportRange,
    SceneGraphAudioSource, SceneGraphExportPlan, TimelineAudioExportPlan, build_basic_export_plan,
    build_scene_graph_export_plan, build_timeline_audio_export_plan,
};
pub use hash::{
    DocumentHash, Sha256Digest, TransactionFingerprint, canonical_document_hash,
    transaction_fingerprint,
};
pub use id::{
    ActorId, AssetId, CaptionPresetId, EditPlanId, IdError, IdempotencyKey, ItemId, JobId,
    LinkGroupId, ProjectId, ProviderId, SceneId, SegmentId, SpeakerId, StoryClipId,
    StorySequenceId, TrackId, TransactionId, TranscriptId, WordId,
};
pub use model::{
    Asset, AssetKind, AssetProvenance, Background, Bookmark, CURRENT_SCHEMA_VERSION, CanvasSize,
    Extensions, FrameRate, ItemContent, MediaKind, MotionGraphicElement, ProjectDocument,
    ProjectEnvelope, ProjectSettings, Revision, Scene, SourceRange, TICKS_PER_SECOND, TimelineItem,
    Track, TrackKind,
};
pub use motion_graphic::{
    MotionGraphicLimits, MotionGraphicTemplateDescriptor, MotionGraphicValidationError,
    MotionGraphicValidationReport, builtin_motion_graphic_template,
    builtin_motion_graphic_templates, validate_motion_graphic_dsl,
    validate_motion_graphic_dsl_with_limits,
};
pub use nle_xml::{NleExport, NleFormat, NleXmlError, export_nle_xml};
pub use operation::{Actor, ActorKind, EditTransaction, Operation, TrackPatch};
pub use project_check::{
    DeliveryValidationReport, ProjectIssueSeverity, ProjectValidationIssue,
    validate_project_delivery,
};
pub use reducer::{
    ApplyOutcome, ChangeAction, ChangeKind, ChangeSummary, OperationsOutcome, ValidationReport,
    ValidationWarning, apply_operations, apply_transaction, validate_transaction,
};
pub use story::build_story_materialization_operations;
pub use subtitle::{SubtitleCue, SubtitleError, SubtitleFormat, export_subtitle, parse_subtitle};
pub use transcript::{
    AnchorBias, AnchorEdge, CaptionElement, CaptionStyle, CaptionTextAlign, StoryClip,
    StorySequence, TimelineAnchor, TranscriptDocument, TranscriptSegment, TranscriptSpeaker,
    TranscriptWord,
};
pub use transcript_analysis::{
    TranscriptCleanupAction, TranscriptCleanupAnalysis, TranscriptCleanupOptions,
    TranscriptCleanupSuggestion, TranscriptCleanupSuggestionKind, TranscriptCleanupSummary,
    analyze_transcript_cleanup, build_transcript_cleanup_edit_plan,
};
pub use validation::validate_document;
pub use workflow::{
    AdapterTransport, ApprovalRequirement, CapabilityAdapter, CapabilityKind, CostEstimate,
    DependencyImpact, DependencyImpactKind, EditPlan, GenerationProvenance, JobError, JobPhase,
    JobRecord, JobState, PlanDiff, PlanWarning, ProviderAvailability, ProviderDescriptor,
    ProviderProtocolStep, WarningSeverity, builtin_provider_descriptors,
    operations_are_auto_apply_eligible,
};
