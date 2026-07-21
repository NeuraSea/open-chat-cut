use serde::{Deserialize, Serialize};

use crate::{
    ActorId, Asset, AssetId, CaptionElement, CaptionStyle, IdempotencyKey, ItemId, ProjectDocument,
    ProjectId, ProjectSettings, Revision, Scene, SceneId, SegmentId, SpeakerId, StoryClipId,
    StorySequence, StorySequenceId, TimelineItem, Track, TrackId, TransactionId,
    TranscriptDocument, TranscriptId, WordId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActorKind {
    User,
    Agent,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    pub kind: ActorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ActorId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl Actor {
    pub fn user(id: ActorId) -> Self {
        Self {
            kind: ActorKind::User,
            id: Some(id),
            display_name: None,
        }
    }

    pub fn agent(id: ActorId) -> Self {
        Self {
            kind: ActorKind::Agent,
            id: Some(id),
            display_name: None,
        }
    }

    pub const fn system() -> Self {
        Self {
            kind: ActorKind::System,
            id: None,
            display_name: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditTransaction {
    pub transaction_id: TransactionId,
    pub project_id: ProjectId,
    pub base_revision: Revision,
    pub idempotency_key: IdempotencyKey,
    pub actor: Actor,
    pub operations: Vec<Operation>,
}

impl EditTransaction {
    pub fn new(
        transaction_id: TransactionId,
        project_id: ProjectId,
        base_revision: Revision,
        idempotency_key: IdempotencyKey,
        actor: Actor,
        operations: Vec<Operation>,
    ) -> Self {
        Self {
            transaction_id,
            project_id,
            base_revision,
            idempotency_key,
            actor,
            operations,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

impl TrackPatch {
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.muted.is_none()
            && self.hidden.is_none()
            && self.locked.is_none()
    }
}

/// Semantic edits accepted by both UI shells and agents.
///
/// Every variant carries typed domain data. There is intentionally no JSON-patch
/// operation; Classic migrations use `ReplaceDocument` or `ReplaceSceneGraph` and
/// still receive revision CAS, validation, hashing, and an inverse operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Operation {
    ReplaceDocument {
        document: Box<ProjectDocument>,
    },
    ReplaceSceneGraph {
        scenes: Vec<Scene>,
        current_scene_id: Option<SceneId>,
    },
    SetProjectName {
        name: String,
    },
    SetProjectSettings {
        settings: ProjectSettings,
    },
    AddAsset {
        asset: Asset,
    },
    UpsertAsset {
        asset: Asset,
    },
    RemoveAsset {
        asset_id: AssetId,
    },
    AddScene {
        scene: Scene,
        index: Option<usize>,
    },
    RemoveScene {
        scene_id: SceneId,
    },
    SetSceneName {
        scene_id: SceneId,
        name: String,
    },
    AddTrack {
        scene_id: SceneId,
        track: Track,
        index: Option<usize>,
    },
    RemoveTrack {
        track_id: TrackId,
    },
    SetTrackProperties {
        track_id: TrackId,
        patch: TrackPatch,
    },
    InsertItem {
        track_id: TrackId,
        item: TimelineItem,
        index: Option<usize>,
    },
    RemoveItem {
        item_id: ItemId,
    },
    MoveItem {
        item_id: ItemId,
        target_track_id: TrackId,
        target_index: usize,
        start_ticks: i64,
    },
    ReplaceItem {
        item_id: ItemId,
        item: TimelineItem,
    },
    TrimItem {
        item_id: ItemId,
        start_ticks: i64,
        duration_ticks: i64,
        source_range: Option<crate::SourceRange>,
    },
    SplitItem {
        item_id: ItemId,
        split_at_ticks: i64,
        new_item_id: ItemId,
    },
    SetCaption {
        item_id: ItemId,
        caption: CaptionElement,
    },
    SetCaptionStyle {
        item_id: ItemId,
        style: CaptionStyle,
    },
    UpsertTranscript {
        transcript: TranscriptDocument,
    },
    RemoveTranscript {
        transcript_id: TranscriptId,
    },
    SetTranscriptWordsDeleted {
        transcript_id: TranscriptId,
        word_ids: Vec<WordId>,
        deleted: bool,
    },
    DeleteTranscriptSegment {
        transcript_id: TranscriptId,
        segment_id: SegmentId,
    },
    SetTranscriptDisplayText {
        transcript_id: TranscriptId,
        word_id: WordId,
        display_text: String,
    },
    SetTranscriptSpeaker {
        transcript_id: TranscriptId,
        word_ids: Vec<WordId>,
        speaker_id: Option<SpeakerId>,
    },
    SplitTranscriptSegment {
        transcript_id: TranscriptId,
        segment_id: SegmentId,
        at_word_id: WordId,
        new_segment_id: SegmentId,
    },
    MergeTranscriptSegments {
        transcript_id: TranscriptId,
        first_segment_id: SegmentId,
        second_segment_id: SegmentId,
    },
    ReorderTranscriptSegments {
        transcript_id: TranscriptId,
        segment_ids: Vec<SegmentId>,
    },
    UpsertStorySequence {
        sequence: StorySequence,
    },
    RemoveStorySequence {
        sequence_id: StorySequenceId,
    },
    ReorderStoryClips {
        sequence_id: StorySequenceId,
        clip_ids: Vec<StoryClipId>,
    },
    CloseStoryGaps {
        sequence_id: StorySequenceId,
        threshold_ticks: i64,
        target_gap_ticks: i64,
    },
}
