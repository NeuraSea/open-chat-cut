use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Actor, AssetId, ChangeSummary, DocumentHash, EditPlanId, Extensions, JobId, Operation,
    ProjectId, ProviderId, Revision,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilityKind {
    AgentPlanning,
    VisionAnalysis,
    ImageGeneration,
    VideoGeneration,
    MusicGeneration,
    SpeechSynthesis,
    SoundEffectGeneration,
    AudioCleanup,
    Transcription,
    SpeakerDiarization,
    StockSearch,
    WebCapture,
    PreviewRender,
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderProtocolStep {
    Submit,
    Poll,
    Resume,
    Download,
    Normalize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AdapterTransport {
    Codex,
    LocalProcess {
        executable: String,
        #[serde(default)]
        arguments: Vec<String>,
    },
    Http {
        base_url: String,
    },
    OpenAiCompatible {
        base_url: String,
    },
}

/// Serializable adapter contract used by the daemon to discover capabilities.
/// Runtime crates bind this descriptor to an implementation; the domain crate
/// deliberately does not define an async/networking trait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityAdapter {
    pub id: String,
    pub capability: CapabilityKind,
    pub transport: AdapterTransport,
    /// Ordered lifecycle implemented by the adapter. Remote generation normally
    /// uses submit -> poll/resume -> download -> normalize.
    pub protocol: Vec<ProviderProtocolStep>,
    #[serde(default)]
    pub input_mime_types: Vec<String>,
    #[serde(default)]
    pub output_mime_types: Vec<String>,
    #[serde(default)]
    pub requires_network: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_name: Option<String>,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProviderAvailability {
    Available,
    NeedsConfiguration { missing: Vec<String> },
    Degraded { message: String },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub name: String,
    pub availability: ProviderAvailability,
    #[serde(default)]
    pub adapters: Vec<CapabilityAdapter>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

/// Built-in provider catalog. Runtime availability remains explicit: listing a
/// provider never implies that credentials are configured or that paid work can
/// be submitted without approval.
pub fn builtin_provider_descriptors(media_worker_available: bool) -> Vec<ProviderDescriptor> {
    let remote_protocol = vec![
        ProviderProtocolStep::Submit,
        ProviderProtocolStep::Poll,
        ProviderProtocolStep::Resume,
        ProviderProtocolStep::Download,
        ProviderProtocolStep::Normalize,
    ];
    vec![
        provider(
            "codex-image",
            "Codex image generation",
            ProviderAvailability::Unavailable {
                reason: "Codex app-server handoff is not connected to this daemon instance"
                    .to_owned(),
            },
            CapabilityAdapter {
                id: "codex-image".to_owned(),
                capability: CapabilityKind::ImageGeneration,
                transport: AdapterTransport::Codex,
                protocol: vec![
                    ProviderProtocolStep::Submit,
                    ProviderProtocolStep::Download,
                    ProviderProtocolStep::Normalize,
                ],
                input_mime_types: Vec::new(),
                output_mime_types: vec!["image/png".to_owned()],
                requires_network: true,
                credential_name: None,
                extensions: Extensions::new(),
            },
            Vec::new(),
        ),
        provider(
            "seedance",
            "Volcengine Seedance",
            ProviderAvailability::NeedsConfiguration {
                missing: vec!["seedance.apiKey".to_owned()],
            },
            remote_adapter(
                "seedance",
                CapabilityKind::VideoGeneration,
                "https://ark.cn-beijing.volces.com/api/v3",
                "seedance.apiKey",
                remote_protocol.clone(),
                vec!["video/mp4".to_owned()],
            ),
            vec!["seedance".to_owned()],
        ),
        provider(
            "seedance-compatible",
            "Seedance-compatible endpoint",
            ProviderAvailability::NeedsConfiguration {
                missing: vec![
                    "seedanceCompatible.baseUrl".to_owned(),
                    "seedanceCompatible.apiKey".to_owned(),
                ],
            },
            remote_adapter(
                "seedance-compatible",
                CapabilityKind::VideoGeneration,
                "https://provider.invalid/v1",
                "seedanceCompatible.apiKey",
                remote_protocol.clone(),
                vec!["video/mp4".to_owned(), "video/webm".to_owned()],
            ),
            Vec::new(),
        ),
        provider(
            "fal",
            "fal.ai fallback",
            ProviderAvailability::NeedsConfiguration {
                missing: vec!["fal.apiKey".to_owned()],
            },
            remote_adapter(
                "fal",
                CapabilityKind::VideoGeneration,
                "https://queue.fal.run",
                "fal.apiKey",
                remote_protocol.clone(),
                vec!["video/mp4".to_owned()],
            ),
            Vec::new(),
        ),
        provider(
            "suno",
            "Suno music",
            ProviderAvailability::NeedsConfiguration {
                missing: vec!["suno.baseUrl".to_owned(), "suno.apiKey".to_owned()],
            },
            remote_adapter(
                "suno",
                CapabilityKind::MusicGeneration,
                "https://provider.invalid/v1",
                "suno.apiKey",
                remote_protocol,
                vec!["audio/mpeg".to_owned(), "audio/flac".to_owned()],
            ),
            Vec::new(),
        ),
        provider(
            "new-api-image",
            "New API private image generation",
            ProviderAvailability::NeedsConfiguration {
                missing: vec![
                    "newApiImage.baseUrl".to_owned(),
                    "newApiImage.apiKey|apiKeyEnv|apiKeyKeychain".to_owned(),
                ],
            },
            remote_adapter(
                "new-api-image",
                CapabilityKind::ImageGeneration,
                "https://provider.invalid/v1",
                "newApiImage.apiKey",
                vec![
                    ProviderProtocolStep::Submit,
                    ProviderProtocolStep::Resume,
                    ProviderProtocolStep::Download,
                    ProviderProtocolStep::Normalize,
                ],
                vec!["image/png".to_owned(), "image/jpeg".to_owned()],
            ),
            vec!["occ-image".to_owned()],
        ),
        provider(
            "new-api-video",
            "New API video generation",
            ProviderAvailability::NeedsConfiguration {
                missing: vec![
                    "newApiVideo.baseUrl".to_owned(),
                    "newApiVideo.apiKey|apiKeyEnv|apiKeyKeychain".to_owned(),
                ],
            },
            remote_adapter(
                "new-api-video",
                CapabilityKind::VideoGeneration,
                "https://provider.invalid/v1",
                "newApiVideo.apiKey",
                vec![
                    ProviderProtocolStep::Submit,
                    ProviderProtocolStep::Poll,
                    ProviderProtocolStep::Resume,
                    ProviderProtocolStep::Download,
                    ProviderProtocolStep::Normalize,
                ],
                vec!["video/mp4".to_owned(), "video/webm".to_owned()],
            ),
            Vec::new(),
        ),
        provider(
            "new-api-voice",
            "New API private voice",
            ProviderAvailability::NeedsConfiguration {
                missing: vec![
                    "newApiVoice.baseUrl".to_owned(),
                    "newApiVoice.apiKey|apiKeyEnv|apiKeyKeychain".to_owned(),
                ],
            },
            remote_adapter(
                "new-api-voice",
                CapabilityKind::SpeechSynthesis,
                "https://provider.invalid/v1",
                "newApiVoice.apiKey",
                vec![
                    ProviderProtocolStep::Submit,
                    ProviderProtocolStep::Resume,
                    ProviderProtocolStep::Download,
                    ProviderProtocolStep::Normalize,
                ],
                vec!["audio/wav".to_owned(), "audio/mpeg".to_owned()],
            ),
            vec!["occ-tts".to_owned()],
        ),
        provider(
            "local-web-capture",
            "Isolated website capture",
            if media_worker_available {
                ProviderAvailability::Available
            } else {
                ProviderAvailability::Unavailable {
                    reason: "native media worker with Chromium is not configured".to_owned(),
                }
            },
            CapabilityAdapter {
                id: "local-web-capture".to_owned(),
                capability: CapabilityKind::WebCapture,
                transport: AdapterTransport::LocalProcess {
                    executable: "openchatcut-media-worker".to_owned(),
                    arguments: Vec::new(),
                },
                protocol: vec![
                    ProviderProtocolStep::Submit,
                    ProviderProtocolStep::Download,
                    ProviderProtocolStep::Normalize,
                ],
                input_mime_types: vec!["text/html".to_owned(), "application/xhtml+xml".to_owned()],
                output_mime_types: vec!["image/png".to_owned()],
                requires_network: true,
                credential_name: None,
                extensions: Extensions::new(),
            },
            vec!["chromium-offline-snapshot-v1".to_owned()],
        ),
        provider(
            "local-voice",
            "Local Kokoro/Piper voice",
            if media_worker_available {
                ProviderAvailability::NeedsConfiguration {
                    missing: vec!["localVoice.model".to_owned()],
                }
            } else {
                ProviderAvailability::Unavailable {
                    reason: "native media worker is not configured".to_owned(),
                }
            },
            CapabilityAdapter {
                id: "local-voice".to_owned(),
                capability: CapabilityKind::SpeechSynthesis,
                transport: AdapterTransport::LocalProcess {
                    executable: "openchatcut-media-worker".to_owned(),
                    arguments: Vec::new(),
                },
                protocol: vec![
                    ProviderProtocolStep::Submit,
                    ProviderProtocolStep::Normalize,
                ],
                input_mime_types: Vec::new(),
                output_mime_types: vec!["audio/wav".to_owned()],
                requires_network: false,
                credential_name: None,
                extensions: Extensions::new(),
            },
            vec!["kokoro".to_owned(), "piper".to_owned()],
        ),
        provider(
            "local-audiogen",
            "Local AudioGen SFX",
            ProviderAvailability::Unavailable {
                reason: "optional AudioGen runtime is not installed".to_owned(),
            },
            CapabilityAdapter {
                id: "local-audiogen".to_owned(),
                capability: CapabilityKind::SoundEffectGeneration,
                transport: AdapterTransport::LocalProcess {
                    executable: "openchatcut-media-worker".to_owned(),
                    arguments: Vec::new(),
                },
                protocol: vec![
                    ProviderProtocolStep::Submit,
                    ProviderProtocolStep::Normalize,
                ],
                input_mime_types: Vec::new(),
                output_mime_types: vec!["audio/wav".to_owned()],
                requires_network: false,
                credential_name: None,
                extensions: Extensions::new(),
            },
            vec!["audiogen".to_owned()],
        ),
    ]
}

fn provider(
    id: &str,
    name: &str,
    availability: ProviderAvailability,
    adapter: CapabilityAdapter,
    models: Vec<String>,
) -> ProviderDescriptor {
    ProviderDescriptor {
        id: ProviderId::new(id).expect("built-in provider ID is valid"),
        name: name.to_owned(),
        availability,
        adapters: vec![adapter],
        models,
        documentation_url: None,
        extensions: Extensions::new(),
    }
}

fn remote_adapter(
    id: &str,
    capability: CapabilityKind,
    base_url: &str,
    credential_name: &str,
    protocol: Vec<ProviderProtocolStep>,
    output_mime_types: Vec<String>,
) -> CapabilityAdapter {
    CapabilityAdapter {
        id: id.to_owned(),
        capability,
        transport: AdapterTransport::Http {
            base_url: base_url.to_owned(),
        },
        protocol,
        input_mime_types: Vec::new(),
        output_mime_types,
        requires_network: true,
        credential_name: Some(credential_name.to_owned()),
        extensions: Extensions::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEstimate {
    /// One-millionth units of the named currency; avoids floating point drift.
    pub amount_micros: u64,
    pub currency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default)]
    pub is_estimate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WarningSeverity {
    Info,
    Warning,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanWarning {
    pub code: String,
    pub message: String,
    pub severity: WarningSeverity,
    #[serde(default)]
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyImpactKind {
    Reanchor,
    Regenerate,
    Relink,
    Remove,
    Invalidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyImpact {
    pub entity_id: String,
    pub kind: DependencyImpactKind,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanDiff {
    pub summary: String,
    #[serde(default)]
    pub changes: Vec<ChangeSummary>,
    #[serde(default)]
    pub dependency_impacts: Vec<DependencyImpact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_result_hash: Option<DocumentHash>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ApprovalRequirement {
    None,
    AutoApplyEligible,
    Confirm { reasons: Vec<String> },
}

/// Return whether a batch is safe to auto-apply when the project explicitly
/// opts into that policy. This is deliberately conservative: operations that
/// remove content, change timing destructively, replace a scene graph, or
/// create durable media remain review-only even though the reducer makes them
/// undoable.
pub fn operations_are_auto_apply_eligible(operations: &[Operation]) -> bool {
    !operations.is_empty()
        && operations.iter().all(|operation| {
            matches!(
                operation,
                Operation::SetProjectName { .. }
                    | Operation::SetProjectSettings { .. }
                    | Operation::SetSceneName { .. }
                    | Operation::SetTrackProperties { .. }
                    | Operation::MoveItem { .. }
                    | Operation::SetCaption { .. }
                    | Operation::SetCaptionStyle { .. }
                    | Operation::SetTranscriptDisplayText { .. }
                    | Operation::SetTranscriptSpeaker { .. }
                    | Operation::CloseStoryGaps { .. }
            )
        })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditPlan {
    pub id: EditPlanId,
    pub project_id: ProjectId,
    pub expected_revision: Revision,
    pub objective: String,
    pub actor: Actor,
    pub operations: Vec<Operation>,
    pub diff: PlanDiff,
    #[serde(default)]
    pub warnings: Vec<PlanWarning>,
    #[serde(default)]
    pub estimated_costs: Vec<CostEstimate>,
    pub approval: ApprovalRequirement,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobPhase {
    Submit,
    Poll,
    Resume,
    Download,
    Normalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Queued,
    Running,
    WaitingForProvider,
    Paused,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationProvenance {
    pub provider_id: ProviderId,
    pub model: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,
    #[serde(default)]
    pub input_asset_ids: Vec<AssetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_job_id: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub id: JobId,
    pub project_id: ProjectId,
    pub project_revision: Revision,
    pub capability: CapabilityKind,
    pub provider_id: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub state: JobState,
    pub phase: JobPhase,
    #[serde(default)]
    pub attempt: u32,
    /// Integer basis points in the inclusive range 0..=10_000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_bps: Option<u16>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_job_id: Option<String>,
    #[serde(default)]
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(default)]
    pub output_asset_ids: Vec<AssetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JobError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<GenerationProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostEstimate>,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{Actor, ActorId, ProjectId};

    use super::*;

    #[test]
    fn provider_and_job_contracts_round_trip_through_json() {
        let provider = ProviderDescriptor {
            id: ProviderId::new("seedance").unwrap(),
            name: "Seedance".into(),
            availability: ProviderAvailability::NeedsConfiguration {
                missing: vec!["SEEDANCE_API_KEY".into()],
            },
            adapters: vec![CapabilityAdapter {
                id: "seedance-video-v1".into(),
                capability: CapabilityKind::VideoGeneration,
                transport: AdapterTransport::Http {
                    base_url: "https://example.invalid/v1".into(),
                },
                protocol: vec![
                    ProviderProtocolStep::Submit,
                    ProviderProtocolStep::Poll,
                    ProviderProtocolStep::Resume,
                    ProviderProtocolStep::Download,
                    ProviderProtocolStep::Normalize,
                ],
                input_mime_types: vec!["image/png".into()],
                output_mime_types: vec!["video/mp4".into()],
                requires_network: true,
                credential_name: Some("SEEDANCE_API_KEY".into()),
                extensions: Extensions::new(),
            }],
            models: vec!["seedance-1".into()],
            documentation_url: None,
            extensions: Extensions::new(),
        };
        let encoded = serde_json::to_string(&provider).unwrap();
        assert_eq!(
            serde_json::from_str::<ProviderDescriptor>(&encoded).unwrap(),
            provider
        );

        let job = JobRecord {
            id: JobId::new("job-1").unwrap(),
            project_id: ProjectId::new("project-1").unwrap(),
            project_revision: 7,
            capability: CapabilityKind::VideoGeneration,
            provider_id: ProviderId::new("seedance").unwrap(),
            model: Some("seedance-1".into()),
            state: JobState::Running,
            phase: JobPhase::Poll,
            attempt: 2,
            progress_bps: Some(5_000),
            created_at_ms: 100,
            updated_at_ms: 200,
            external_job_id: Some("remote-1".into()),
            input: json!({"prompt": "waves"}),
            output: None,
            output_asset_ids: Vec::new(),
            error: None,
            provenance: None,
            cost: Some(CostEstimate {
                amount_micros: 1_500_000,
                currency: "USD".into(),
                unit: Some("request".into()),
                is_estimate: true,
            }),
            extensions: Extensions::new(),
        };
        let encoded = serde_json::to_string(&job).unwrap();
        assert_eq!(serde_json::from_str::<JobRecord>(&encoded).unwrap(), job);
    }

    #[test]
    fn edit_plan_round_trips_diff_cost_and_warnings() {
        let plan = EditPlan {
            id: EditPlanId::new("plan-1").unwrap(),
            project_id: ProjectId::new("project-1").unwrap(),
            expected_revision: 3,
            objective: "Remove filler words".into(),
            actor: Actor::agent(ActorId::new("codex").unwrap()),
            operations: vec![Operation::SetProjectName {
                name: "Edited".into(),
            }],
            diff: PlanDiff {
                summary: "Rename the project".into(),
                changes: Vec::new(),
                dependency_impacts: vec![DependencyImpact {
                    entity_id: "caption-1".into(),
                    kind: DependencyImpactKind::Reanchor,
                    reason: "Transcript edit".into(),
                }],
                expected_result_hash: None,
            },
            warnings: vec![PlanWarning {
                code: "semanticDeletion".into(),
                message: "Speech will be removed".into(),
                severity: WarningSeverity::Destructive,
                requires_confirmation: true,
            }],
            estimated_costs: vec![CostEstimate {
                amount_micros: 0,
                currency: "USD".into(),
                unit: None,
                is_estimate: true,
            }],
            approval: ApprovalRequirement::Confirm {
                reasons: vec!["semantic deletion".into()],
            },
            extensions: Extensions::new(),
        };
        let encoded = serde_json::to_string(&plan).unwrap();
        assert_eq!(serde_json::from_str::<EditPlan>(&encoded).unwrap(), plan);
    }

    #[test]
    fn auto_apply_policy_only_allows_reversible_mechanical_operations() {
        assert!(operations_are_auto_apply_eligible(&[
            Operation::SetTranscriptDisplayText {
                transcript_id: crate::TranscriptId::new("transcript-1").unwrap(),
                word_id: crate::WordId::new("word-1").unwrap(),
                display_text: "corrected".into(),
            },
            Operation::SetCaptionStyle {
                item_id: crate::ItemId::new("caption-1").unwrap(),
                style: crate::CaptionStyle::default(),
            },
        ]));
        assert!(!operations_are_auto_apply_eligible(&[
            Operation::SetTranscriptWordsDeleted {
                transcript_id: crate::TranscriptId::new("transcript-1").unwrap(),
                word_ids: vec![crate::WordId::new("word-1").unwrap()],
                deleted: true,
            },
        ]));
        assert!(!operations_are_auto_apply_eligible(&[]));
    }

    #[test]
    fn builtin_generators_never_claim_unconfigured_paid_providers_are_available() {
        let providers = builtin_provider_descriptors(true);
        assert!(
            providers
                .iter()
                .any(|provider| provider.id.as_str() == "seedance")
        );
        for provider in providers.iter().filter(|provider| {
            matches!(
                provider.id.as_str(),
                "seedance" | "seedance-compatible" | "fal" | "suno"
            )
        }) {
            assert!(matches!(
                provider.availability,
                ProviderAvailability::NeedsConfiguration { .. }
            ));
            assert!(
                provider
                    .adapters
                    .iter()
                    .all(|adapter| adapter.requires_network)
            );
        }
    }
}
