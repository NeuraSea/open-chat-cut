use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A bounded, declarative capability request emitted by an Agent planner.
///
/// These values are plans, not executable tool calls. Runtime code must bind
/// the project, revision, confirmation and idempotency key itself so untrusted
/// model output can never select another project or bypass approval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AgentCapabilityCall {
    SearchBroll {
        query: String,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        transcript_id: Option<String>,
        #[serde(default)]
        word_id: Option<String>,
        #[serde(default)]
        edge: Option<AgentAnchorEdge>,
        #[serde(default)]
        bias: Option<AgentAnchorBias>,
    },
    StartTranscription {
        asset_id: String,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        diarization: bool,
        #[serde(default)]
        min_speakers: Option<u32>,
        #[serde(default)]
        max_speakers: Option<u32>,
        #[serde(default)]
        engine: Option<AgentTranscriptionEngine>,
    },
    GenerateAsset {
        kind: AgentGenerationKind,
        provider: String,
        #[serde(default)]
        model: Option<String>,
        prompt: String,
        #[serde(default)]
        options: Value,
    },
    ProcessAudio {
        asset_id: String,
        operation: AgentAudioOperation,
        #[serde(default)]
        options: Value,
    },
    StartExport {
        format: AgentExportFormat,
        output_path: String,
        #[serde(default)]
        allow_overwrite: bool,
        #[serde(default)]
        settings: Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAnchorEdge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAnchorBias {
    Before,
    After,
    Nearest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentTranscriptionEngine {
    Auto,
    FasterWhisper,
    NewApiAsr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentGenerationKind {
    Image,
    Video,
    Voice,
    Music,
    Sfx,
    WebCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentAudioOperation {
    Denoise,
    Normalize,
    CompressDialogue,
    DuckMusic,
    Loop,
    Crossfade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentExportFormat {
    Mp4,
    Webm,
    Wav,
    Mp3,
    Srt,
    Vtt,
    Ass,
    Txt,
    Png,
    PngSequence,
    #[serde(rename = "prores-4444")]
    Prores4444,
    PremiereXml,
    ResolveXml,
    ProjectPackage,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentCapabilityError {
    #[error("Agent capability plan contains more than {maximum} calls")]
    TooManyCalls { maximum: usize },
    #[error("Agent capability call {index} is invalid: {message}")]
    InvalidCall { index: usize, message: String },
}

impl AgentCapabilityCall {
    pub fn tool_name(&self) -> &'static str {
        match self {
            Self::SearchBroll { .. } => "search_broll",
            Self::StartTranscription { .. } => "start_transcription",
            Self::GenerateAsset { .. } => "generate_asset",
            Self::ProcessAudio { .. } => "process_audio",
            Self::StartExport { .. } => "start_export",
        }
    }

    pub fn requires_approval(&self) -> bool {
        !matches!(self, Self::SearchBroll { .. })
    }

    pub fn sends_external_data(&self) -> bool {
        match self {
            Self::StartTranscription {
                engine:
                    None
                    | Some(AgentTranscriptionEngine::Auto)
                    | Some(AgentTranscriptionEngine::NewApiAsr),
                ..
            } => true,
            Self::GenerateAsset { provider, .. } => {
                !matches!(provider.as_str(), "local-voice" | "local-audiogen")
            }
            _ => false,
        }
    }

    pub fn may_charge_provider(&self) -> bool {
        matches!(
            self,
            Self::GenerateAsset {
                provider,
                ..
            } if !matches!(
                provider.as_str(),
                "codex-image"
                    | "local-voice"
                    | "local-audiogen"
                    | "local-web-capture"
                    | "new-api-image"
                    | "new-api-voice"
            )
        )
    }

    pub fn summary(&self) -> String {
        match self {
            Self::SearchBroll { query, .. } => format!("Search local B-roll for {query:?}"),
            Self::StartTranscription { asset_id, .. } => {
                format!("Transcribe managed asset {asset_id}")
            }
            Self::GenerateAsset { kind, provider, .. } => {
                format!("Generate {kind:?} with {provider}")
            }
            Self::ProcessAudio {
                asset_id,
                operation,
                ..
            } => format!("Run {operation:?} on {asset_id}"),
            Self::StartExport {
                format,
                output_path,
                ..
            } => format!("Export {format:?} to {output_path}"),
        }
    }
}

pub fn validate_agent_capability_calls(
    calls: &[AgentCapabilityCall],
) -> Result<(), AgentCapabilityError> {
    const MAX_CALLS: usize = 16;
    if calls.len() > MAX_CALLS {
        return Err(AgentCapabilityError::TooManyCalls { maximum: MAX_CALLS });
    }
    for (index, call) in calls.iter().enumerate() {
        let invalid = |message: &str| AgentCapabilityError::InvalidCall {
            index,
            message: message.to_owned(),
        };
        match call {
            AgentCapabilityCall::SearchBroll {
                query,
                limit,
                transcript_id,
                word_id,
                ..
            } => {
                validate_text(query, 1, 500).map_err(|message| invalid(message))?;
                if limit.is_some_and(|value| !(1..=50).contains(&value)) {
                    return Err(invalid("limit must be between 1 and 50"));
                }
                if transcript_id.is_some() != word_id.is_some() {
                    return Err(invalid("transcriptId and wordId must be provided together"));
                }
                for value in [transcript_id, word_id].into_iter().flatten() {
                    validate_text(value, 1, 256).map_err(|message| invalid(message))?;
                }
            }
            AgentCapabilityCall::StartTranscription {
                asset_id,
                language,
                min_speakers,
                max_speakers,
                ..
            } => {
                validate_text(asset_id, 1, 256).map_err(|message| invalid(message))?;
                if let Some(language) = language {
                    validate_text(language, 1, 64).map_err(|message| invalid(message))?;
                }
                if min_speakers.is_some_and(|value| !(1..=32).contains(&value))
                    || max_speakers.is_some_and(|value| !(1..=32).contains(&value))
                    || min_speakers
                        .zip(*max_speakers)
                        .is_some_and(|(minimum, maximum)| minimum > maximum)
                {
                    return Err(invalid("speaker bounds must be between 1 and 32"));
                }
            }
            AgentCapabilityCall::GenerateAsset {
                provider,
                model,
                prompt,
                options,
                ..
            } => {
                validate_text(provider, 1, 200).map_err(|message| invalid(message))?;
                if let Some(model) = model {
                    validate_text(model, 1, 200).map_err(|message| invalid(message))?;
                }
                validate_text(prompt, 1, 20_000).map_err(|message| invalid(message))?;
                validate_options(options).map_err(|message| invalid(message))?;
            }
            AgentCapabilityCall::ProcessAudio {
                asset_id, options, ..
            } => {
                validate_text(asset_id, 1, 256).map_err(|message| invalid(message))?;
                validate_options(options).map_err(|message| invalid(message))?;
            }
            AgentCapabilityCall::StartExport {
                output_path,
                settings,
                ..
            } => {
                validate_text(output_path, 1, 240).map_err(|message| invalid(message))?;
                if output_path.contains(['/', '\\']) || matches!(output_path.as_str(), "." | "..") {
                    return Err(invalid("outputPath must be a portable file name"));
                }
                validate_options(settings).map_err(|message| invalid(message))?;
            }
        }
    }
    Ok(())
}

fn validate_text(value: &str, minimum: usize, maximum: usize) -> Result<(), &'static str> {
    let length = value.len();
    if length < minimum || length > maximum || value.chars().any(char::is_control) {
        return Err("text length or characters are outside the allowed bounds");
    }
    Ok(())
}

fn validate_options(value: &Value) -> Result<(), &'static str> {
    if value.is_null() {
        return Ok(());
    }
    if !value.is_object() {
        return Err("options must be an object");
    }
    if serde_json::to_vec(value)
        .map(|encoded| encoded.len() > 64 * 1024)
        .unwrap_or(true)
    {
        return Err("options exceed the 64 KiB limit");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rejects_unbound_or_unsafe_capability_arguments() {
        let calls = serde_json::from_value::<Vec<AgentCapabilityCall>>(json!([{
            "type": "startExport",
            "format": "mp4",
            "outputPath": "../outside.mp4"
        }]))
        .unwrap();
        assert!(validate_agent_capability_calls(&calls).is_err());

        assert!(
            serde_json::from_value::<AgentCapabilityCall>(json!({
                "type": "generateAsset",
                "projectId": "another-project",
                "kind": "image",
                "provider": "codex-image",
                "prompt": "A clean title image"
            }))
            .is_err()
        );
    }

    #[test]
    fn classifies_read_only_and_paid_calls() {
        let search = AgentCapabilityCall::SearchBroll {
            query: "city".to_owned(),
            limit: None,
            transcript_id: None,
            word_id: None,
            edge: None,
            bias: None,
        };
        assert!(!search.requires_approval());

        let generation = AgentCapabilityCall::GenerateAsset {
            kind: AgentGenerationKind::Image,
            provider: "codex-image".to_owned(),
            model: Some("gpt-image-2".to_owned()),
            prompt: "A city skyline".to_owned(),
            options: json!({}),
        };
        assert!(generation.requires_approval());
        assert!(generation.sends_external_data());
        assert!(!generation.may_charge_provider());
        validate_agent_capability_calls(&[generation]).unwrap();

        let paid_generation = AgentCapabilityCall::GenerateAsset {
            kind: AgentGenerationKind::Video,
            provider: "seedance".to_owned(),
            model: None,
            prompt: "A city flyover".to_owned(),
            options: json!({}),
        };
        assert!(paid_generation.may_charge_provider());
    }

    #[test]
    fn professional_export_names_match_the_public_tool_protocol() {
        let call = AgentCapabilityCall::StartExport {
            format: AgentExportFormat::Prores4444,
            output_path: "graphic.mov".to_owned(),
            allow_overwrite: false,
            settings: json!({}),
        };
        assert_eq!(serde_json::to_value(call).unwrap()["format"], "prores-4444");
    }
}
