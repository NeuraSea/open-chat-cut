use std::collections::{HashMap, HashSet};

use crate::{
    AssetKind, AssetProvenance, CURRENT_SCHEMA_VERSION, CaptionElement, CaptionStyle, DomainError,
    Extensions, ItemContent, MediaKind, ProjectDocument, TimelineItem, TrackKind,
    TranscriptDocument,
};

fn invalid(path: impl Into<String>, message: impl Into<String>) -> DomainError {
    DomainError::InvalidDocument {
        path: path.into(),
        message: message.into(),
    }
}

fn require_name(value: &str, path: &str) -> Result<(), DomainError> {
    if value.trim().is_empty() {
        return Err(invalid(path, "must not be blank"));
    }
    if value.len() > 512 {
        return Err(invalid(path, "must not exceed 512 bytes"));
    }
    Ok(())
}

fn unique(ids: &mut HashSet<String>, id: &str, entity: &'static str) -> Result<(), DomainError> {
    if !ids.insert(id.to_owned()) {
        return Err(DomainError::DuplicateEntity {
            entity: entity.into(),
            id: id.into(),
        });
    }
    Ok(())
}

fn check_extensions(
    extensions: &Extensions,
    path: &str,
    reserved: &[&str],
) -> Result<(), DomainError> {
    if let Some(key) = reserved.iter().find(|key| extensions.contains_key(**key)) {
        return Err(invalid(
            format!("{path}.extensions.{key}"),
            "duplicates a typed field",
        ));
    }
    Ok(())
}

fn validate_style(style: &CaptionStyle, path: &str) -> Result<(), DomainError> {
    require_name(&style.font_family, &format!("{path}.fontFamily"))?;
    for (field, value) in [
        ("fontSize", style.font_size),
        ("outlineWidth", style.outline_width),
        ("positionX", style.position_x),
        ("positionY", style.position_y),
        ("maxWidth", style.max_width),
        ("lineHeight", style.line_height),
    ] {
        if !value.is_finite() {
            return Err(invalid(format!("{path}.{field}"), "must be finite"));
        }
    }
    if style.font_size <= 0.0 || style.line_height <= 0.0 || style.max_width <= 0.0 {
        return Err(invalid(
            path,
            "fontSize, lineHeight, and maxWidth must be positive",
        ));
    }
    if style.outline_width < 0.0 {
        return Err(invalid(
            format!("{path}.outlineWidth"),
            "must not be negative",
        ));
    }
    check_extensions(
        &style.extensions,
        path,
        &[
            "fontFamily",
            "fontSize",
            "textColor",
            "activeWordColor",
            "backgroundColor",
            "outlineColor",
            "outlineWidth",
            "positionX",
            "positionY",
            "maxWidth",
            "lineHeight",
            "textAlign",
        ],
    )
}

fn validate_caption(
    caption: &CaptionElement,
    transcripts: &HashMap<String, &TranscriptDocument>,
    path: &str,
) -> Result<(), DomainError> {
    require_name(&caption.language, &format!("{path}.language"))?;
    if caption.word_ids.is_empty() {
        return Err(invalid(format!("{path}.wordIds"), "must not be empty"));
    }
    let transcript = transcripts
        .get(caption.transcript_id.as_str())
        .ok_or_else(|| DomainError::EntityNotFound {
            entity: "caption transcript".into(),
            id: caption.transcript_id.to_string(),
        })?;
    let words = transcript
        .words
        .iter()
        .map(|word| word.id.as_str())
        .collect::<HashSet<_>>();
    let mut caption_words = HashSet::new();
    for word_id in &caption.word_ids {
        if !words.contains(word_id.as_str()) {
            return Err(DomainError::EntityNotFound {
                entity: "caption word".into(),
                id: word_id.to_string(),
            });
        }
        if !caption_words.insert(word_id.as_str()) {
            return Err(DomainError::DuplicateEntity {
                entity: "caption word".into(),
                id: word_id.to_string(),
            });
        }
    }
    if let Some(speaker_id) = &caption.speaker_id
        && !transcript
            .speakers
            .iter()
            .any(|speaker| speaker.id == *speaker_id)
    {
        return Err(DomainError::EntityNotFound {
            entity: "caption speaker".into(),
            id: speaker_id.to_string(),
        });
    }
    validate_style(&caption.style, &format!("{path}.style"))?;
    check_extensions(
        &caption.extensions,
        path,
        &[
            "transcriptId",
            "wordIds",
            "language",
            "translationOfLanguage",
            "speakerId",
            "presetId",
            "style",
        ],
    )
}

fn validate_transcript(
    transcript: &TranscriptDocument,
    assets: &HashSet<String>,
    path: &str,
) -> Result<(), DomainError> {
    require_name(&transcript.language, &format!("{path}.language"))?;
    if let Some(asset_id) = &transcript.asset_id
        && !assets.contains(asset_id.as_str())
    {
        return Err(DomainError::EntityNotFound {
            entity: "transcript asset".into(),
            id: asset_id.to_string(),
        });
    }

    let mut speaker_ids = HashSet::new();
    for (index, speaker) in transcript.speakers.iter().enumerate() {
        unique(&mut speaker_ids, speaker.id.as_str(), "transcript speaker")?;
        require_name(&speaker.label, &format!("{path}.speakers[{index}].label"))?;
    }

    let mut word_ids = HashSet::new();
    let mut previous_start = None;
    for (index, word) in transcript.words.iter().enumerate() {
        unique(&mut word_ids, word.id.as_str(), "transcript word")?;
        require_name(
            &word.spoken_text,
            &format!("{path}.words[{index}].spokenText"),
        )?;
        require_name(
            &word.display_text,
            &format!("{path}.words[{index}].displayText"),
        )?;
        if word.start_ticks < 0 || word.end_ticks <= word.start_ticks {
            return Err(invalid(
                format!("{path}.words[{index}]"),
                "requires 0 <= startTicks < endTicks",
            ));
        }
        if previous_start.is_some_and(|start| word.start_ticks < start) {
            return Err(invalid(
                format!("{path}.words[{index}].startTicks"),
                "source words must remain ordered by source time",
            ));
        }
        previous_start = Some(word.start_ticks);
        if let Some(confidence) = word.confidence
            && (!confidence.is_finite() || !(0.0..=1.0).contains(&confidence))
        {
            return Err(invalid(
                format!("{path}.words[{index}].confidence"),
                "must be finite and between 0 and 1",
            ));
        }
        if let Some(speaker_id) = &word.speaker_id
            && !speaker_ids.contains(speaker_id.as_str())
        {
            return Err(DomainError::EntityNotFound {
                entity: "transcript word speaker".into(),
                id: speaker_id.to_string(),
            });
        }
        check_extensions(
            &word.extensions,
            &format!("{path}.words[{index}]"),
            &[
                "id",
                "spokenText",
                "displayText",
                "startTicks",
                "endTicks",
                "speakerId",
                "deleted",
                "confidence",
            ],
        )?;
    }

    let mut segment_ids = HashSet::new();
    let mut segmented_word_ids = HashSet::new();
    for (index, segment) in transcript.segments.iter().enumerate() {
        unique(&mut segment_ids, segment.id.as_str(), "transcript segment")?;
        if segment.word_ids.is_empty() {
            return Err(invalid(
                format!("{path}.segments[{index}].wordIds"),
                "must not be empty",
            ));
        }
        for word_id in &segment.word_ids {
            if !word_ids.contains(word_id.as_str()) {
                return Err(DomainError::EntityNotFound {
                    entity: "transcript segment word".into(),
                    id: word_id.to_string(),
                });
            }
            if !segmented_word_ids.insert(word_id.as_str()) {
                return Err(DomainError::DuplicateEntity {
                    entity: "segmented transcript word".into(),
                    id: word_id.to_string(),
                });
            }
        }
        if let Some(speaker_id) = &segment.speaker_id
            && !speaker_ids.contains(speaker_id.as_str())
        {
            return Err(DomainError::EntityNotFound {
                entity: "transcript segment speaker".into(),
                id: speaker_id.to_string(),
            });
        }
    }
    check_extensions(
        &transcript.extensions,
        path,
        &["id", "assetId", "language", "speakers", "words", "segments"],
    )
}

fn validate_track_item(
    item: &TimelineItem,
    track_kind: TrackKind,
    assets: &HashMap<String, (AssetKind, bool)>,
    transcripts: &HashMap<String, &TranscriptDocument>,
    path: &str,
) -> Result<(), DomainError> {
    require_name(&item.name, &format!("{path}.name"))?;
    if item.start_ticks < 0 || item.duration_ticks <= 0 || item.end_ticks().is_none() {
        return Err(invalid(
            path,
            "requires a non-negative startTicks, positive durationTicks, and no overflow",
        ));
    }
    if let Some(source) = item.source_range {
        if source.in_ticks < 0 || source.out_ticks <= source.in_ticks {
            return Err(invalid(
                format!("{path}.sourceRange"),
                "requires 0 <= inTicks < outTicks",
            ));
        }
        if let Some(source_duration) = item.source_duration_ticks
            && (source_duration <= 0 || source.out_ticks > source_duration)
        {
            return Err(invalid(
                format!("{path}.sourceDurationTicks"),
                "must be positive and include sourceRange.outTicks",
            ));
        }
    } else if item
        .source_duration_ticks
        .is_some_and(|duration| duration <= 0)
    {
        return Err(invalid(
            format!("{path}.sourceDurationTicks"),
            "must be positive",
        ));
    }
    if let Some(anchor) = &item.timeline_anchor {
        if anchor.fallback_ticks < 0 {
            return Err(invalid(
                format!("{path}.timelineAnchor.fallbackTicks"),
                "must be non-negative",
            ));
        }
        let transcript = transcripts
            .get(anchor.transcript_id.as_str())
            .ok_or_else(|| DomainError::EntityNotFound {
                entity: "timeline anchor transcript".into(),
                id: anchor.transcript_id.to_string(),
            })?;
        if !transcript
            .words
            .iter()
            .any(|word| word.id == anchor.word_id)
        {
            return Err(DomainError::EntityNotFound {
                entity: "timeline anchor word".into(),
                id: anchor.word_id.to_string(),
            });
        }
    }

    let compatible = match (&track_kind, &item.content) {
        (_, ItemContent::Custom { .. }) => true,
        (TrackKind::Video, ItemContent::Media { media_kind, .. }) => {
            matches!(media_kind, MediaKind::Video | MediaKind::Image)
        }
        (TrackKind::Audio, ItemContent::Media { media_kind, .. }) => {
            *media_kind == MediaKind::Audio
        }
        (TrackKind::Text, ItemContent::Text { .. } | ItemContent::Caption { .. }) => true,
        (TrackKind::Caption, ItemContent::Caption { .. }) => true,
        (
            TrackKind::Graphic,
            ItemContent::MotionGraphic { .. }
            | ItemContent::Sticker { .. }
            | ItemContent::Media {
                media_kind: MediaKind::Image,
                ..
            },
        ) => true,
        (TrackKind::Effect, ItemContent::Effect { .. }) => true,
        _ => false,
    };
    if !compatible {
        return Err(invalid(
            path,
            "item content is incompatible with its track kind",
        ));
    }

    match &item.content {
        ItemContent::Media {
            asset_id,
            media_kind,
        } => {
            let (asset_kind, asset_has_audio) =
                assets
                    .get(asset_id.as_str())
                    .ok_or_else(|| DomainError::EntityNotFound {
                        entity: "timeline item asset".into(),
                        id: asset_id.to_string(),
                    })?;
            let kind_matches = match (*asset_kind, *media_kind) {
                (AssetKind::Video, MediaKind::Video)
                | (AssetKind::Image, MediaKind::Image)
                | (AssetKind::Audio, MediaKind::Audio) => true,
                // A Classic video import can expose its embedded audio as a
                // linked audio-lane item while both views retain one managed
                // content-addressed asset. `mediaKind` describes the timeline
                // use here, so permit that audio view only when the video was
                // actually probed as containing audio.
                (AssetKind::Video, MediaKind::Audio) => *asset_has_audio,
                _ => false,
            };
            if !kind_matches {
                return Err(invalid(
                    path,
                    "mediaKind does not match the referenced asset kind",
                ));
            }
        }
        ItemContent::Text { .. } => {}
        ItemContent::Caption { caption } => {
            validate_caption(caption, transcripts, &format!("{path}.caption"))?;
        }
        ItemContent::MotionGraphic { motion_graphic } => {
            if motion_graphic.dsl_version == 0 {
                return Err(invalid(
                    format!("{path}.motionGraphic.dslVersion"),
                    "must be positive",
                ));
            }
        }
        ItemContent::Sticker { sticker_id } => {
            require_name(sticker_id, &format!("{path}.stickerId"))?;
        }
        ItemContent::Effect { effect_type } => {
            require_name(effect_type, &format!("{path}.effectType"))?;
        }
        ItemContent::Custom { custom_type, .. } => {
            require_name(custom_type, &format!("{path}.customType"))?;
        }
    }
    check_extensions(
        &item.extensions,
        path,
        &[
            "id",
            "name",
            "startTicks",
            "durationTicks",
            "sourceRange",
            "sourceDurationTicks",
            "linkGroupId",
            "timelineAnchor",
            "enabled",
            "content",
        ],
    )
}

pub fn validate_document(document: &ProjectDocument) -> Result<(), DomainError> {
    if document.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(DomainError::UnsupportedSchemaVersion {
            actual: document.schema_version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    require_name(&document.name, "name")?;
    let settings = &document.settings;
    if settings.fps.numerator == 0 || settings.fps.denominator == 0 {
        return Err(invalid(
            "settings.fps",
            "numerator and denominator must be positive",
        ));
    }
    if settings.canvas_size.width == 0
        || settings.canvas_size.height == 0
        || settings.canvas_size.width > 16_384
        || settings.canvas_size.height > 16_384
    {
        return Err(invalid(
            "settings.canvasSize",
            "width and height must be between 1 and 16384",
        ));
    }
    if let crate::Background::Blur { blur_intensity } = settings.background
        && (!blur_intensity.is_finite() || blur_intensity < 0.0)
    {
        return Err(invalid(
            "settings.background.blurIntensity",
            "must be finite and non-negative",
        ));
    }
    check_extensions(
        &settings.extensions,
        "settings",
        &["fps", "canvasSize", "background"],
    )?;

    let mut asset_ids = HashSet::new();
    let mut asset_kinds = HashMap::new();
    for (index, asset) in document.assets.iter().enumerate() {
        unique(&mut asset_ids, asset.id.as_str(), "asset")?;
        require_name(&asset.name, &format!("assets[{index}].name"))?;
        if asset.duration_ticks.is_some_and(|duration| duration <= 0) {
            return Err(invalid(
                format!("assets[{index}].durationTicks"),
                "must be positive",
            ));
        }
        if asset.width == Some(0) || asset.height == Some(0) {
            return Err(invalid(
                format!("assets[{index}]"),
                "asset dimensions must be positive",
            ));
        }
        asset_kinds.insert(asset.id.to_string(), (asset.kind, asset.has_audio));
        check_extensions(
            &asset.extensions,
            &format!("assets[{index}]"),
            &[
                "id",
                "name",
                "kind",
                "contentHash",
                "durationTicks",
                "width",
                "height",
                "hasAudio",
                "provenance",
            ],
        )?;
    }
    for asset in &document.assets {
        if let AssetProvenance::Derived {
            parent_asset_id, ..
        } = &asset.provenance
            && !asset_ids.contains(parent_asset_id.as_str())
        {
            return Err(DomainError::EntityNotFound {
                entity: "parent asset".into(),
                id: parent_asset_id.to_string(),
            });
        }
    }

    let mut transcript_ids = HashSet::new();
    for (index, transcript) in document.transcripts.iter().enumerate() {
        unique(&mut transcript_ids, transcript.id.as_str(), "transcript")?;
        validate_transcript(transcript, &asset_ids, &format!("transcripts[{index}]"))?;
    }
    let transcripts = document
        .transcripts
        .iter()
        .map(|transcript| (transcript.id.to_string(), transcript))
        .collect::<HashMap<_, _>>();

    let mut scene_ids = HashSet::new();
    let mut track_ids = HashSet::new();
    let mut item_ids = HashSet::new();
    let mut main_count = 0;
    for (scene_index, scene) in document.scenes.iter().enumerate() {
        unique(&mut scene_ids, scene.id.as_str(), "scene")?;
        require_name(&scene.name, &format!("scenes[{scene_index}].name"))?;
        main_count += usize::from(scene.is_main);
        for (bookmark_index, bookmark) in scene.bookmarks.iter().enumerate() {
            if bookmark.time_ticks < 0
                || bookmark
                    .duration_ticks
                    .is_some_and(|duration| duration <= 0)
            {
                return Err(invalid(
                    format!("scenes[{scene_index}].bookmarks[{bookmark_index}]"),
                    "timeTicks must be non-negative and durationTicks must be positive",
                ));
            }
        }
        for (track_index, track) in scene.tracks.iter().enumerate() {
            unique(&mut track_ids, track.id.as_str(), "track")?;
            require_name(
                &track.name,
                &format!("scenes[{scene_index}].tracks[{track_index}].name"),
            )?;
            for (item_index, item) in track.items.iter().enumerate() {
                unique(&mut item_ids, item.id.as_str(), "timeline item")?;
                validate_track_item(
                    item,
                    track.kind,
                    &asset_kinds,
                    &transcripts,
                    &format!("scenes[{scene_index}].tracks[{track_index}].items[{item_index}]"),
                )?;
            }
            check_extensions(
                &track.extensions,
                &format!("scenes[{scene_index}].tracks[{track_index}]"),
                &["id", "name", "kind", "muted", "hidden", "locked", "items"],
            )?;
        }
        check_extensions(
            &scene.extensions,
            &format!("scenes[{scene_index}]"),
            &["id", "name", "isMain", "tracks", "bookmarks"],
        )?;
    }
    if main_count > 1 {
        return Err(invalid("scenes", "at most one scene may be marked isMain"));
    }
    if !document.scenes.is_empty() && document.current_scene_id.is_none() {
        return Err(invalid(
            "currentSceneId",
            "is required when the project contains scenes",
        ));
    }
    if let Some(current_scene_id) = &document.current_scene_id
        && !scene_ids.contains(current_scene_id.as_str())
    {
        return Err(DomainError::EntityNotFound {
            entity: "current scene".into(),
            id: current_scene_id.to_string(),
        });
    }

    let mut sequence_ids = HashSet::new();
    let mut clip_ids = HashSet::new();
    let mut story_link_group_ids = HashSet::new();
    for (sequence_index, sequence) in document.story_sequences.iter().enumerate() {
        unique(&mut sequence_ids, sequence.id.as_str(), "story sequence")?;
        let transcript = transcripts
            .get(sequence.transcript_id.as_str())
            .ok_or_else(|| DomainError::EntityNotFound {
                entity: "story sequence transcript".into(),
                id: sequence.transcript_id.to_string(),
            })?;
        let transcript_words = transcript
            .words
            .iter()
            .map(|word| word.id.as_str())
            .collect::<HashSet<_>>();
        let mut previous_start = None;
        for (clip_index, clip) in sequence.clips.iter().enumerate() {
            unique(&mut clip_ids, clip.id.as_str(), "story clip")?;
            unique(
                &mut story_link_group_ids,
                clip.link_group_id.as_str(),
                "story clip link group",
            )?;
            if clip.word_ids.is_empty()
                || clip.timeline_start_ticks < 0
                || clip.source_start_ticks < 0
                || clip.source_end_ticks <= clip.source_start_ticks
                || clip.timeline_end_ticks().is_none()
            {
                return Err(invalid(
                    format!("storySequences[{sequence_index}].clips[{clip_index}]"),
                    "requires words and valid, non-overflowing timeline/source ranges",
                ));
            }
            if previous_start.is_some_and(|start| clip.timeline_start_ticks < start) {
                return Err(invalid(
                    format!(
                        "storySequences[{sequence_index}].clips[{clip_index}].timelineStartTicks"
                    ),
                    "clips must be ordered by timeline position",
                ));
            }
            previous_start = Some(clip.timeline_start_ticks);
            let mut clip_word_ids = HashSet::new();
            for word_id in &clip.word_ids {
                if !transcript_words.contains(word_id.as_str()) {
                    return Err(DomainError::EntityNotFound {
                        entity: "story clip word".into(),
                        id: word_id.to_string(),
                    });
                }
                if !clip_word_ids.insert(word_id.as_str()) {
                    return Err(DomainError::DuplicateEntity {
                        entity: "story clip word".into(),
                        id: word_id.to_string(),
                    });
                }
            }
            check_extensions(
                &clip.extensions,
                &format!("storySequences[{sequence_index}].clips[{clip_index}]"),
                &[
                    "id",
                    "wordIds",
                    "timelineStartTicks",
                    "sourceStartTicks",
                    "sourceEndTicks",
                    "linkGroupId",
                ],
            )?;
        }
        check_extensions(
            &sequence.extensions,
            &format!("storySequences[{sequence_index}]"),
            &["id", "transcriptId", "clips"],
        )?;
    }

    check_extensions(
        &document.extensions,
        "project",
        &[
            "schemaVersion",
            "id",
            "name",
            "settings",
            "scenes",
            "currentSceneId",
            "assets",
            "transcripts",
            "storySequences",
        ],
    )?;
    Ok(())
}
