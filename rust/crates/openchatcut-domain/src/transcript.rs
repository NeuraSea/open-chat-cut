use serde::{Deserialize, Serialize};

use crate::{
    AssetId, CaptionPresetId, Extensions, LinkGroupId, SegmentId, SpeakerId, StoryClipId,
    StorySequenceId, TranscriptId, WordId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorEdge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorBias {
    Before,
    After,
    Nearest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineAnchor {
    pub transcript_id: TranscriptId,
    pub word_id: WordId,
    pub edge: AnchorEdge,
    #[serde(default = "default_anchor_bias")]
    pub bias: AnchorBias,
    /// Allows a proposal to remain placeable if the referenced word was removed.
    pub fallback_ticks: i64,
}

const fn default_anchor_bias() -> AnchorBias {
    AnchorBias::Nearest
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSpeaker {
    pub id: SpeakerId,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptWord {
    pub id: WordId,
    /// The recognizer output is source truth and edit operations never mutate it.
    pub spoken_text: String,
    /// Correctable text used by captions, scripts, and exports.
    pub display_text: String,
    pub start_ticks: i64,
    pub end_ticks: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<SpeakerId>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub id: SegmentId,
    pub word_ids: Vec<WordId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<SpeakerId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptDocument {
    pub id: TranscriptId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<AssetId>,
    pub language: String,
    #[serde(default)]
    pub speakers: Vec<TranscriptSpeaker>,
    #[serde(default)]
    pub words: Vec<TranscriptWord>,
    #[serde(default)]
    pub segments: Vec<TranscriptSegment>,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

impl TranscriptDocument {
    pub fn new(id: TranscriptId, language: impl Into<String>) -> Self {
        Self {
            id,
            asset_id: None,
            language: language.into(),
            speakers: Vec::new(),
            words: Vec::new(),
            segments: Vec::new(),
            extensions: Extensions::new(),
        }
    }

    pub fn word(&self, word_id: &WordId) -> Option<&TranscriptWord> {
        self.words.iter().find(|word| &word.id == word_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryClip {
    pub id: StoryClipId,
    pub word_ids: Vec<WordId>,
    pub timeline_start_ticks: i64,
    pub source_start_ticks: i64,
    pub source_end_ticks: i64,
    pub link_group_id: LinkGroupId,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

impl StoryClip {
    pub fn duration_ticks(&self) -> Option<i64> {
        self.source_end_ticks.checked_sub(self.source_start_ticks)
    }

    pub fn timeline_end_ticks(&self) -> Option<i64> {
        self.timeline_start_ticks
            .checked_add(self.duration_ticks()?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorySequence {
    pub id: StorySequenceId,
    pub transcript_id: TranscriptId,
    #[serde(default)]
    pub clips: Vec<StoryClip>,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptionTextAlign {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionStyle {
    pub font_family: String,
    pub font_size: f32,
    pub text_color: String,
    pub active_word_color: String,
    pub background_color: String,
    pub outline_color: String,
    pub outline_width: f32,
    pub position_x: f32,
    pub position_y: f32,
    pub max_width: f32,
    pub line_height: f32,
    pub text_align: CaptionTextAlign,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self {
            font_family: "Inter".into(),
            font_size: 64.0,
            text_color: "#ffffff".into(),
            active_word_color: "#ffd60a".into(),
            background_color: "#00000000".into(),
            outline_color: "#000000".into(),
            outline_width: 4.0,
            position_x: 0.5,
            position_y: 0.85,
            max_width: 0.85,
            line_height: 1.15,
            text_align: CaptionTextAlign::Center,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionElement {
    pub transcript_id: TranscriptId,
    pub word_ids: Vec<WordId>,
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_of_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<SpeakerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<CaptionPresetId>,
    #[serde(default)]
    pub style: CaptionStyle,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}
