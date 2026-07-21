use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AssetId, CaptionElement, DocumentHash, DomainError, ItemId, LinkGroupId, ProjectId, SceneId,
    StorySequence, TimelineAnchor, TrackId, TranscriptDocument, canonical_document_hash,
    validate_document,
};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const TICKS_PER_SECOND: i64 = 120_000;
pub type Revision = u64;
pub type Extensions = BTreeMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameRate {
    pub numerator: u32,
    pub denominator: u32,
}

impl FrameRate {
    pub const FPS_30: Self = Self {
        numerator: 30,
        denominator: 1,
    };
}

impl Default for FrameRate {
    fn default() -> Self {
        Self::FPS_30
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasSize {
    pub width: u32,
    pub height: u32,
}

impl Default for CanvasSize {
    fn default() -> Self {
        Self {
            width: 1_920,
            height: 1_080,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Background {
    Color { color: String },
    Blur { blur_intensity: f32 },
}

impl Default for Background {
    fn default() -> Self {
        Self::Color {
            color: "#000000".into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    #[serde(default)]
    pub fps: FrameRate,
    #[serde(default)]
    pub canvas_size: CanvasSize,
    #[serde(default)]
    pub background: Background,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub time_ticks: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ticks: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub id: SceneId,
    pub name: String,
    #[serde(default)]
    pub is_main: bool,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    /// Unknown Classic scene fields are flattened here for lossless migration round-trips.
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

impl Scene {
    pub fn new(id: SceneId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            is_main: false,
            tracks: Vec::new(),
            bookmarks: Vec::new(),
            extensions: Extensions::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    Video,
    Audio,
    Text,
    Caption,
    Graphic,
    Effect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub kind: TrackKind,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub items: Vec<TimelineItem>,
    /// Unknown Classic track fields are flattened here for lossless migration round-trips.
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

impl Track {
    pub fn new(id: TrackId, name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            muted: false,
            hidden: false,
            locked: false,
            items: Vec::new(),
            extensions: Extensions::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRange {
    pub in_ticks: i64,
    pub out_ticks: i64,
}

impl SourceRange {
    pub fn duration_ticks(self) -> Option<i64> {
        self.out_ticks.checked_sub(self.in_ticks)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Video,
    Image,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionGraphicElement {
    pub dsl_version: u32,
    pub definition: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ItemContent {
    Media {
        asset_id: AssetId,
        media_kind: MediaKind,
    },
    Text {
        #[serde(default)]
        text: String,
    },
    Caption {
        caption: Box<CaptionElement>,
    },
    MotionGraphic {
        motion_graphic: MotionGraphicElement,
    },
    Sticker {
        sticker_id: String,
    },
    Effect {
        effect_type: String,
    },
    Custom {
        custom_type: String,
        data: Value,
    },
}

impl ItemContent {
    pub fn asset_id(&self) -> Option<&AssetId> {
        match self {
            Self::Media { asset_id, .. } => Some(asset_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItem {
    pub id: ItemId,
    pub name: String,
    pub start_ticks: i64,
    pub duration_ticks: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<SourceRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_duration_ticks: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_group_id: Option<LinkGroupId>,
    /// Stable transcript-word placement used by B-roll and other semantic
    /// overlays. Transcript edits remap the item's timeline start through the
    /// same StorySequence mapping as captions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_anchor: Option<TimelineAnchor>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub content: ItemContent,
    /// Transform, style, params, keyframes, effects, masks, and other Classic fields
    /// that are not yet typed are flattened here and survive a JSON round-trip.
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

const fn default_true() -> bool {
    true
}

impl TimelineItem {
    pub fn new(
        id: ItemId,
        name: impl Into<String>,
        start_ticks: i64,
        duration_ticks: i64,
        content: ItemContent,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            start_ticks,
            duration_ticks,
            source_range: None,
            source_duration_ticks: None,
            link_group_id: None,
            timeline_anchor: None,
            enabled: true,
            content,
            extensions: Extensions::new(),
        }
    }

    pub fn end_ticks(&self) -> Option<i64> {
        self.start_ticks.checked_add(self.duration_ticks)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetKind {
    Video,
    Image,
    Audio,
    Font,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AssetProvenance {
    Imported {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_name: Option<String>,
    },
    Generated {
        provider: String,
        model: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<String>,
    },
    Derived {
        parent_asset_id: AssetId,
        operation: String,
    },
}

impl Default for AssetProvenance {
    fn default() -> Self {
        Self::Imported { source_name: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: AssetId,
    pub name: String,
    pub kind: AssetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<crate::Sha256Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ticks: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default)]
    pub has_audio: bool,
    #[serde(default)]
    pub provenance: AssetProvenance,
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

impl Asset {
    pub fn new(id: AssetId, name: impl Into<String>, kind: AssetKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            content_hash: None,
            duration_ticks: None,
            width: None,
            height: None,
            has_audio: false,
            provenance: AssetProvenance::default(),
            extensions: Extensions::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocument {
    pub schema_version: u32,
    pub id: ProjectId,
    pub name: String,
    #[serde(default)]
    pub settings: ProjectSettings,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_scene_id: Option<SceneId>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub transcripts: Vec<TranscriptDocument>,
    #[serde(default)]
    pub story_sequences: Vec<StorySequence>,
    /// Unknown project-level Classic fields (including view state and migration
    /// metadata) are flattened here rather than discarded.
    #[serde(default, flatten)]
    pub extensions: Extensions,
}

impl ProjectDocument {
    pub fn new(id: ProjectId, name: impl Into<String>) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id,
            name: name.into(),
            settings: ProjectSettings::default(),
            scenes: Vec::new(),
            current_scene_id: None,
            assets: Vec::new(),
            transcripts: Vec::new(),
            story_sequences: Vec::new(),
            extensions: Extensions::new(),
        }
    }

    pub fn find_track(&self, track_id: &TrackId) -> Option<(&Scene, &Track)> {
        self.scenes.iter().find_map(|scene| {
            scene
                .tracks
                .iter()
                .find(|track| &track.id == track_id)
                .map(|track| (scene, track))
        })
    }

    pub fn find_item(&self, item_id: &ItemId) -> Option<(&Scene, &Track, &TimelineItem)> {
        self.scenes.iter().find_map(|scene| {
            scene.tracks.iter().find_map(|track| {
                track
                    .items
                    .iter()
                    .find(|item| &item.id == item_id)
                    .map(|item| (scene, track, item))
            })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEnvelope {
    pub document: ProjectDocument,
    pub revision: Revision,
    pub document_hash: DocumentHash,
}

impl ProjectEnvelope {
    pub fn new(document: ProjectDocument) -> Result<Self, DomainError> {
        validate_document(&document)?;
        let document_hash = canonical_document_hash(&document)?;
        Ok(Self {
            document,
            revision: 0,
            document_hash,
        })
    }

    pub fn verify(&self) -> Result<(), DomainError> {
        validate_document(&self.document)?;
        let actual = canonical_document_hash(&self.document)?;
        if actual != self.document_hash {
            return Err(DomainError::EnvelopeHashMismatch {
                expected_hash: self.document_hash.to_string(),
                actual_hash: actual.to_string(),
            });
        }
        Ok(())
    }
}
