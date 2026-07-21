use std::collections::HashSet;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    DomainError, Extensions, ItemContent, LinkGroupId, MediaKind, Operation, ProjectDocument,
    SourceRange, StoryClip, StoryClipId, StorySequence, StorySequenceId, TICKS_PER_SECOND,
    TimelineItem, TrackId, TranscriptDocument,
};

fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::InvalidOperation {
        message: message.into(),
    }
}

fn frame_duration_ticks(document: &ProjectDocument) -> Result<i64, DomainError> {
    let fps = document.settings.fps;
    if fps.numerator == 0 {
        return Err(invalid("project frame rate numerator must be positive"));
    }
    let numerator = (TICKS_PER_SECOND as i128)
        .checked_mul(fps.denominator as i128)
        .ok_or(DomainError::ArithmeticOverflow)?;
    let denominator = fps.numerator as i128;
    let rounded = numerator
        .checked_add(denominator / 2)
        .ok_or(DomainError::ArithmeticOverflow)?
        / denominator;
    i64::try_from(rounded.max(1)).map_err(|_| DomainError::ArithmeticOverflow)
}

fn align_down(value: i64, quantum: i64) -> Result<i64, DomainError> {
    if value < 0 || quantum <= 0 {
        return Err(invalid("story boundaries require non-negative time"));
    }
    Ok(value / quantum * quantum)
}

fn stable_suffix(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let mut suffix = String::with_capacity(20);
    for byte in &digest[..10] {
        use std::fmt::Write as _;
        write!(&mut suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    suffix
}

fn linked_piece(
    original: &TimelineItem,
    clip: &StoryClip,
    piece_index: usize,
    frame_ticks: i64,
) -> Result<TimelineItem, DomainError> {
    let mut piece = original.clone();
    if piece_index > 0 {
        piece.id = crate::ItemId::new(format!(
            "{}:story:{}",
            original.id,
            stable_suffix(&[original.id.as_str(), clip.id.as_str()])
        ))
        .map_err(|error| invalid(error.to_string()))?;
    }
    let duration_ticks = clip
        .duration_ticks()
        .filter(|duration| *duration > 0)
        .ok_or_else(|| invalid("story clip duration must be positive"))?;
    piece.start_ticks = clip.timeline_start_ticks;
    piece.duration_ticks = duration_ticks;
    piece.source_range = Some(SourceRange {
        in_ticks: clip.source_start_ticks,
        out_ticks: clip.source_end_ticks,
    });
    piece.link_group_id = Some(clip.link_group_id.clone());
    piece.extensions.insert(
        "storyEdit".into(),
        json!({
            "clipId": clip.id,
            "materializedFromItemId": original.id,
            "frameAligned": true,
            "recommendedCrossfadeTicks": (frame_ticks / 2).clamp(1, 2_400),
        }),
    );
    Ok(piece)
}

/// Build the semantic operations that turn a transcript's primary timeline
/// placement into real, frame-aligned A/V clips.
///
/// The function is intentionally pure. The daemon may include the returned
/// operations in the same revision as `UpsertTranscript`, so a crash can never
/// leave text and timeline materialization at different revisions.
pub fn build_story_materialization_operations(
    document: &ProjectDocument,
    transcript: &TranscriptDocument,
) -> Result<Vec<Operation>, DomainError> {
    if document
        .story_sequences
        .iter()
        .any(|sequence| sequence.transcript_id == transcript.id)
    {
        return Ok(Vec::new());
    }
    let Some(asset_id) = transcript.asset_id.as_ref() else {
        return Ok(Vec::new());
    };
    let frame_ticks = frame_duration_ticks(document)?;

    let mut candidates = document
        .scenes
        .iter()
        .flat_map(|scene| &scene.tracks)
        .flat_map(|track| {
            track.items.iter().filter_map(move |item| {
                let ItemContent::Media {
                    asset_id: item_asset_id,
                    media_kind: MediaKind::Audio | MediaKind::Video,
                } = &item.content
                else {
                    return None;
                };
                if item_asset_id != asset_id || item.link_group_id.is_some() {
                    return None;
                }
                let source_start = item.source_range.map_or(0, |range| range.in_ticks);
                let source_end = item.source_range.map_or_else(
                    || source_start.checked_add(item.duration_ticks),
                    |range| Some(range.out_ticks),
                )?;
                (source_end > source_start
                    && source_end.checked_sub(source_start) == Some(item.duration_ticks))
                .then_some((track.id.clone(), item.clone(), source_start, source_end))
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, item, source_start, _)| {
        (item.start_ticks, *source_start, item.id.to_string())
    });
    let Some((_, primary, source_start, source_end)) = candidates.first().cloned() else {
        return Ok(Vec::new());
    };
    let linked = candidates
        .into_iter()
        .filter(|(_, item, candidate_start, candidate_end)| {
            item.start_ticks == primary.start_ticks
                && item.duration_ticks == primary.duration_ticks
                && *candidate_start == source_start
                && *candidate_end == source_end
        })
        .collect::<Vec<_>>();

    let sequence_suffix = stable_suffix(&[
        document.id.as_str(),
        transcript.id.as_str(),
        primary.id.as_str(),
    ]);
    let sequence_id = StorySequenceId::new(format!("story:{sequence_suffix}"))
        .map_err(|error| invalid(error.to_string()))?;
    let words_by_id = transcript
        .words
        .iter()
        .map(|word| (word.id.clone(), word))
        .collect::<std::collections::HashMap<_, _>>();
    let mut segment_words = transcript
        .segments
        .iter()
        .filter_map(|segment| {
            let words = segment
                .word_ids
                .iter()
                .filter_map(|word_id| words_by_id.get(word_id).copied())
                .filter(|word| {
                    !word.deleted && word.start_ticks < source_end && word.end_ticks > source_start
                })
                .collect::<Vec<_>>();
            (!words.is_empty()).then_some((segment.id.as_str(), words))
        })
        .collect::<Vec<_>>();
    if segment_words.is_empty() {
        let words = transcript
            .words
            .iter()
            .filter(|word| {
                !word.deleted && word.start_ticks < source_end && word.end_ticks > source_start
            })
            .collect::<Vec<_>>();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        segment_words.push(("all", words));
    }

    let segment_count = segment_words.len();
    let mut clips = Vec::with_capacity(segment_count);
    for (index, (segment_id, words)) in segment_words.iter().enumerate() {
        let first_word = words.first().expect("empty segments were removed");
        let clip_source_start = if index == 0 {
            source_start
        } else {
            align_down(first_word.start_ticks, frame_ticks)?.max(source_start)
        };
        let clip_source_end = if index + 1 == segment_count {
            source_end
        } else {
            let next_word = segment_words[index + 1]
                .1
                .first()
                .expect("empty segments were removed");
            align_down(next_word.start_ticks, frame_ticks)?.min(source_end)
        };
        if clip_source_end <= clip_source_start {
            return Err(invalid("transcript segment produced an empty story clip"));
        }
        let clip_suffix = stable_suffix(&[sequence_id.as_str(), segment_id]);
        let timeline_start_ticks = primary
            .start_ticks
            .checked_add(
                clip_source_start
                    .checked_sub(source_start)
                    .ok_or(DomainError::ArithmeticOverflow)?,
            )
            .ok_or(DomainError::ArithmeticOverflow)?;
        clips.push(StoryClip {
            id: StoryClipId::new(format!("story-clip:{clip_suffix}"))
                .map_err(|error| invalid(error.to_string()))?,
            word_ids: words.iter().map(|word| word.id.clone()).collect(),
            timeline_start_ticks,
            source_start_ticks: clip_source_start,
            source_end_ticks: clip_source_end,
            link_group_id: LinkGroupId::new(format!("link:story:{clip_suffix}"))
                .map_err(|error| invalid(error.to_string()))?,
            extensions: Extensions::new(),
        });
    }

    let mut operations = vec![Operation::UpsertStorySequence {
        sequence: StorySequence {
            id: sequence_id,
            transcript_id: transcript.id.clone(),
            clips: clips.clone(),
            extensions: [(
                "storyMaterialization".into(),
                json!({
                    "version": 1,
                    "sourceAssetId": asset_id,
                    "sourceItemId": primary.id,
                    "frameAligned": true,
                }),
            )]
            .into_iter()
            .collect(),
        },
    }];
    let mut generated_ids = HashSet::new();
    for (track_id, original, _, _) in linked {
        for (piece_index, clip) in clips.iter().enumerate() {
            let piece = linked_piece(&original, clip, piece_index, frame_ticks)?;
            if !generated_ids.insert(piece.id.clone()) {
                return Err(DomainError::DuplicateEntity {
                    entity: "materialized timeline item".into(),
                    id: piece.id.to_string(),
                });
            }
            if piece_index == 0 {
                operations.push(Operation::ReplaceItem {
                    item_id: original.id.clone(),
                    item: piece,
                });
            } else {
                operations.push(Operation::InsertItem {
                    track_id: TrackId::new(track_id.as_str())
                        .map_err(|error| invalid(error.to_string()))?,
                    item: piece,
                    index: None,
                });
            }
        }
    }
    Ok(operations)
}
