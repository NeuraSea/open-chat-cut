use std::collections::BTreeMap;

use openchatcut_domain::{
    Actor, ActorId, AnchorBias, AnchorEdge, Asset, AssetId, AssetKind, CaptionElement,
    CaptionStyle, DomainError, EditTransaction, Extensions, IdempotencyKey, ItemContent, ItemId,
    LinkGroupId, MediaKind, Operation, ProjectDocument, ProjectEnvelope, ProjectId, Scene, SceneId,
    SegmentId, SpeakerId, StoryClip, StoryClipId, StorySequence, StorySequenceId, TimelineAnchor,
    TimelineItem, Track, TrackId, TrackKind, TransactionId, TranscriptDocument, TranscriptId,
    TranscriptSegment, TranscriptSpeaker, TranscriptWord, WordId, apply_transaction,
    build_story_materialization_operations, transaction_fingerprint, validate_transaction,
};
use serde_json::json;

fn project_id() -> ProjectId {
    ProjectId::new("project-1").unwrap()
}

fn transaction(revision: u64, suffix: &str, operations: Vec<Operation>) -> EditTransaction {
    EditTransaction::new(
        TransactionId::new(format!("tx-{suffix}")).unwrap(),
        project_id(),
        revision,
        IdempotencyKey::new(format!("request-{suffix}")).unwrap(),
        Actor::agent(ActorId::new("codex").unwrap()),
        operations,
    )
}

fn word(id: &str, text: &str, start_ticks: i64, end_ticks: i64) -> TranscriptWord {
    TranscriptWord {
        id: WordId::new(id).unwrap(),
        spoken_text: text.into(),
        display_text: text.into(),
        start_ticks,
        end_ticks,
        speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
        deleted: false,
        confidence: Some(0.9),
        extensions: Extensions::new(),
    }
}

fn populated_document() -> ProjectDocument {
    let asset_id = AssetId::new("asset-1").unwrap();
    let mut asset = Asset::new(asset_id.clone(), "Interview.wav", AssetKind::Audio);
    asset.duration_ticks = Some(360_000);

    let words = vec![
        word("word-1", "um", 0, 60_000),
        word("word-2", "hello", 70_000, 150_000),
        word("word-3", "world", 160_000, 240_000),
    ];
    let transcript = TranscriptDocument {
        id: TranscriptId::new("transcript-1").unwrap(),
        asset_id: Some(asset_id.clone()),
        language: "en".into(),
        speakers: vec![TranscriptSpeaker {
            id: SpeakerId::new("speaker-1").unwrap(),
            label: "Speaker 1".into(),
            color: None,
        }],
        words: words.clone(),
        segments: vec![TranscriptSegment {
            id: SegmentId::new("segment-1").unwrap(),
            word_ids: words.iter().map(|word| word.id.clone()).collect(),
            speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
        }],
        extensions: Extensions::new(),
    };

    let mut audio_item = TimelineItem::new(
        ItemId::new("audio-item-1").unwrap(),
        "Interview",
        0,
        240_000,
        ItemContent::Media {
            asset_id,
            media_kind: MediaKind::Audio,
        },
    );
    audio_item.link_group_id = Some(LinkGroupId::new("link-1").unwrap());
    let mut audio_track = Track::new(
        TrackId::new("audio-track-1").unwrap(),
        "Dialogue",
        TrackKind::Audio,
    );
    audio_track.items.push(audio_item);

    let caption = CaptionElement {
        transcript_id: transcript.id.clone(),
        word_ids: words.iter().map(|word| word.id.clone()).collect(),
        language: "en".into(),
        translation_of_language: None,
        speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
        preset_id: None,
        style: CaptionStyle::default(),
        extensions: Extensions::new(),
    };
    let caption_item = TimelineItem::new(
        ItemId::new("caption-item-1").unwrap(),
        "Captions",
        0,
        240_000,
        ItemContent::Caption {
            caption: Box::new(caption),
        },
    );
    let mut caption_track = Track::new(
        TrackId::new("caption-track-1").unwrap(),
        "Captions",
        TrackKind::Caption,
    );
    caption_track.items.push(caption_item);

    let scene_id = SceneId::new("scene-1").unwrap();
    let mut scene = Scene::new(scene_id.clone(), "Main");
    scene.is_main = true;
    scene.tracks = vec![audio_track, caption_track];

    let sequence = StorySequence {
        id: StorySequenceId::new("story-1").unwrap(),
        transcript_id: transcript.id.clone(),
        clips: vec![StoryClip {
            id: StoryClipId::new("clip-1").unwrap(),
            word_ids: words.iter().map(|word| word.id.clone()).collect(),
            timeline_start_ticks: 0,
            source_start_ticks: 0,
            source_end_ticks: 240_000,
            link_group_id: LinkGroupId::new("link-1").unwrap(),
            extensions: Extensions::new(),
        }],
        extensions: Extensions::new(),
    };

    let mut document = ProjectDocument::new(project_id(), "Interview");
    document.assets.push(asset);
    document.transcripts.push(transcript);
    document.story_sequences.push(sequence);
    document.scenes.push(scene);
    document.current_scene_id = Some(scene_id);
    document
}

#[test]
fn managed_asset_metadata_can_be_upserted_without_breaking_stable_references() {
    let document = populated_document();
    let envelope = ProjectEnvelope::new(document.clone()).unwrap();
    let mut managed = document.assets[0].clone();
    managed.name = "Interview managed.wav".into();
    managed.duration_ticks = Some(480_000);
    managed
        .extensions
        .insert("managedMedia".into(), json!({ "byteSize": 12 }));
    let applied = apply_transaction(
        &envelope,
        &transaction(
            0,
            "upsert-asset",
            vec![Operation::UpsertAsset {
                asset: managed.clone(),
            }],
        ),
    )
    .unwrap();
    assert_eq!(applied.envelope.document.assets[0], managed);
    assert_eq!(
        applied.envelope.document.scenes[0].tracks[0].items[0]
            .content
            .asset_id(),
        Some(&managed.id)
    );
}

#[test]
fn audio_lane_can_reference_an_audible_video_asset() {
    let asset_id = AssetId::new("camera-source").unwrap();
    let mut asset = Asset::new(asset_id.clone(), "Camera.mp4", AssetKind::Video);
    asset.has_audio = true;
    asset.duration_ticks = Some(240_000);

    let item = TimelineItem::new(
        ItemId::new("camera-audio").unwrap(),
        "Camera audio",
        0,
        240_000,
        ItemContent::Media {
            asset_id,
            media_kind: MediaKind::Audio,
        },
    );
    let mut track = Track::new(
        TrackId::new("camera-audio-track").unwrap(),
        "Camera audio",
        TrackKind::Audio,
    );
    track.items.push(item);
    let scene_id = SceneId::new("camera-scene").unwrap();
    let mut scene = Scene::new(scene_id.clone(), "Camera");
    scene.is_main = true;
    scene.tracks.push(track);

    let mut document = ProjectDocument::new(project_id(), "Camera edit");
    document.assets.push(asset);
    document.scenes.push(scene);
    document.current_scene_id = Some(scene_id);

    ProjectEnvelope::new(document).expect("embedded video audio should validate");
}

#[test]
fn audio_lane_rejects_a_silent_video_asset() {
    let asset_id = AssetId::new("silent-source").unwrap();
    let mut asset = Asset::new(asset_id.clone(), "Silent.mp4", AssetKind::Video);
    asset.duration_ticks = Some(240_000);
    let item = TimelineItem::new(
        ItemId::new("silent-audio").unwrap(),
        "Impossible audio",
        0,
        240_000,
        ItemContent::Media {
            asset_id,
            media_kind: MediaKind::Audio,
        },
    );
    let mut track = Track::new(
        TrackId::new("silent-audio-track").unwrap(),
        "Impossible audio",
        TrackKind::Audio,
    );
    track.items.push(item);
    let scene_id = SceneId::new("silent-scene").unwrap();
    let mut scene = Scene::new(scene_id.clone(), "Silent");
    scene.is_main = true;
    scene.tracks.push(track);

    let mut document = ProjectDocument::new(project_id(), "Silent edit");
    document.assets.push(asset);
    document.scenes.push(scene);
    document.current_scene_id = Some(scene_id);

    let error = ProjectEnvelope::new(document).expect_err("silent video must not supply audio");
    assert!(error.to_string().contains("mediaKind does not match"));
}

#[test]
fn validates_and_applies_a_revisioned_atomic_transaction() {
    let document = populated_document();
    let envelope = ProjectEnvelope::new(document.clone()).unwrap();
    let track = Track::new(
        TrackId::new("graphics-track-1").unwrap(),
        "Graphics",
        TrackKind::Graphic,
    );
    let edit = transaction(
        0,
        "apply",
        vec![
            Operation::SetProjectName {
                name: "Published interview".into(),
            },
            Operation::AddTrack {
                scene_id: SceneId::new("scene-1").unwrap(),
                track,
                index: None,
            },
        ],
    );

    let report = validate_transaction(&envelope, &edit).unwrap();
    let applied = apply_transaction(&envelope, &edit).unwrap();

    assert_eq!(report.next_revision, 1);
    assert_eq!(
        report.resulting_document_hash,
        applied.envelope.document_hash
    );
    assert_eq!(applied.envelope.revision, 1);
    assert_eq!(applied.envelope.document.name, "Published interview");
    assert_eq!(applied.changes.len(), 2);
    assert_eq!(envelope.document, document, "the reducer mutated its input");
    applied.envelope.verify().unwrap();
}

#[test]
fn transcript_deletions_remap_caption_anchors_and_timeline_range() {
    let envelope = ProjectEnvelope::new(populated_document()).unwrap();
    let applied = apply_transaction(
        &envelope,
        &transaction(
            0,
            "caption-remap",
            vec![Operation::SetTranscriptWordsDeleted {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                word_ids: vec![WordId::new("word-1").unwrap()],
                deleted: true,
            }],
        ),
    )
    .unwrap();
    let caption_item = &applied.envelope.document.scenes[0].tracks[1].items[0];
    let ItemContent::Caption { caption } = &caption_item.content else {
        panic!("expected caption item");
    };
    assert_eq!(
        caption.word_ids,
        [
            WordId::new("word-2").unwrap(),
            WordId::new("word-3").unwrap()
        ]
    );
    // The source cut is snapped down to the nearest project frame. The word
    // therefore begins no more than one frame after the new timeline origin.
    assert_eq!(caption_item.start_ticks, 2_000);
    assert_eq!(caption_item.duration_ticks, 170_000);
    let dialogue = &applied.envelope.document.scenes[0].tracks[0].items[0];
    assert_eq!(dialogue.start_ticks, 0);
    assert_eq!(dialogue.duration_ticks, 172_000);
    assert_eq!(dialogue.source_range.unwrap().in_ticks, 68_000);

    let removed = apply_transaction(
        &applied.envelope,
        &transaction(
            1,
            "caption-empty",
            vec![Operation::SetTranscriptWordsDeleted {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                word_ids: vec![
                    WordId::new("word-2").unwrap(),
                    WordId::new("word-3").unwrap(),
                ],
                deleted: true,
            }],
        ),
    )
    .unwrap();
    assert!(
        removed.envelope.document.scenes[0].tracks[1]
            .items
            .is_empty()
    );
    assert!(
        removed.envelope.document.scenes[0].tracks[0]
            .items
            .is_empty(),
        "deleting every spoken word removes the real linked dialogue clip"
    );
}

#[test]
fn transcript_materialization_builds_real_frame_aligned_story_clips() {
    let asset_id = AssetId::new("asset-story-source").unwrap();
    let mut asset = Asset::new(asset_id.clone(), "Interview.mp4", AssetKind::Video);
    asset.duration_ticks = Some(480_000);
    asset.has_audio = true;
    let words = vec![
        word("story-word-1", "first", 0, 60_000),
        word("story-word-2", "second", 240_000, 300_000),
    ];
    let transcript = TranscriptDocument {
        id: TranscriptId::new("transcript-story-source").unwrap(),
        asset_id: Some(asset_id.clone()),
        language: "en".into(),
        speakers: vec![TranscriptSpeaker {
            id: SpeakerId::new("speaker-1").unwrap(),
            label: "Speaker 1".into(),
            color: None,
        }],
        words: words.clone(),
        segments: vec![
            TranscriptSegment {
                id: SegmentId::new("story-segment-1").unwrap(),
                word_ids: vec![words[0].id.clone()],
                speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
            },
            TranscriptSegment {
                id: SegmentId::new("story-segment-2").unwrap(),
                word_ids: vec![words[1].id.clone()],
                speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
            },
        ],
        extensions: Extensions::new(),
    };
    let mut item = TimelineItem::new(
        ItemId::new("story-source-item").unwrap(),
        "Interview",
        0,
        480_000,
        ItemContent::Media {
            asset_id: asset_id.clone(),
            media_kind: MediaKind::Video,
        },
    );
    item.source_duration_ticks = Some(480_000);
    let mut track = Track::new(
        TrackId::new("story-source-track").unwrap(),
        "Interview",
        TrackKind::Video,
    );
    track.items.push(item);
    let scene_id = SceneId::new("story-source-scene").unwrap();
    let mut scene = Scene::new(scene_id.clone(), "Main");
    scene.is_main = true;
    scene.tracks.push(track);
    let mut document = ProjectDocument::new(project_id(), "Story materialization");
    document.assets.push(asset);
    document.scenes.push(scene);
    document.current_scene_id = Some(scene_id);

    let mut operations = vec![Operation::UpsertTranscript {
        transcript: transcript.clone(),
    }];
    operations.extend(build_story_materialization_operations(&document, &transcript).unwrap());
    let applied = apply_transaction(
        &ProjectEnvelope::new(document).unwrap(),
        &transaction(0, "materialize-story", operations),
    )
    .unwrap();

    let sequence = &applied.envelope.document.story_sequences[0];
    assert_eq!(sequence.clips.len(), 2);
    assert_eq!(sequence.clips[0].word_ids, [words[0].id.clone()]);
    assert_eq!(sequence.clips[1].word_ids, [words[1].id.clone()]);
    assert_eq!(sequence.clips[0].source_start_ticks, 0);
    assert_eq!(sequence.clips[0].source_end_ticks, 240_000);
    assert_eq!(sequence.clips[1].timeline_start_ticks, 240_000);
    assert_eq!(sequence.clips[1].source_end_ticks, 480_000);
    let timeline_items = &applied.envelope.document.scenes[0].tracks[0].items;
    assert_eq!(timeline_items.len(), 2);
    for (item, clip) in timeline_items.iter().zip(&sequence.clips) {
        assert_eq!(item.link_group_id, Some(clip.link_group_id.clone()));
        assert_eq!(item.start_ticks, clip.timeline_start_ticks);
        assert_eq!(item.source_range.unwrap().in_ticks, clip.source_start_ticks);
        assert_eq!(item.source_range.unwrap().out_ticks, clip.source_end_ticks);
    }
}

#[test]
fn closing_a_materialized_spoken_pause_trims_source_ripples_media_and_undoes_exactly() {
    let asset_id = AssetId::new("asset-pause-source").unwrap();
    let mut asset = Asset::new(asset_id.clone(), "Interview.mp4", AssetKind::Video);
    asset.duration_ticks = Some(480_000);
    asset.has_audio = true;
    let words = vec![
        word("pause-word-1", "before", 0, 60_000),
        word("pause-word-2", "after", 240_000, 300_000),
    ];
    let transcript = TranscriptDocument {
        id: TranscriptId::new("transcript-pause-source").unwrap(),
        asset_id: Some(asset_id.clone()),
        language: "en".into(),
        speakers: vec![TranscriptSpeaker {
            id: SpeakerId::new("speaker-1").unwrap(),
            label: "Speaker 1".into(),
            color: None,
        }],
        words: words.clone(),
        segments: vec![
            TranscriptSegment {
                id: SegmentId::new("pause-segment-1").unwrap(),
                word_ids: vec![words[0].id.clone()],
                speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
            },
            TranscriptSegment {
                id: SegmentId::new("pause-segment-2").unwrap(),
                word_ids: vec![words[1].id.clone()],
                speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
            },
        ],
        extensions: Extensions::new(),
    };
    let mut item = TimelineItem::new(
        ItemId::new("pause-source-item").unwrap(),
        "Interview",
        0,
        480_000,
        ItemContent::Media {
            asset_id: asset_id.clone(),
            media_kind: MediaKind::Video,
        },
    );
    item.source_duration_ticks = Some(480_000);
    let mut track = Track::new(
        TrackId::new("pause-source-track").unwrap(),
        "Interview",
        TrackKind::Video,
    );
    track.items.push(item);
    let scene_id = SceneId::new("pause-source-scene").unwrap();
    let mut scene = Scene::new(scene_id.clone(), "Main");
    scene.is_main = true;
    scene.tracks.push(track);
    let mut document = ProjectDocument::new(project_id(), "Pause compression");
    document.assets.push(asset);
    document.scenes.push(scene);
    document.current_scene_id = Some(scene_id);

    let mut materialize_operations = vec![Operation::UpsertTranscript {
        transcript: transcript.clone(),
    }];
    materialize_operations
        .extend(build_story_materialization_operations(&document, &transcript).unwrap());
    let materialized = apply_transaction(
        &ProjectEnvelope::new(document).unwrap(),
        &transaction(0, "materialize-pause-story", materialize_operations),
    )
    .unwrap();
    let materialized_document = materialized.envelope.document.clone();
    let sequence_id = materialized_document.story_sequences[0].id.clone();

    let compressed = apply_transaction(
        &materialized.envelope,
        &transaction(
            1,
            "compress-spoken-pause",
            vec![Operation::CloseStoryGaps {
                sequence_id,
                threshold_ticks: 150_000,
                target_gap_ticks: 12_000,
            }],
        ),
    )
    .unwrap();
    let sequence = &compressed.envelope.document.story_sequences[0];
    assert_eq!(sequence.clips[0].source_start_ticks, 0);
    assert_eq!(sequence.clips[0].source_end_ticks, 72_000);
    assert_eq!(sequence.clips[0].timeline_start_ticks, 0);
    assert_eq!(sequence.clips[1].source_start_ticks, 240_000);
    assert_eq!(sequence.clips[1].timeline_start_ticks, 72_000);
    assert_eq!(
        sequence.clips[1].timeline_start_ticks - words[0].end_ticks,
        12_000,
        "the requested spoken pause is preserved after removing source silence"
    );
    let items = &compressed.envelope.document.scenes[0].tracks[0].items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].start_ticks, 0);
    assert_eq!(items[0].duration_ticks, 72_000);
    assert_eq!(items[0].source_range.unwrap().out_ticks, 72_000);
    assert_eq!(items[1].start_ticks, 72_000);
    assert_eq!(items[1].source_range.unwrap().in_ticks, 240_000);
    assert_eq!(
        items[0].link_group_id,
        Some(sequence.clips[0].link_group_id.clone())
    );
    assert_eq!(
        items[1].link_group_id,
        Some(sequence.clips[1].link_group_id.clone())
    );

    let undone = apply_transaction(
        &compressed.envelope,
        &transaction(
            2,
            "undo-compress-spoken-pause",
            compressed.inverse_operations.clone(),
        ),
    )
    .unwrap();
    assert_eq!(undone.envelope.document, materialized_document);
}

#[test]
fn deleting_an_utterance_materializes_the_timeline_and_undo_restores_it() {
    let original = populated_document();
    let envelope = ProjectEnvelope::new(original.clone()).unwrap();
    let deleted = apply_transaction(
        &envelope,
        &transaction(
            0,
            "delete-utterance-timeline",
            vec![Operation::DeleteTranscriptSegment {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                segment_id: SegmentId::new("segment-1").unwrap(),
            }],
        ),
    )
    .unwrap();
    assert!(
        deleted.envelope.document.scenes[0].tracks[0]
            .items
            .is_empty(),
        "deleting an utterance must remove its real linked media"
    );
    assert!(
        deleted.envelope.document.scenes[0].tracks[1]
            .items
            .is_empty(),
        "semantic captions must remap in the same revision"
    );

    let restored = apply_transaction(
        &deleted.envelope,
        &transaction(1, "undo-delete-utterance", deleted.inverse_operations),
    )
    .unwrap();
    assert_eq!(restored.envelope.document, original);
}

#[test]
fn splitting_at_a_word_splits_real_linked_media_and_is_undoable() {
    let original = populated_document();
    let envelope = ProjectEnvelope::new(original.clone()).unwrap();
    let split = apply_transaction(
        &envelope,
        &transaction(
            0,
            "split-at-word-timeline",
            vec![Operation::SplitTranscriptSegment {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                segment_id: SegmentId::new("segment-1").unwrap(),
                at_word_id: WordId::new("word-2").unwrap(),
                new_segment_id: SegmentId::new("segment-split").unwrap(),
            }],
        ),
    )
    .unwrap();
    let sequence = &split.envelope.document.story_sequences[0];
    assert_eq!(sequence.clips.len(), 2);
    assert_eq!(sequence.clips[0].word_ids, [WordId::new("word-1").unwrap()]);
    assert_eq!(
        sequence.clips[1].word_ids,
        [
            WordId::new("word-2").unwrap(),
            WordId::new("word-3").unwrap()
        ]
    );
    assert_eq!(sequence.clips[0].source_end_ticks, 68_000);
    assert_eq!(sequence.clips[1].source_start_ticks, 68_000);
    assert_eq!(sequence.clips[1].timeline_start_ticks, 68_000);
    let timeline_items = &split.envelope.document.scenes[0].tracks[0].items;
    assert_eq!(timeline_items.len(), 2);
    assert_eq!(timeline_items[0].duration_ticks, 68_000);
    assert_eq!(timeline_items[1].start_ticks, 68_000);
    assert_eq!(
        timeline_items[1].link_group_id,
        Some(sequence.clips[1].link_group_id.clone())
    );

    let restored = apply_transaction(
        &split.envelope,
        &transaction(1, "undo-split-at-word", split.inverse_operations),
    )
    .unwrap();
    assert_eq!(restored.envelope.document, original);
}

#[test]
fn middle_word_deletion_splits_every_linked_av_item_without_drift() {
    let mut document = populated_document();
    let mut derived_track = Track::new(
        TrackId::new("derived-audio-track").unwrap(),
        "Clean dialogue",
        TrackKind::Audio,
    );
    let mut derived = document.scenes[0].tracks[0].items[0].clone();
    derived.id = ItemId::new("derived-audio-item").unwrap();
    derived.name = "Clean dialogue".into();
    derived_track.items.push(derived);
    document.scenes[0].tracks.push(derived_track);

    let envelope = ProjectEnvelope::new(document).unwrap();
    let applied = apply_transaction(
        &envelope,
        &transaction(
            0,
            "split-linked-av",
            vec![Operation::SetTranscriptWordsDeleted {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                word_ids: vec![WordId::new("word-2").unwrap()],
                deleted: true,
            }],
        ),
    )
    .unwrap();

    let sequence = &applied.envelope.document.story_sequences[0];
    assert_eq!(sequence.clips.len(), 2);
    assert_eq!(sequence.clips[0].word_ids, [WordId::new("word-1").unwrap()]);
    assert_eq!(sequence.clips[1].word_ids, [WordId::new("word-3").unwrap()]);
    assert_eq!(sequence.clips[0].timeline_start_ticks, 0);
    assert_eq!(sequence.clips[1].timeline_start_ticks, 60_000);
    assert_eq!(
        sequence.clips[0].extensions["storyCrossfade"]["fadeOutTicks"],
        2_000
    );
    assert_eq!(
        sequence.clips[1].extensions["storyCrossfade"]["fadeInTicks"],
        2_000
    );

    for track_index in [0usize, 2usize] {
        let items = &applied.envelope.document.scenes[0].tracks[track_index].items;
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].link_group_id,
            Some(sequence.clips[0].link_group_id.clone())
        );
        assert_eq!(
            items[1].link_group_id,
            Some(sequence.clips[1].link_group_id.clone())
        );
        assert_eq!(items[0].source_range.unwrap().out_ticks, 60_000);
        assert_eq!(items[1].source_range.unwrap().in_ticks, 160_000);
        assert_eq!(items[1].start_ticks, 60_000);
        assert_eq!(items[1].duration_ticks, 80_000);
        assert_eq!(
            items[1].extensions["storyEdit"]["recommendedCrossfadeTicks"],
            2_000
        );
        assert_eq!(items[0].extensions["storyCrossfade"]["fadeOutTicks"], 2_000);
        assert_eq!(items[1].extensions["storyCrossfade"]["fadeInTicks"], 2_000);
        assert_eq!(
            items[1].extensions["storyCrossfade"]["preservesLinkedAvTiming"],
            true
        );
    }
}

#[test]
fn transcript_anchored_broll_follows_words_and_uses_directional_fallback() {
    let mut document = populated_document();
    let image_id = AssetId::new("asset:broll").unwrap();
    document
        .assets
        .push(Asset::new(image_id.clone(), "B-roll.png", AssetKind::Image));
    let mut broll = TimelineItem::new(
        ItemId::new("item:broll").unwrap(),
        "B-roll",
        70_000,
        60_000,
        ItemContent::Media {
            asset_id: image_id,
            media_kind: MediaKind::Image,
        },
    );
    broll.timeline_anchor = Some(TimelineAnchor {
        transcript_id: TranscriptId::new("transcript-1").unwrap(),
        word_id: WordId::new("word-2").unwrap(),
        edge: AnchorEdge::Start,
        bias: AnchorBias::After,
        fallback_ticks: 10_000,
    });
    let mut track = Track::new(
        TrackId::new("track:broll").unwrap(),
        "B-roll",
        TrackKind::Graphic,
    );
    track.items.push(broll);
    document.scenes[0].tracks.push(track);
    let envelope = ProjectEnvelope::new(document).unwrap();
    let moved = apply_transaction(
        &envelope,
        &transaction(
            0,
            "broll-anchor-next",
            vec![Operation::SetTranscriptWordsDeleted {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                word_ids: vec![WordId::new("word-2").unwrap()],
                deleted: true,
            }],
        ),
    )
    .unwrap();
    assert_eq!(
        moved.envelope.document.scenes[0].tracks[2].items[0].start_ticks,
        60_000
    );

    let fallback = apply_transaction(
        &moved.envelope,
        &transaction(
            1,
            "broll-anchor-fallback",
            vec![Operation::SetTranscriptWordsDeleted {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                word_ids: vec![WordId::new("word-3").unwrap()],
                deleted: true,
            }],
        ),
    )
    .unwrap();
    assert_eq!(
        fallback.envelope.document.scenes[0].tracks[2].items[0].start_ticks,
        10_000
    );
}

#[test]
fn rejects_stale_revisions_as_conflicts() {
    let mut envelope = ProjectEnvelope::new(populated_document()).unwrap();
    envelope.revision = 4;
    let edit = transaction(
        3,
        "stale",
        vec![Operation::SetProjectName {
            name: "Stale".into(),
        }],
    );

    assert_eq!(
        apply_transaction(&envelope, &edit),
        Err(DomainError::RevisionConflict {
            expected_revision: 3,
            actual_revision: 4,
        })
    );
}

#[test]
fn later_operation_failure_rolls_back_the_entire_batch() {
    let envelope = ProjectEnvelope::new(populated_document()).unwrap();
    let before = envelope.clone();
    let duplicate_scene = envelope.document.scenes[0].clone();
    let edit = transaction(
        0,
        "rollback",
        vec![
            Operation::SetProjectName {
                name: "Must roll back".into(),
            },
            Operation::AddScene {
                scene: duplicate_scene,
                index: None,
            },
        ],
    );

    let error = apply_transaction(&envelope, &edit).unwrap_err();
    assert!(matches!(
        error,
        DomainError::OperationFailed {
            operation_index: 1,
            ..
        }
    ));
    assert_eq!(envelope, before);
}

#[test]
fn final_document_validation_failure_does_not_leak_partial_state() {
    let envelope = ProjectEnvelope::new(populated_document()).unwrap();
    let before = envelope.clone();
    let edit = transaction(
        0,
        "invalid-final",
        vec![Operation::SetProjectName { name: "  ".into() }],
    );

    assert!(matches!(
        apply_transaction(&envelope, &edit),
        Err(DomainError::InvalidDocument { .. })
    ));
    assert_eq!(envelope, before);
}

#[test]
fn idempotency_keys_are_validated_and_payloads_have_stable_fingerprints() {
    assert!(serde_json::from_str::<IdempotencyKey>(r#"""#).is_err());
    assert!(serde_json::from_str::<IdempotencyKey>(r#""contains space""#).is_err());

    let first = transaction(
        0,
        "retry",
        vec![Operation::SetProjectName {
            name: "Same request".into(),
        }],
    );
    let round_tripped: EditTransaction =
        serde_json::from_str(&serde_json::to_string(&first).unwrap()).unwrap();
    assert_eq!(
        transaction_fingerprint(&first).unwrap(),
        transaction_fingerprint(&round_tripped).unwrap()
    );

    let mut changed = first.clone();
    changed.operations = vec![Operation::SetProjectName {
        name: "Different payload".into(),
    }];
    assert_ne!(
        transaction_fingerprint(&first).unwrap(),
        transaction_fingerprint(&changed).unwrap(),
        "a daemon can reject reuse of one idempotency key with another payload"
    );

    let empty = transaction(0, "empty", Vec::new());
    let envelope = ProjectEnvelope::new(populated_document()).unwrap();
    assert!(matches!(
        validate_transaction(&envelope, &empty),
        Err(DomainError::InvalidTransaction { ref field, .. }) if field == "operations"
    ));
}

#[test]
fn transcript_edits_preserve_spoken_text_and_support_segment_operations() {
    let envelope = ProjectEnvelope::new(populated_document()).unwrap();
    let edit = transaction(
        0,
        "transcript",
        vec![
            Operation::SetTranscriptWordsDeleted {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                word_ids: vec![WordId::new("word-1").unwrap()],
                deleted: true,
            },
            Operation::SetTranscriptDisplayText {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                word_id: WordId::new("word-2").unwrap(),
                display_text: "Hello!".into(),
            },
            Operation::SetTranscriptSpeaker {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                word_ids: vec![WordId::new("word-2").unwrap()],
                speaker_id: None,
            },
            Operation::SplitTranscriptSegment {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                segment_id: SegmentId::new("segment-1").unwrap(),
                at_word_id: WordId::new("word-2").unwrap(),
                new_segment_id: SegmentId::new("segment-2").unwrap(),
            },
        ],
    );
    let applied = apply_transaction(&envelope, &edit).unwrap();
    let transcript = &applied.envelope.document.transcripts[0];

    assert!(transcript.words[0].deleted);
    assert_eq!(transcript.words[1].spoken_text, "hello");
    assert_eq!(transcript.words[1].display_text, "Hello!");
    assert_eq!(transcript.words[1].speaker_id, None);
    assert_eq!(transcript.segments.len(), 2);
    assert_eq!(
        transcript.segments[1].word_ids,
        vec![
            WordId::new("word-2").unwrap(),
            WordId::new("word-3").unwrap()
        ]
    );

    let merge = transaction(
        1,
        "merge",
        vec![Operation::MergeTranscriptSegments {
            transcript_id: TranscriptId::new("transcript-1").unwrap(),
            first_segment_id: SegmentId::new("segment-1").unwrap(),
            second_segment_id: SegmentId::new("segment-2").unwrap(),
        }],
    );
    let merged = apply_transaction(&applied.envelope, &merge).unwrap();
    assert_eq!(merged.envelope.document.transcripts[0].segments.len(), 1);
}

#[test]
fn applying_inverse_operations_restores_the_exact_document() {
    let original = ProjectEnvelope::new(populated_document()).unwrap();
    let edit = transaction(
        0,
        "forward",
        vec![
            Operation::SetProjectName {
                name: "Changed".into(),
            },
            Operation::DeleteTranscriptSegment {
                transcript_id: TranscriptId::new("transcript-1").unwrap(),
                segment_id: SegmentId::new("segment-1").unwrap(),
            },
            Operation::SetCaptionStyle {
                item_id: ItemId::new("caption-item-1").unwrap(),
                style: CaptionStyle {
                    font_size: 80.0,
                    ..CaptionStyle::default()
                },
            },
        ],
    );
    let forward = apply_transaction(&original, &edit).unwrap();
    let undo = transaction(1, "undo", forward.inverse_operations.clone());
    let undone = apply_transaction(&forward.envelope, &undo).unwrap();

    assert_eq!(undone.envelope.document, original.document);
    assert_eq!(undone.envelope.document_hash, original.document_hash);
    assert_eq!(undone.envelope.revision, 2);
}

#[test]
fn classic_unknown_item_fields_survive_json_round_trip() {
    let mut item = TimelineItem::new(
        ItemId::new("graphic-item-1").unwrap(),
        "Lower third",
        0,
        120_000,
        ItemContent::Custom {
            custom_type: "classicGraphic".into(),
            data: json!({"definitionId": "lower-third"}),
        },
    );
    item.extensions = BTreeMap::from([
        ("transform".into(), json!({"x": 120, "rotation": 4})),
        ("style".into(), json!({"fontWeight": 700})),
        ("keyframes".into(), json!([{"time": 0, "value": 0}])),
        ("trimStart".into(), json!(12_000)),
        ("trimEnd".into(), json!(8_000)),
    ]);

    let encoded = serde_json::to_value(&item).unwrap();
    assert_eq!(encoded["transform"]["rotation"], 4);
    assert_eq!(encoded["keyframes"][0]["time"], 0);
    let decoded: TimelineItem = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, item);
}

#[test]
fn close_story_gaps_is_semantic_and_undoable() {
    let mut document = populated_document();
    let sequence = &mut document.story_sequences[0];
    sequence.clips.push(StoryClip {
        id: StoryClipId::new("clip-2").unwrap(),
        word_ids: vec![WordId::new("word-3").unwrap()],
        timeline_start_ticks: 600_000,
        source_start_ticks: 160_000,
        source_end_ticks: 240_000,
        link_group_id: LinkGroupId::new("link-2").unwrap(),
        extensions: Extensions::new(),
    });
    let mut second_item = document.scenes[0].tracks[0].items[0].clone();
    second_item.id = ItemId::new("audio-item-2").unwrap();
    second_item.start_ticks = 600_000;
    second_item.duration_ticks = 80_000;
    second_item.source_range = Some(openchatcut_domain::SourceRange {
        in_ticks: 160_000,
        out_ticks: 240_000,
    });
    second_item.link_group_id = Some(LinkGroupId::new("link-2").unwrap());
    document.scenes[0].tracks[0].items.push(second_item);
    let envelope = ProjectEnvelope::new(document).unwrap();
    let edit = transaction(
        0,
        "close-gap",
        vec![Operation::CloseStoryGaps {
            sequence_id: StorySequenceId::new("story-1").unwrap(),
            threshold_ticks: 180_000,
            target_gap_ticks: 12_000,
        }],
    );
    let applied = apply_transaction(&envelope, &edit).unwrap();
    assert_eq!(
        applied.envelope.document.story_sequences[0].clips[1].timeline_start_ticks,
        252_000
    );
    assert_eq!(
        applied.envelope.document.scenes[0].tracks[0].items[1].start_ticks,
        252_000
    );
}
