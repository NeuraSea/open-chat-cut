use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    ActorKind, AnchorBias, AnchorEdge, DomainError, EditTransaction, ItemContent, Operation,
    ProjectDocument, ProjectEnvelope, Revision, SourceRange, StoryClip, StorySequence, TrackPatch,
    TransactionFingerprint, TranscriptId, active_caption_word_ranges, canonical_document_hash,
    transaction_fingerprint, validate_document,
};

const MAX_OPERATIONS_PER_TRANSACTION: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Project,
    SceneGraph,
    Scene,
    Track,
    TimelineItem,
    Caption,
    Asset,
    Transcript,
    StorySequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeAction {
    Add,
    Remove,
    Update,
    Move,
    Replace,
    Split,
    Merge,
    Reorder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSummary {
    pub operation_index: usize,
    pub kind: ChangeKind,
    pub action: ChangeAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub base_revision: Revision,
    pub next_revision: Revision,
    pub current_document_hash: crate::DocumentHash,
    pub resulting_document_hash: crate::DocumentHash,
    pub transaction_fingerprint: TransactionFingerprint,
    pub operation_count: usize,
    pub changes: Vec<ChangeSummary>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationsOutcome {
    pub document: ProjectDocument,
    pub inverse_operations: Vec<Operation>,
    pub changes: Vec<ChangeSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub envelope: ProjectEnvelope,
    pub inverse_operations: Vec<Operation>,
    pub changes: Vec<ChangeSummary>,
    pub transaction_fingerprint: TransactionFingerprint,
}

fn invalid_operation(message: impl Into<String>) -> DomainError {
    DomainError::InvalidOperation {
        message: message.into(),
    }
}

fn entity_not_found(entity: &'static str, id: impl ToString) -> DomainError {
    DomainError::EntityNotFound {
        entity: entity.into(),
        id: id.to_string(),
    }
}

fn locate_track(document: &ProjectDocument, track_id: &crate::TrackId) -> Option<(usize, usize)> {
    document
        .scenes
        .iter()
        .enumerate()
        .find_map(|(scene_index, scene)| {
            scene
                .tracks
                .iter()
                .position(|track| track.id == *track_id)
                .map(|track_index| (scene_index, track_index))
        })
}

fn locate_item(
    document: &ProjectDocument,
    item_id: &crate::ItemId,
) -> Option<(usize, usize, usize)> {
    document
        .scenes
        .iter()
        .enumerate()
        .find_map(|(scene_index, scene)| {
            scene
                .tracks
                .iter()
                .enumerate()
                .find_map(|(track_index, track)| {
                    track
                        .items
                        .iter()
                        .position(|item| item.id == *item_id)
                        .map(|item_index| (scene_index, track_index, item_index))
                })
        })
}

fn operation_transcript_id(operation: &Operation) -> Option<&TranscriptId> {
    match operation {
        Operation::UpsertTranscript { transcript } => Some(&transcript.id),
        Operation::RemoveTranscript { transcript_id }
        | Operation::SetTranscriptWordsDeleted { transcript_id, .. }
        | Operation::DeleteTranscriptSegment { transcript_id, .. }
        | Operation::SetTranscriptDisplayText { transcript_id, .. }
        | Operation::SetTranscriptSpeaker { transcript_id, .. }
        | Operation::SplitTranscriptSegment { transcript_id, .. }
        | Operation::MergeTranscriptSegments { transcript_id, .. }
        | Operation::ReorderTranscriptSegments { transcript_id, .. } => Some(transcript_id),
        _ => None,
    }
}

fn story_operation_transcript_id(
    document: &ProjectDocument,
    operation: &Operation,
) -> Option<TranscriptId> {
    match operation {
        Operation::UpsertStorySequence { sequence } => Some(sequence.transcript_id.clone()),
        Operation::RemoveStorySequence { sequence_id }
        | Operation::ReorderStoryClips { sequence_id, .. }
        | Operation::CloseStoryGaps { sequence_id, .. } => document
            .story_sequences
            .iter()
            .find(|sequence| sequence.id == *sequence_id)
            .map(|sequence| sequence.transcript_id.clone()),
        _ => None,
    }
}

/// Keep semantic captions anchored to active transcript words after every
/// transcript mutation. A StorySequence, when present, supplies the materialized
/// timeline time; otherwise source word time is used. Empty caption items are
/// removed rather than leaving an invalid element that blocks the transaction.
fn reconcile_captions_for_transcript(
    document: &mut ProjectDocument,
    transcript_id: &TranscriptId,
) -> Result<(), DomainError> {
    if !document
        .transcripts
        .iter()
        .any(|transcript| transcript.id == *transcript_id)
    {
        return Ok(());
    }
    let mapped = active_caption_word_ranges(document, transcript_id)?;

    for scene in &mut document.scenes {
        for track in &mut scene.tracks {
            track.items.retain_mut(|item| {
                let ItemContent::Caption { caption } = &mut item.content else {
                    return true;
                };
                if caption.transcript_id != *transcript_id {
                    return true;
                }
                caption
                    .word_ids
                    .retain(|word_id| mapped.contains_key(word_id));
                caption.word_ids.sort_by_key(|word_id| {
                    mapped.get(word_id).map_or((i64::MAX, i64::MAX), |range| {
                        (range.start_ticks, range.end_ticks)
                    })
                });
                caption.word_ids.dedup();
                let Some(first) = caption.word_ids.first().and_then(|id| mapped.get(id)) else {
                    return false;
                };
                let Some(last) = caption.word_ids.last().and_then(|id| mapped.get(id)) else {
                    return false;
                };
                let Some(duration) = last.end_ticks.checked_sub(first.start_ticks) else {
                    return false;
                };
                if duration <= 0 {
                    return false;
                }
                item.start_ticks = first.start_ticks;
                item.duration_ticks = duration;
                true
            });
        }
    }
    Ok(())
}

/// Remap transcript-anchored B-roll/overlays after script edits. Deleted anchor
/// words use the declared directional bias; if no suitable active word remains,
/// the explicit fallback preserves deterministic placement.
fn reconcile_timeline_anchors_for_transcript(
    document: &mut ProjectDocument,
    transcript_id: &TranscriptId,
) -> Result<(), DomainError> {
    let Some(transcript) = document
        .transcripts
        .iter()
        .find(|transcript| transcript.id == *transcript_id)
    else {
        return Ok(());
    };
    let ranges = active_caption_word_ranges(document, transcript_id)?;
    let source_indices = transcript
        .words
        .iter()
        .enumerate()
        .map(|(index, word)| (word.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let source_starts = transcript
        .words
        .iter()
        .map(|word| (word.id.clone(), word.start_ticks))
        .collect::<HashMap<_, _>>();
    let mut resolved = HashMap::new();
    for item in document
        .scenes
        .iter()
        .flat_map(|scene| &scene.tracks)
        .flat_map(|track| &track.items)
    {
        let Some(anchor) = &item.timeline_anchor else {
            continue;
        };
        if anchor.transcript_id != *transcript_id {
            continue;
        }
        let range = ranges.get(&anchor.word_id).or_else(|| {
            let anchor_index = source_indices.get(&anchor.word_id).copied();
            match anchor.bias {
                AnchorBias::Before => anchor_index.and_then(|index| {
                    transcript.words[..index]
                        .iter()
                        .rev()
                        .find_map(|word| ranges.get(&word.id))
                }),
                AnchorBias::After => anchor_index.and_then(|index| {
                    transcript.words[index.saturating_add(1)..]
                        .iter()
                        .find_map(|word| ranges.get(&word.id))
                }),
                AnchorBias::Nearest => {
                    let reference = source_starts
                        .get(&anchor.word_id)
                        .copied()
                        .unwrap_or(anchor.fallback_ticks);
                    transcript
                        .words
                        .iter()
                        .filter_map(|word| {
                            ranges.get(&word.id).map(|range| {
                                (
                                    word.start_ticks.abs_diff(reference),
                                    word.start_ticks,
                                    range,
                                )
                            })
                        })
                        .min_by_key(|(distance, source_start, _)| (*distance, *source_start))
                        .map(|(_, _, range)| range)
                }
            }
        });
        let start_ticks = range.map_or(anchor.fallback_ticks, |range| match anchor.edge {
            AnchorEdge::Start => range.start_ticks,
            AnchorEdge::End => range.end_ticks,
        });
        resolved.insert(item.id.clone(), start_ticks);
    }
    for item in document
        .scenes
        .iter_mut()
        .flat_map(|scene| &mut scene.tracks)
        .flat_map(|track| &mut track.items)
    {
        if let Some(start_ticks) = resolved.get(&item.id) {
            item.start_ticks = *start_ticks;
        }
    }
    Ok(())
}

fn ensure_permutation<T>(current: &[T], requested: &[T], entity: &str) -> Result<(), DomainError>
where
    T: Eq + std::hash::Hash + ToString,
{
    if current.len() != requested.len() {
        return Err(invalid_operation(format!(
            "{entity} reorder must contain every ID exactly once"
        )));
    }
    let current = current
        .iter()
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    let requested = requested
        .iter()
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    if current.len() != requested.len() || current != requested {
        return Err(invalid_operation(format!(
            "{entity} reorder must contain every ID exactly once"
        )));
    }
    Ok(())
}

fn apply_track_patch(track: &mut crate::Track, patch: &TrackPatch) -> Result<(), DomainError> {
    if patch.is_empty() {
        return Err(invalid_operation(
            "track patch must change at least one property",
        ));
    }
    if let Some(name) = &patch.name {
        track.name.clone_from(name);
    }
    if let Some(muted) = patch.muted {
        track.muted = muted;
    }
    if let Some(hidden) = patch.hidden {
        track.hidden = hidden;
    }
    if let Some(locked) = patch.locked {
        track.locked = locked;
    }
    Ok(())
}

fn transcript_mut<'a>(
    document: &'a mut ProjectDocument,
    transcript_id: &crate::TranscriptId,
) -> Result<&'a mut crate::TranscriptDocument, DomainError> {
    document
        .transcripts
        .iter_mut()
        .find(|transcript| transcript.id == *transcript_id)
        .ok_or_else(|| entity_not_found("transcript", transcript_id))
}

fn story_sequence_mut<'a>(
    document: &'a mut ProjectDocument,
    sequence_id: &crate::StorySequenceId,
) -> Result<&'a mut crate::StorySequence, DomainError> {
    document
        .story_sequences
        .iter_mut()
        .find(|sequence| sequence.id == *sequence_id)
        .ok_or_else(|| entity_not_found("story sequence", sequence_id))
}

fn frame_duration_ticks(document: &ProjectDocument) -> Result<i64, DomainError> {
    let fps = document.settings.fps;
    let numerator = (crate::TICKS_PER_SECOND as i128)
        .checked_mul(fps.denominator as i128)
        .ok_or(DomainError::ArithmeticOverflow)?;
    let denominator = fps.numerator as i128;
    if denominator == 0 {
        return Err(invalid_operation(
            "project frame rate numerator must be positive",
        ));
    }
    let rounded = numerator
        .checked_add(denominator / 2)
        .ok_or(DomainError::ArithmeticOverflow)?
        / denominator;
    i64::try_from(rounded.max(1)).map_err(|_| DomainError::ArithmeticOverflow)
}

fn align_down(value: i64, quantum: i64) -> Result<i64, DomainError> {
    if value < 0 || quantum <= 0 {
        return Err(invalid_operation(
            "story edit boundaries require non-negative time and a positive frame duration",
        ));
    }
    Ok(value / quantum * quantum)
}

fn align_up(value: i64, quantum: i64) -> Result<i64, DomainError> {
    if value < 0 || quantum <= 0 {
        return Err(invalid_operation(
            "story edit boundaries require non-negative time and a positive frame duration",
        ));
    }
    value
        .checked_add(quantum - 1)
        .and_then(|value| value.checked_div(quantum))
        .and_then(|value| value.checked_mul(quantum))
        .ok_or(DomainError::ArithmeticOverflow)
}

fn derived_stable_id(base: &str, kind: &str, salt: &str) -> String {
    let digest = Sha256::digest(format!("{kind}\0{base}\0{salt}").as_bytes());
    let mut suffix = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        write!(&mut suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    let separator_bytes = kind.len() + suffix.len() + 2;
    let base_limit = 256usize.saturating_sub(separator_bytes);
    let base = &base[..base.len().min(base_limit)];
    format!("{base}:{kind}:{suffix}")
}

fn apply_story_clip_to_item(
    item: &mut crate::TimelineItem,
    clip: &StoryClip,
    frame_ticks: i64,
) -> Result<(), DomainError> {
    let duration_ticks = clip
        .duration_ticks()
        .filter(|duration| *duration > 0)
        .ok_or_else(|| invalid_operation("story clip duration must be positive"))?;
    item.start_ticks = clip.timeline_start_ticks;
    item.duration_ticks = duration_ticks;
    item.link_group_id = Some(clip.link_group_id.clone());
    if matches!(
        item.content,
        ItemContent::Media {
            media_kind: crate::MediaKind::Audio | crate::MediaKind::Video,
            ..
        }
    ) {
        item.source_range = Some(SourceRange {
            in_ticks: clip.source_start_ticks,
            out_ticks: clip.source_end_ticks,
        });
    }
    item.extensions.insert(
        "storyEdit".into(),
        json!({
            "clipId": clip.id,
            "frameAligned": true,
            "recommendedCrossfadeTicks": (frame_ticks / 2).clamp(1, 2_400),
        }),
    );
    if let Some(crossfade) = clip.extensions.get("storyCrossfade") {
        item.extensions
            .insert("storyCrossfade".into(), crossfade.clone());
    } else {
        item.extensions.remove("storyCrossfade");
    }
    Ok(())
}

/// Add bounded equal-power audio envelopes at every closed story boundary.
/// Timeline/source placement remains identical for every linked A/V item; the
/// envelope only smooths the audio discontinuity and cannot introduce drift.
fn annotate_story_crossfades(sequence: &mut StorySequence, frame_ticks: i64) {
    let base = (frame_ticks / 2).clamp(1, 2_400);
    let boundaries = sequence
        .clips
        .windows(2)
        .map(|pair| {
            pair[0].timeline_end_ticks() == Some(pair[1].timeline_start_ticks)
                && pair[0]
                    .duration_ticks()
                    .is_some_and(|duration| duration > 1)
                && pair[1]
                    .duration_ticks()
                    .is_some_and(|duration| duration > 1)
        })
        .collect::<Vec<_>>();
    for index in 0..sequence.clips.len() {
        let duration = sequence.clips[index].duration_ticks().unwrap_or(1);
        let maximum = (duration / 2).max(1);
        let fade_in_ticks = if index > 0 && boundaries[index - 1] {
            base.min(maximum)
        } else {
            0
        };
        let fade_out_ticks = if index < boundaries.len() && boundaries[index] {
            base.min(maximum)
        } else {
            0
        };
        if fade_in_ticks > 0 || fade_out_ticks > 0 {
            sequence.clips[index].extensions.insert(
                "storyCrossfade".into(),
                json!({
                    "version": 1,
                    "fadeInTicks": fade_in_ticks,
                    "fadeOutTicks": fade_out_ticks,
                    "curve": "equalPower",
                    "preservesLinkedAvTiming": true,
                }),
            );
        } else {
            sequence.clips[index].extensions.remove("storyCrossfade");
        }
    }
}

/// Apply StorySequence placement to every linked A/V item. A link group may
/// contain the original picture, dialogue, and any derived audio, so they are
/// always moved and trimmed as one edit rather than drifting independently.
fn reconcile_story_linked_items(
    document: &mut ProjectDocument,
    sequence_id: &crate::StorySequenceId,
) -> Result<(), DomainError> {
    let frame_ticks = frame_duration_ticks(document)?;
    let sequence = document
        .story_sequences
        .iter_mut()
        .find(|sequence| sequence.id == *sequence_id)
        .ok_or_else(|| entity_not_found("story sequence", sequence_id))?;
    annotate_story_crossfades(sequence, frame_ticks);
    let clips = sequence
        .clips
        .iter()
        .map(|clip| (clip.link_group_id.clone(), clip.clone()))
        .collect::<HashMap<_, _>>();
    for item in document
        .scenes
        .iter_mut()
        .flat_map(|scene| &mut scene.tracks)
        .flat_map(|track| &mut track.items)
    {
        let Some(link_group_id) = &item.link_group_id else {
            continue;
        };
        let Some(clip) = clips.get(link_group_id) else {
            continue;
        };
        apply_story_clip_to_item(item, clip, frame_ticks)?;
    }
    Ok(())
}

/// Convert deleted transcript words into real, frame-aligned linked timeline
/// cuts. Existing gaps between source clips are preserved; time removed inside
/// a clip ripples every later clip left. Splits clone every linked A/V item so
/// picture, dialogue, derived audio, and captions remain on the same clock.
fn materialize_story_word_deletions(
    document: &mut ProjectDocument,
    transcript_id: &TranscriptId,
) -> Result<(), DomainError> {
    let frame_ticks = frame_duration_ticks(document)?;
    let transcript = document
        .transcripts
        .iter()
        .find(|transcript| transcript.id == *transcript_id)
        .ok_or_else(|| entity_not_found("transcript", transcript_id))?
        .clone();
    let words = transcript
        .words
        .iter()
        .map(|word| (word.id.clone(), word))
        .collect::<HashMap<_, _>>();

    let sequence_indices = document
        .story_sequences
        .iter()
        .enumerate()
        .filter_map(|(index, sequence)| (sequence.transcript_id == *transcript_id).then_some(index))
        .collect::<Vec<_>>();

    for sequence_index in sequence_indices {
        let original_clips = document.story_sequences[sequence_index].clips.clone();
        let mut replacements = HashMap::<crate::LinkGroupId, Vec<StoryClip>>::new();
        let mut new_clips = Vec::new();
        let mut removed_before = 0i64;

        for original in original_clips {
            let original_duration = original
                .duration_ticks()
                .filter(|duration| *duration > 0)
                .ok_or_else(|| invalid_operation("story clip duration must be positive"))?;
            let active = original
                .word_ids
                .iter()
                .map(|word_id| {
                    words
                        .get(word_id)
                        .map(|word| !word.deleted)
                        .ok_or_else(|| entity_not_found("story clip word", word_id))
                })
                .collect::<Result<Vec<_>, _>>()?;

            if active.iter().all(|active| *active) {
                let mut clip = original.clone();
                clip.timeline_start_ticks = clip
                    .timeline_start_ticks
                    .checked_sub(removed_before)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                replacements.insert(original.link_group_id.clone(), vec![clip.clone()]);
                new_clips.push(clip);
                continue;
            }

            let mut runs = Vec::<(usize, usize)>::new();
            let mut cursor = 0;
            while cursor < active.len() {
                if !active[cursor] {
                    cursor += 1;
                    continue;
                }
                let start = cursor;
                while cursor < active.len() && active[cursor] {
                    cursor += 1;
                }
                runs.push((start, cursor));
            }

            let adjusted_start = original
                .timeline_start_ticks
                .checked_sub(removed_before)
                .ok_or(DomainError::ArithmeticOverflow)?;
            let mut kept_duration = 0i64;
            let mut parts = Vec::with_capacity(runs.len());
            for (part_index, (run_start, run_end)) in runs.into_iter().enumerate() {
                let first_word = words.get(&original.word_ids[run_start]).ok_or_else(|| {
                    entity_not_found("story clip word", &original.word_ids[run_start])
                })?;
                let last_word = words.get(&original.word_ids[run_end - 1]).ok_or_else(|| {
                    entity_not_found("story clip word", &original.word_ids[run_end - 1])
                })?;
                let source_start = if run_start == 0 {
                    original.source_start_ticks
                } else {
                    align_down(first_word.start_ticks, frame_ticks)?
                        .max(original.source_start_ticks)
                };
                let source_end = if run_end == original.word_ids.len() {
                    original.source_end_ticks
                } else {
                    align_up(last_word.end_ticks, frame_ticks)?.min(original.source_end_ticks)
                };
                if source_end <= source_start {
                    return Err(invalid_operation(
                        "frame-aligned transcript cut produced an empty source range",
                    ));
                }
                let duration = source_end
                    .checked_sub(source_start)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                let timeline_start_ticks = adjusted_start
                    .checked_add(kept_duration)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                let first_word_id = &original.word_ids[run_start];
                let last_word_id = &original.word_ids[run_end - 1];
                let salt = format!("{}:{}", first_word_id, last_word_id);
                let (id, link_group_id) = if part_index == 0 {
                    (original.id.clone(), original.link_group_id.clone())
                } else {
                    (
                        crate::StoryClipId::new(derived_stable_id(
                            original.id.as_str(),
                            "part",
                            &salt,
                        ))
                        .map_err(|error| invalid_operation(error.to_string()))?,
                        crate::LinkGroupId::new(derived_stable_id(
                            original.link_group_id.as_str(),
                            "part",
                            &salt,
                        ))
                        .map_err(|error| invalid_operation(error.to_string()))?,
                    )
                };
                let mut clip = StoryClip {
                    id,
                    word_ids: original.word_ids[run_start..run_end].to_vec(),
                    timeline_start_ticks,
                    source_start_ticks: source_start,
                    source_end_ticks: source_end,
                    link_group_id,
                    extensions: original.extensions.clone(),
                };
                clip.extensions.insert(
                    "storyEdit".into(),
                    json!({
                        "sourceClipId": original.id,
                        "frameAligned": true,
                        "recommendedCrossfadeTicks": (frame_ticks / 2).clamp(1, 2_400),
                    }),
                );
                kept_duration = kept_duration
                    .checked_add(duration)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                parts.push(clip.clone());
                new_clips.push(clip);
            }

            let removed = original_duration.saturating_sub(kept_duration);
            removed_before = removed_before
                .checked_add(removed)
                .ok_or(DomainError::ArithmeticOverflow)?;
            replacements.insert(original.link_group_id.clone(), parts);
        }

        document.story_sequences[sequence_index].clips = new_clips;
        annotate_story_crossfades(&mut document.story_sequences[sequence_index], frame_ticks);
        let annotated = document.story_sequences[sequence_index]
            .clips
            .iter()
            .map(|clip| (clip.link_group_id.clone(), clip.clone()))
            .collect::<HashMap<_, _>>();
        for parts in replacements.values_mut() {
            for part in parts {
                if let Some(clip) = annotated.get(&part.link_group_id) {
                    part.clone_from(clip);
                }
            }
        }

        let existing_ids = document
            .scenes
            .iter()
            .flat_map(|scene| &scene.tracks)
            .flat_map(|track| &track.items)
            .map(|item| item.id.clone())
            .collect::<HashSet<_>>();
        let mut generated_ids = HashSet::new();
        for track in document
            .scenes
            .iter_mut()
            .flat_map(|scene| &mut scene.tracks)
        {
            let mut materialized = Vec::with_capacity(track.items.len());
            for item in track.items.drain(..) {
                let Some(link_group_id) = item.link_group_id.as_ref() else {
                    materialized.push(item);
                    continue;
                };
                let Some(parts) = replacements.get(link_group_id) else {
                    materialized.push(item);
                    continue;
                };
                for (part_index, clip) in parts.iter().enumerate() {
                    let mut part = item.clone();
                    if part_index > 0 {
                        let id = crate::ItemId::new(derived_stable_id(
                            item.id.as_str(),
                            "story",
                            clip.id.as_str(),
                        ))
                        .map_err(|error| invalid_operation(error.to_string()))?;
                        if existing_ids.contains(&id) || !generated_ids.insert(id.clone()) {
                            return Err(DomainError::DuplicateEntity {
                                entity: "materialized timeline item".into(),
                                id: id.to_string(),
                            });
                        }
                        part.id = id;
                    }
                    apply_story_clip_to_item(&mut part, clip, frame_ticks)?;
                    materialized.push(part);
                }
            }
            track.items = materialized;
        }
    }
    Ok(())
}

/// Split every materialized occurrence of a transcript word at the same
/// frame-aligned source boundary. Linked picture/dialogue/derived-audio items
/// are cloned as one group, so a script split is immediately a real timeline
/// split rather than merely a paragraph change.
fn split_story_at_word(
    document: &mut ProjectDocument,
    transcript_id: &TranscriptId,
    at_word_id: &crate::WordId,
) -> Result<(), DomainError> {
    let frame_ticks = frame_duration_ticks(document)?;
    let split_source_ticks = document
        .transcripts
        .iter()
        .find(|transcript| transcript.id == *transcript_id)
        .and_then(|transcript| transcript.word(at_word_id))
        .map(|word| word.start_ticks)
        .ok_or_else(|| entity_not_found("story split word", at_word_id))?;
    let sequence_indices = document
        .story_sequences
        .iter()
        .enumerate()
        .filter_map(|(index, sequence)| (sequence.transcript_id == *transcript_id).then_some(index))
        .collect::<Vec<_>>();

    for sequence_index in sequence_indices {
        let Some(clip_index) = document.story_sequences[sequence_index]
            .clips
            .iter()
            .position(|clip| clip.word_ids.contains(at_word_id))
        else {
            continue;
        };
        let original = document.story_sequences[sequence_index].clips[clip_index].clone();
        let word_index = original
            .word_ids
            .iter()
            .position(|word_id| word_id == at_word_id)
            .expect("the containing clip was located above");
        if word_index == 0 {
            continue;
        }
        let boundary = align_down(split_source_ticks, frame_ticks)?
            .clamp(original.source_start_ticks, original.source_end_ticks);
        if boundary <= original.source_start_ticks || boundary >= original.source_end_ticks {
            return Err(invalid_operation(
                "frame-aligned transcript split must be inside its story clip",
            ));
        }
        let mut left = original.clone();
        left.word_ids = original.word_ids[..word_index].to_vec();
        left.source_end_ticks = boundary;
        let salt = format!("{}:{}", original.id, at_word_id);
        let mut right = StoryClip {
            id: crate::StoryClipId::new(derived_stable_id(original.id.as_str(), "split", &salt))
                .map_err(|error| invalid_operation(error.to_string()))?,
            word_ids: original.word_ids[word_index..].to_vec(),
            timeline_start_ticks: original
                .timeline_start_ticks
                .checked_add(
                    boundary
                        .checked_sub(original.source_start_ticks)
                        .ok_or(DomainError::ArithmeticOverflow)?,
                )
                .ok_or(DomainError::ArithmeticOverflow)?,
            source_start_ticks: boundary,
            source_end_ticks: original.source_end_ticks,
            link_group_id: crate::LinkGroupId::new(derived_stable_id(
                original.link_group_id.as_str(),
                "split",
                &salt,
            ))
            .map_err(|error| invalid_operation(error.to_string()))?,
            extensions: original.extensions.clone(),
        };
        left.extensions.insert(
            "storyEdit".into(),
            json!({
                "sourceClipId": original.id,
                "splitAtWordId": at_word_id,
                "frameAligned": true,
            }),
        );
        right.extensions.insert(
            "storyEdit".into(),
            json!({
                "sourceClipId": original.id,
                "splitAtWordId": at_word_id,
                "frameAligned": true,
            }),
        );
        document.story_sequences[sequence_index]
            .clips
            .splice(clip_index..=clip_index, [left, right]);
        annotate_story_crossfades(&mut document.story_sequences[sequence_index], frame_ticks);
        let split_clips =
            document.story_sequences[sequence_index].clips[clip_index..=clip_index + 1].to_vec();

        let existing_ids = document
            .scenes
            .iter()
            .flat_map(|scene| &scene.tracks)
            .flat_map(|track| &track.items)
            .map(|item| item.id.clone())
            .collect::<HashSet<_>>();
        let mut generated_ids = HashSet::new();
        for track in document
            .scenes
            .iter_mut()
            .flat_map(|scene| &mut scene.tracks)
        {
            let mut materialized = Vec::with_capacity(track.items.len() + 1);
            for item in track.items.drain(..) {
                if item.link_group_id.as_ref() != Some(&original.link_group_id) {
                    materialized.push(item);
                    continue;
                }
                for (part_index, clip) in split_clips.iter().enumerate() {
                    let mut part = item.clone();
                    if part_index > 0 {
                        let id = crate::ItemId::new(derived_stable_id(
                            item.id.as_str(),
                            "story-split",
                            clip.id.as_str(),
                        ))
                        .map_err(|error| invalid_operation(error.to_string()))?;
                        if existing_ids.contains(&id) || !generated_ids.insert(id.clone()) {
                            return Err(DomainError::DuplicateEntity {
                                entity: "materialized timeline item".into(),
                                id: id.to_string(),
                            });
                        }
                        part.id = id;
                    }
                    apply_story_clip_to_item(&mut part, clip, frame_ticks)?;
                    materialized.push(part);
                }
            }
            track.items = materialized;
        }
    }
    Ok(())
}

fn close_story_pauses(
    document: &mut ProjectDocument,
    sequence_id: &crate::StorySequenceId,
    threshold_ticks: i64,
    target_gap_ticks: i64,
) -> Result<(), DomainError> {
    let frame_ticks = frame_duration_ticks(document)?;
    let transcript_id = document
        .story_sequences
        .iter()
        .find(|sequence| sequence.id == *sequence_id)
        .map(|sequence| sequence.transcript_id.clone())
        .ok_or_else(|| entity_not_found("story sequence", sequence_id))?;
    let words = document
        .transcripts
        .iter()
        .find(|transcript| transcript.id == transcript_id)
        .ok_or_else(|| entity_not_found("story sequence transcript", &transcript_id))?
        .words
        .iter()
        .map(|word| (word.id.clone(), word.clone()))
        .collect::<HashMap<_, _>>();
    let sequence = story_sequence_mut(document, sequence_id)?;

    // A freshly materialized story keeps the source continuous: inter-utterance
    // silence lives at the tail of the preceding clip. Trim that source tail
    // first, then ripple all following clips by exactly the removed duration.
    for index in 1..sequence.clips.len() {
        let previous_word = sequence.clips[index - 1]
            .word_ids
            .iter()
            .rev()
            .find_map(|word_id| words.get(word_id));
        let next_word = sequence.clips[index]
            .word_ids
            .iter()
            .find_map(|word_id| words.get(word_id));
        let Some((previous_word, next_word)) = previous_word.zip(next_word) else {
            continue;
        };
        let spoken_gap = next_word
            .start_ticks
            .checked_sub(previous_word.end_ticks)
            .ok_or(DomainError::ArithmeticOverflow)?;
        if spoken_gap <= threshold_ticks {
            continue;
        }
        let desired_source_end = align_up(
            previous_word
                .end_ticks
                .checked_add(target_gap_ticks)
                .ok_or(DomainError::ArithmeticOverflow)?,
            frame_ticks,
        )?
        .min(sequence.clips[index].source_start_ticks);
        let old_source_end = sequence.clips[index - 1].source_end_ticks;
        if desired_source_end <= sequence.clips[index - 1].source_start_ticks
            || desired_source_end >= old_source_end
        {
            continue;
        }
        let removed = old_source_end
            .checked_sub(desired_source_end)
            .ok_or(DomainError::ArithmeticOverflow)?;
        sequence.clips[index - 1].source_end_ticks = desired_source_end;
        for clip in &mut sequence.clips[index..] {
            clip.timeline_start_ticks = clip
                .timeline_start_ticks
                .checked_sub(removed)
                .ok_or(DomainError::ArithmeticOverflow)?;
        }
    }

    // Preserve support for imported/manual StorySequences that explicitly
    // represent silence as an empty timeline gap.
    for index in 1..sequence.clips.len() {
        let previous_end = sequence.clips[index - 1]
            .timeline_end_ticks()
            .ok_or(DomainError::ArithmeticOverflow)?;
        let gap = sequence.clips[index]
            .timeline_start_ticks
            .checked_sub(previous_end)
            .ok_or(DomainError::ArithmeticOverflow)?;
        if gap > threshold_ticks {
            let shift = gap
                .checked_sub(target_gap_ticks)
                .ok_or(DomainError::ArithmeticOverflow)?;
            for clip in &mut sequence.clips[index..] {
                clip.timeline_start_ticks = clip
                    .timeline_start_ticks
                    .checked_sub(shift)
                    .ok_or(DomainError::ArithmeticOverflow)?;
            }
        }
    }
    reconcile_story_linked_items(document, sequence_id)
}

fn apply_operation(
    document: &mut ProjectDocument,
    operation: &Operation,
) -> Result<(), DomainError> {
    match operation {
        Operation::ReplaceDocument {
            document: replacement,
        } => {
            if replacement.id != document.id {
                return Err(DomainError::ProjectMismatch {
                    expected_project_id: document.id.clone(),
                    actual_project_id: replacement.id.clone(),
                });
            }
            *document = replacement.as_ref().clone();
        }
        Operation::ReplaceSceneGraph {
            scenes,
            current_scene_id,
        } => {
            document.scenes.clone_from(scenes);
            document.current_scene_id.clone_from(current_scene_id);
        }
        Operation::SetProjectName { name } => document.name.clone_from(name),
        Operation::SetProjectSettings { settings } => document.settings.clone_from(settings),
        Operation::AddAsset { asset } => {
            if document.assets.iter().any(|current| current.id == asset.id) {
                return Err(DomainError::DuplicateEntity {
                    entity: "asset".into(),
                    id: asset.id.to_string(),
                });
            }
            document.assets.push(asset.clone());
        }
        Operation::UpsertAsset { asset } => {
            if let Some(current) = document
                .assets
                .iter_mut()
                .find(|current| current.id == asset.id)
            {
                current.clone_from(asset);
            } else {
                document.assets.push(asset.clone());
            }
        }
        Operation::RemoveAsset { asset_id } => {
            if let Some(item) = document
                .scenes
                .iter()
                .flat_map(|scene| &scene.tracks)
                .flat_map(|track| &track.items)
                .find(|item| item.content.asset_id() == Some(asset_id))
            {
                return Err(DomainError::ReferentialIntegrity {
                    entity: "asset".into(),
                    id: asset_id.to_string(),
                    referenced_by: format!("timeline item {}", item.id),
                });
            }
            if let Some(transcript) = document
                .transcripts
                .iter()
                .find(|transcript| transcript.asset_id.as_ref() == Some(asset_id))
            {
                return Err(DomainError::ReferentialIntegrity {
                    entity: "asset".into(),
                    id: asset_id.to_string(),
                    referenced_by: format!("transcript {}", transcript.id),
                });
            }
            if let Some(asset) = document.assets.iter().find(|asset| {
                matches!(
                    &asset.provenance,
                    crate::AssetProvenance::Derived { parent_asset_id, .. }
                        if parent_asset_id == asset_id
                )
            }) {
                return Err(DomainError::ReferentialIntegrity {
                    entity: "asset".into(),
                    id: asset_id.to_string(),
                    referenced_by: format!("derived asset {}", asset.id),
                });
            }
            let index = document
                .assets
                .iter()
                .position(|asset| asset.id == *asset_id)
                .ok_or_else(|| entity_not_found("asset", asset_id))?;
            document.assets.remove(index);
        }
        Operation::AddScene { scene, index } => {
            if document.scenes.iter().any(|current| current.id == scene.id) {
                return Err(DomainError::DuplicateEntity {
                    entity: "scene".into(),
                    id: scene.id.to_string(),
                });
            }
            let index = index.unwrap_or(document.scenes.len());
            if index > document.scenes.len() {
                return Err(invalid_operation("scene insertion index is out of bounds"));
            }
            document.scenes.insert(index, scene.clone());
            if document.current_scene_id.is_none() {
                document.current_scene_id = Some(scene.id.clone());
            }
        }
        Operation::RemoveScene { scene_id } => {
            let index = document
                .scenes
                .iter()
                .position(|scene| scene.id == *scene_id)
                .ok_or_else(|| entity_not_found("scene", scene_id))?;
            document.scenes.remove(index);
            if document.current_scene_id.as_ref() == Some(scene_id) {
                document.current_scene_id = document.scenes.first().map(|scene| scene.id.clone());
            }
        }
        Operation::SetSceneName { scene_id, name } => {
            let scene = document
                .scenes
                .iter_mut()
                .find(|scene| scene.id == *scene_id)
                .ok_or_else(|| entity_not_found("scene", scene_id))?;
            scene.name.clone_from(name);
        }
        Operation::AddTrack {
            scene_id,
            track,
            index,
        } => {
            if locate_track(document, &track.id).is_some() {
                return Err(DomainError::DuplicateEntity {
                    entity: "track".into(),
                    id: track.id.to_string(),
                });
            }
            let scene = document
                .scenes
                .iter_mut()
                .find(|scene| scene.id == *scene_id)
                .ok_or_else(|| entity_not_found("scene", scene_id))?;
            let index = index.unwrap_or(scene.tracks.len());
            if index > scene.tracks.len() {
                return Err(invalid_operation("track insertion index is out of bounds"));
            }
            scene.tracks.insert(index, track.clone());
        }
        Operation::RemoveTrack { track_id } => {
            let (scene_index, track_index) = locate_track(document, track_id)
                .ok_or_else(|| entity_not_found("track", track_id))?;
            document.scenes[scene_index].tracks.remove(track_index);
        }
        Operation::SetTrackProperties { track_id, patch } => {
            let (scene_index, track_index) = locate_track(document, track_id)
                .ok_or_else(|| entity_not_found("track", track_id))?;
            apply_track_patch(&mut document.scenes[scene_index].tracks[track_index], patch)?;
        }
        Operation::InsertItem {
            track_id,
            item,
            index,
        } => {
            if locate_item(document, &item.id).is_some() {
                return Err(DomainError::DuplicateEntity {
                    entity: "timeline item".into(),
                    id: item.id.to_string(),
                });
            }
            let (scene_index, track_index) = locate_track(document, track_id)
                .ok_or_else(|| entity_not_found("track", track_id))?;
            let items = &mut document.scenes[scene_index].tracks[track_index].items;
            let index = index.unwrap_or(items.len());
            if index > items.len() {
                return Err(invalid_operation("item insertion index is out of bounds"));
            }
            items.insert(index, item.clone());
        }
        Operation::RemoveItem { item_id } => {
            let (scene_index, track_index, item_index) = locate_item(document, item_id)
                .ok_or_else(|| entity_not_found("timeline item", item_id))?;
            document.scenes[scene_index].tracks[track_index]
                .items
                .remove(item_index);
        }
        Operation::MoveItem {
            item_id,
            target_track_id,
            target_index,
            start_ticks,
        } => {
            let (source_scene, source_track, source_item) = locate_item(document, item_id)
                .ok_or_else(|| entity_not_found("timeline item", item_id))?;
            let mut item = document.scenes[source_scene].tracks[source_track]
                .items
                .remove(source_item);
            let (target_scene, target_track) = locate_track(document, target_track_id)
                .ok_or_else(|| entity_not_found("target track", target_track_id))?;
            let items = &mut document.scenes[target_scene].tracks[target_track].items;
            if *target_index > items.len() {
                return Err(invalid_operation("target item index is out of bounds"));
            }
            item.start_ticks = *start_ticks;
            items.insert(*target_index, item);
        }
        Operation::ReplaceItem { item_id, item } => {
            if item.id != *item_id {
                return Err(invalid_operation(
                    "replacement timeline item must preserve its stable ID",
                ));
            }
            let (scene_index, track_index, item_index) = locate_item(document, item_id)
                .ok_or_else(|| entity_not_found("timeline item", item_id))?;
            document.scenes[scene_index].tracks[track_index].items[item_index] = item.clone();
        }
        Operation::TrimItem {
            item_id,
            start_ticks,
            duration_ticks,
            source_range,
        } => {
            let (scene_index, track_index, item_index) = locate_item(document, item_id)
                .ok_or_else(|| entity_not_found("timeline item", item_id))?;
            let item = &mut document.scenes[scene_index].tracks[track_index].items[item_index];
            item.start_ticks = *start_ticks;
            item.duration_ticks = *duration_ticks;
            item.source_range = *source_range;
        }
        Operation::SplitItem {
            item_id,
            split_at_ticks,
            new_item_id,
        } => {
            if locate_item(document, new_item_id).is_some() {
                return Err(DomainError::DuplicateEntity {
                    entity: "timeline item".into(),
                    id: new_item_id.to_string(),
                });
            }
            let (scene_index, track_index, item_index) = locate_item(document, item_id)
                .ok_or_else(|| entity_not_found("timeline item", item_id))?;
            let original =
                document.scenes[scene_index].tracks[track_index].items[item_index].clone();
            let end = original
                .end_ticks()
                .ok_or(DomainError::ArithmeticOverflow)?;
            if *split_at_ticks <= original.start_ticks || *split_at_ticks >= end {
                return Err(invalid_operation(
                    "split point must be strictly inside the item",
                ));
            }
            let left_duration = split_at_ticks
                .checked_sub(original.start_ticks)
                .ok_or(DomainError::ArithmeticOverflow)?;
            let right_duration = end
                .checked_sub(*split_at_ticks)
                .ok_or(DomainError::ArithmeticOverflow)?;
            let original_duration = original.duration_ticks;
            let mut left = original.clone();
            let mut right = original;
            left.duration_ticks = left_duration;
            right.id = new_item_id.clone();
            right.start_ticks = *split_at_ticks;
            right.duration_ticks = right_duration;
            if let Some(source_range) = left.source_range {
                let source_duration = source_range
                    .out_ticks
                    .checked_sub(source_range.in_ticks)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                if source_duration <= 0 || original_duration <= 0 {
                    return Err(invalid_operation("item source range is invalid"));
                }
                let proportional_left = (i128::from(source_duration) * i128::from(left_duration))
                    / i128::from(original_duration);
                let proportional_left = i64::try_from(proportional_left)
                    .map_err(|_| DomainError::ArithmeticOverflow)?;
                let source_split = source_range
                    .in_ticks
                    .checked_add(proportional_left)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                if source_split <= source_range.in_ticks || source_split >= source_range.out_ticks {
                    return Err(invalid_operation(
                        "item source range is too short for a proportional split",
                    ));
                }
                left.source_range = Some(crate::SourceRange {
                    in_ticks: source_range.in_ticks,
                    out_ticks: source_split,
                });
                right.source_range = Some(crate::SourceRange {
                    in_ticks: source_split,
                    out_ticks: source_range.out_ticks,
                });
            }
            let items = &mut document.scenes[scene_index].tracks[track_index].items;
            items[item_index] = left;
            items.insert(item_index + 1, right);
        }
        Operation::SetCaption { item_id, caption } => {
            let (scene_index, track_index, item_index) = locate_item(document, item_id)
                .ok_or_else(|| entity_not_found("caption item", item_id))?;
            let item = &mut document.scenes[scene_index].tracks[track_index].items[item_index];
            match &mut item.content {
                ItemContent::Caption { caption: current } => current.as_mut().clone_from(caption),
                _ => return Err(invalid_operation("target item is not a caption")),
            }
        }
        Operation::SetCaptionStyle { item_id, style } => {
            let (scene_index, track_index, item_index) = locate_item(document, item_id)
                .ok_or_else(|| entity_not_found("caption item", item_id))?;
            let item = &mut document.scenes[scene_index].tracks[track_index].items[item_index];
            match &mut item.content {
                ItemContent::Caption { caption } => caption.style.clone_from(style),
                _ => return Err(invalid_operation("target item is not a caption")),
            }
        }
        Operation::UpsertTranscript { transcript } => {
            if let Some(current) = document
                .transcripts
                .iter_mut()
                .find(|current| current.id == transcript.id)
            {
                current.clone_from(transcript);
            } else {
                document.transcripts.push(transcript.clone());
            }
        }
        Operation::RemoveTranscript { transcript_id } => {
            if let Some(item) = document
                .scenes
                .iter()
                .flat_map(|scene| &scene.tracks)
                .flat_map(|track| &track.items)
                .find(|item| {
                    matches!(
                        &item.content,
                        ItemContent::Caption { caption } if caption.transcript_id == *transcript_id
                    )
                })
            {
                return Err(DomainError::ReferentialIntegrity {
                    entity: "transcript".into(),
                    id: transcript_id.to_string(),
                    referenced_by: format!("caption item {}", item.id),
                });
            }
            if let Some(sequence) = document
                .story_sequences
                .iter()
                .find(|sequence| sequence.transcript_id == *transcript_id)
            {
                return Err(DomainError::ReferentialIntegrity {
                    entity: "transcript".into(),
                    id: transcript_id.to_string(),
                    referenced_by: format!("story sequence {}", sequence.id),
                });
            }
            let index = document
                .transcripts
                .iter()
                .position(|transcript| transcript.id == *transcript_id)
                .ok_or_else(|| entity_not_found("transcript", transcript_id))?;
            document.transcripts.remove(index);
        }
        Operation::SetTranscriptWordsDeleted {
            transcript_id,
            word_ids,
            deleted,
        } => {
            if word_ids.is_empty() {
                return Err(invalid_operation("wordIds must not be empty"));
            }
            let transcript = transcript_mut(document, transcript_id)?;
            let requested = word_ids
                .iter()
                .map(ToString::to_string)
                .collect::<HashSet<_>>();
            if requested.len() != word_ids.len() {
                return Err(invalid_operation("wordIds must not contain duplicates"));
            }
            for word_id in word_ids {
                let word = transcript
                    .words
                    .iter_mut()
                    .find(|word| word.id == *word_id)
                    .ok_or_else(|| entity_not_found("transcript word", word_id))?;
                word.deleted = *deleted;
            }
            if *deleted {
                materialize_story_word_deletions(document, transcript_id)?;
            }
        }
        Operation::DeleteTranscriptSegment {
            transcript_id,
            segment_id,
        } => {
            let transcript = transcript_mut(document, transcript_id)?;
            let word_ids = transcript
                .segments
                .iter()
                .find(|segment| segment.id == *segment_id)
                .ok_or_else(|| entity_not_found("transcript segment", segment_id))?
                .word_ids
                .clone();
            for word_id in word_ids {
                let word = transcript
                    .words
                    .iter_mut()
                    .find(|word| word.id == word_id)
                    .ok_or_else(|| entity_not_found("transcript segment word", &word_id))?;
                word.deleted = true;
            }
            materialize_story_word_deletions(document, transcript_id)?;
        }
        Operation::SetTranscriptDisplayText {
            transcript_id,
            word_id,
            display_text,
        } => {
            let transcript = transcript_mut(document, transcript_id)?;
            let word = transcript
                .words
                .iter_mut()
                .find(|word| word.id == *word_id)
                .ok_or_else(|| entity_not_found("transcript word", word_id))?;
            word.display_text.clone_from(display_text);
        }
        Operation::SetTranscriptSpeaker {
            transcript_id,
            word_ids,
            speaker_id,
        } => {
            if word_ids.is_empty() {
                return Err(invalid_operation("wordIds must not be empty"));
            }
            let transcript = transcript_mut(document, transcript_id)?;
            if let Some(speaker_id) = speaker_id
                && !transcript
                    .speakers
                    .iter()
                    .any(|speaker| speaker.id == *speaker_id)
            {
                return Err(entity_not_found("transcript speaker", speaker_id));
            }
            let requested = word_ids
                .iter()
                .map(ToString::to_string)
                .collect::<HashSet<_>>();
            if requested.len() != word_ids.len() {
                return Err(invalid_operation("wordIds must not contain duplicates"));
            }
            for word_id in word_ids {
                let word = transcript
                    .words
                    .iter_mut()
                    .find(|word| word.id == *word_id)
                    .ok_or_else(|| entity_not_found("transcript word", word_id))?;
                word.speaker_id.clone_from(speaker_id);
            }
        }
        Operation::SplitTranscriptSegment {
            transcript_id,
            segment_id,
            at_word_id,
            new_segment_id,
        } => {
            let transcript = transcript_mut(document, transcript_id)?;
            if transcript
                .segments
                .iter()
                .any(|segment| segment.id == *new_segment_id)
            {
                return Err(DomainError::DuplicateEntity {
                    entity: "transcript segment".into(),
                    id: new_segment_id.to_string(),
                });
            }
            let segment_index = transcript
                .segments
                .iter()
                .position(|segment| segment.id == *segment_id)
                .ok_or_else(|| entity_not_found("transcript segment", segment_id))?;
            let split_index = transcript.segments[segment_index]
                .word_ids
                .iter()
                .position(|word_id| word_id == at_word_id)
                .ok_or_else(|| entity_not_found("segment split word", at_word_id))?;
            if split_index == 0 {
                return Err(invalid_operation(
                    "segment split word must not be the first word",
                ));
            }
            let word_ids = transcript.segments[segment_index]
                .word_ids
                .split_off(split_index);
            let speaker_id = transcript.segments[segment_index].speaker_id.clone();
            transcript.segments.insert(
                segment_index + 1,
                crate::TranscriptSegment {
                    id: new_segment_id.clone(),
                    word_ids,
                    speaker_id,
                },
            );
            split_story_at_word(document, transcript_id, at_word_id)?;
        }
        Operation::MergeTranscriptSegments {
            transcript_id,
            first_segment_id,
            second_segment_id,
        } => {
            let transcript = transcript_mut(document, transcript_id)?;
            let first_index = transcript
                .segments
                .iter()
                .position(|segment| segment.id == *first_segment_id)
                .ok_or_else(|| entity_not_found("first transcript segment", first_segment_id))?;
            let second_index = transcript
                .segments
                .iter()
                .position(|segment| segment.id == *second_segment_id)
                .ok_or_else(|| entity_not_found("second transcript segment", second_segment_id))?;
            if second_index != first_index + 1 {
                return Err(invalid_operation(
                    "only adjacent transcript segments in forward order can be merged",
                ));
            }
            let second = transcript.segments.remove(second_index);
            transcript.segments[first_index]
                .word_ids
                .extend(second.word_ids);
        }
        Operation::ReorderTranscriptSegments {
            transcript_id,
            segment_ids,
        } => {
            let transcript = transcript_mut(document, transcript_id)?;
            let current = transcript
                .segments
                .iter()
                .map(|segment| segment.id.clone())
                .collect::<Vec<_>>();
            ensure_permutation(&current, segment_ids, "transcript segment")?;
            let mut by_id = transcript
                .segments
                .drain(..)
                .map(|segment| (segment.id.clone(), segment))
                .collect::<HashMap<_, _>>();
            let mut reordered = Vec::with_capacity(segment_ids.len());
            for id in segment_ids {
                reordered.push(
                    by_id
                        .remove(id)
                        .ok_or_else(|| entity_not_found("transcript segment", id))?,
                );
            }
            transcript.segments = reordered;
        }
        Operation::UpsertStorySequence { sequence } => {
            if let Some(current) = document
                .story_sequences
                .iter_mut()
                .find(|current| current.id == sequence.id)
            {
                current.clone_from(sequence);
            } else {
                document.story_sequences.push(sequence.clone());
            }
        }
        Operation::RemoveStorySequence { sequence_id } => {
            let index = document
                .story_sequences
                .iter()
                .position(|sequence| sequence.id == *sequence_id)
                .ok_or_else(|| entity_not_found("story sequence", sequence_id))?;
            document.story_sequences.remove(index);
        }
        Operation::ReorderStoryClips {
            sequence_id,
            clip_ids,
        } => {
            let sequence = story_sequence_mut(document, sequence_id)?;
            let current = sequence
                .clips
                .iter()
                .map(|clip| clip.id.clone())
                .collect::<Vec<_>>();
            ensure_permutation(&current, clip_ids, "story clip")?;
            let first_start = sequence
                .clips
                .first()
                .map_or(0, |clip| clip.timeline_start_ticks);
            let mut by_id = sequence
                .clips
                .drain(..)
                .map(|clip| (clip.id.clone(), clip))
                .collect::<HashMap<_, _>>();
            let mut reordered = Vec::<StoryClip>::with_capacity(clip_ids.len());
            for id in clip_ids {
                reordered.push(
                    by_id
                        .remove(id)
                        .ok_or_else(|| entity_not_found("story clip", id))?,
                );
            }
            let mut cursor = first_start;
            for clip in &mut reordered {
                clip.timeline_start_ticks = cursor;
                cursor = clip
                    .timeline_end_ticks()
                    .ok_or(DomainError::ArithmeticOverflow)?;
            }
            sequence.clips = reordered;
            reconcile_story_linked_items(document, sequence_id)?;
        }
        Operation::CloseStoryGaps {
            sequence_id,
            threshold_ticks,
            target_gap_ticks,
        } => {
            if *threshold_ticks < 0 || *target_gap_ticks < 0 || target_gap_ticks > threshold_ticks {
                return Err(invalid_operation(
                    "gap thresholds require 0 <= targetGapTicks <= thresholdTicks",
                ));
            }
            close_story_pauses(document, sequence_id, *threshold_ticks, *target_gap_ticks)?;
        }
    }
    Ok(())
}

fn change_summary(operation_index: usize, operation: &Operation) -> ChangeSummary {
    let (kind, action, entity_id) = match operation {
        Operation::ReplaceDocument { document } => (
            ChangeKind::Project,
            ChangeAction::Replace,
            Some(document.id.to_string()),
        ),
        Operation::ReplaceSceneGraph { .. } => {
            (ChangeKind::SceneGraph, ChangeAction::Replace, None)
        }
        Operation::SetProjectName { .. } | Operation::SetProjectSettings { .. } => {
            (ChangeKind::Project, ChangeAction::Update, None)
        }
        Operation::AddAsset { asset } => (
            ChangeKind::Asset,
            ChangeAction::Add,
            Some(asset.id.to_string()),
        ),
        Operation::UpsertAsset { asset } => (
            ChangeKind::Asset,
            ChangeAction::Update,
            Some(asset.id.to_string()),
        ),
        Operation::RemoveAsset { asset_id } => (
            ChangeKind::Asset,
            ChangeAction::Remove,
            Some(asset_id.to_string()),
        ),
        Operation::AddScene { scene, .. } => (
            ChangeKind::Scene,
            ChangeAction::Add,
            Some(scene.id.to_string()),
        ),
        Operation::RemoveScene { scene_id } => (
            ChangeKind::Scene,
            ChangeAction::Remove,
            Some(scene_id.to_string()),
        ),
        Operation::SetSceneName { scene_id, .. } => (
            ChangeKind::Scene,
            ChangeAction::Update,
            Some(scene_id.to_string()),
        ),
        Operation::AddTrack { track, .. } => (
            ChangeKind::Track,
            ChangeAction::Add,
            Some(track.id.to_string()),
        ),
        Operation::RemoveTrack { track_id } => (
            ChangeKind::Track,
            ChangeAction::Remove,
            Some(track_id.to_string()),
        ),
        Operation::SetTrackProperties { track_id, .. } => (
            ChangeKind::Track,
            ChangeAction::Update,
            Some(track_id.to_string()),
        ),
        Operation::InsertItem { item, .. } => (
            ChangeKind::TimelineItem,
            ChangeAction::Add,
            Some(item.id.to_string()),
        ),
        Operation::RemoveItem { item_id } => (
            ChangeKind::TimelineItem,
            ChangeAction::Remove,
            Some(item_id.to_string()),
        ),
        Operation::MoveItem { item_id, .. } => (
            ChangeKind::TimelineItem,
            ChangeAction::Move,
            Some(item_id.to_string()),
        ),
        Operation::ReplaceItem { item_id, .. } | Operation::TrimItem { item_id, .. } => (
            ChangeKind::TimelineItem,
            ChangeAction::Update,
            Some(item_id.to_string()),
        ),
        Operation::SplitItem { item_id, .. } => (
            ChangeKind::TimelineItem,
            ChangeAction::Split,
            Some(item_id.to_string()),
        ),
        Operation::SetCaption { item_id, .. } | Operation::SetCaptionStyle { item_id, .. } => (
            ChangeKind::Caption,
            ChangeAction::Update,
            Some(item_id.to_string()),
        ),
        Operation::UpsertTranscript { transcript } => (
            ChangeKind::Transcript,
            ChangeAction::Update,
            Some(transcript.id.to_string()),
        ),
        Operation::RemoveTranscript { transcript_id }
        | Operation::SetTranscriptWordsDeleted { transcript_id, .. }
        | Operation::DeleteTranscriptSegment { transcript_id, .. }
        | Operation::SetTranscriptDisplayText { transcript_id, .. }
        | Operation::SetTranscriptSpeaker { transcript_id, .. }
        | Operation::SplitTranscriptSegment { transcript_id, .. }
        | Operation::MergeTranscriptSegments { transcript_id, .. }
        | Operation::ReorderTranscriptSegments { transcript_id, .. } => (
            ChangeKind::Transcript,
            if matches!(operation, Operation::RemoveTranscript { .. }) {
                ChangeAction::Remove
            } else if matches!(operation, Operation::SplitTranscriptSegment { .. }) {
                ChangeAction::Split
            } else if matches!(operation, Operation::MergeTranscriptSegments { .. }) {
                ChangeAction::Merge
            } else if matches!(operation, Operation::ReorderTranscriptSegments { .. }) {
                ChangeAction::Reorder
            } else {
                ChangeAction::Update
            },
            Some(transcript_id.to_string()),
        ),
        Operation::UpsertStorySequence { sequence } => (
            ChangeKind::StorySequence,
            ChangeAction::Update,
            Some(sequence.id.to_string()),
        ),
        Operation::RemoveStorySequence { sequence_id }
        | Operation::ReorderStoryClips { sequence_id, .. }
        | Operation::CloseStoryGaps { sequence_id, .. } => (
            ChangeKind::StorySequence,
            if matches!(operation, Operation::RemoveStorySequence { .. }) {
                ChangeAction::Remove
            } else if matches!(operation, Operation::ReorderStoryClips { .. }) {
                ChangeAction::Reorder
            } else {
                ChangeAction::Update
            },
            Some(sequence_id.to_string()),
        ),
    };
    ChangeSummary {
        operation_index,
        kind,
        action,
        entity_id,
    }
}

pub fn apply_operations(
    document: &ProjectDocument,
    operations: &[Operation],
) -> Result<OperationsOutcome, DomainError> {
    if operations.is_empty() {
        return Err(DomainError::InvalidTransaction {
            field: "operations".into(),
            message: "must contain at least one operation".into(),
        });
    }
    if operations.len() > MAX_OPERATIONS_PER_TRANSACTION {
        return Err(DomainError::InvalidTransaction {
            field: "operations".into(),
            message: format!("must contain at most {MAX_OPERATIONS_PER_TRANSACTION} operations"),
        });
    }
    validate_document(document)?;
    let original = document.clone();
    let mut next = document.clone();
    let mut changes = Vec::with_capacity(operations.len());
    let mut changed_transcripts = HashSet::new();
    for (operation_index, operation) in operations.iter().enumerate() {
        let story_transcript_id = story_operation_transcript_id(&next, operation);
        apply_operation(&mut next, operation).map_err(|cause| DomainError::OperationFailed {
            operation_index,
            cause: Box::new(cause),
        })?;
        if let Some(transcript_id) = operation_transcript_id(operation) {
            changed_transcripts.insert(transcript_id.clone());
        }
        if let Some(transcript_id) = story_transcript_id {
            changed_transcripts.insert(transcript_id);
        }
        changes.push(change_summary(operation_index, operation));
    }
    for transcript_id in changed_transcripts {
        reconcile_captions_for_transcript(&mut next, &transcript_id)?;
        reconcile_timeline_anchors_for_transcript(&mut next, &transcript_id)?;
    }
    validate_document(&next)?;
    Ok(OperationsOutcome {
        document: next,
        // A single snapshot inverse is intentionally used for a transaction: it is exact,
        // handles dependent multi-operation batches, and remains a semantic operation.
        inverse_operations: vec![Operation::ReplaceDocument {
            document: Box::new(original),
        }],
        changes,
    })
}

fn validate_transaction_input(
    envelope: &ProjectEnvelope,
    transaction: &EditTransaction,
) -> Result<(), DomainError> {
    envelope.verify()?;
    if transaction.project_id != envelope.document.id {
        return Err(DomainError::ProjectMismatch {
            expected_project_id: envelope.document.id.clone(),
            actual_project_id: transaction.project_id.clone(),
        });
    }
    if transaction.base_revision != envelope.revision {
        return Err(DomainError::RevisionConflict {
            expected_revision: transaction.base_revision,
            actual_revision: envelope.revision,
        });
    }
    match transaction.actor.kind {
        ActorKind::User | ActorKind::Agent if transaction.actor.id.is_none() => {
            return Err(DomainError::InvalidTransaction {
                field: "actor.id".into(),
                message: "user and agent actors require a stable ID".into(),
            });
        }
        ActorKind::System if transaction.actor.id.is_some() => {
            return Err(DomainError::InvalidTransaction {
                field: "actor.id".into(),
                message: "system actors must not impersonate a user or agent ID".into(),
            });
        }
        _ => {}
    }
    if transaction
        .actor
        .display_name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(DomainError::InvalidTransaction {
            field: "actor.displayName".into(),
            message: "must not be blank when provided".into(),
        });
    }
    Ok(())
}

fn warnings_for(operations: &[Operation]) -> Vec<ValidationWarning> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::ReplaceDocument { .. } => Some(ValidationWarning {
                code: "wholeDocumentReplacement".into(),
                message: "The complete project document will be replaced atomically.".into(),
            }),
            Operation::ReplaceSceneGraph { .. } => Some(ValidationWarning {
                code: "wholeSceneGraphReplacement".into(),
                message: "All scenes and the current-scene pointer will be replaced atomically."
                    .into(),
            }),
            _ => None,
        })
        .collect()
}

pub fn validate_transaction(
    envelope: &ProjectEnvelope,
    transaction: &EditTransaction,
) -> Result<ValidationReport, DomainError> {
    validate_transaction_input(envelope, transaction)?;
    let outcome = apply_operations(&envelope.document, &transaction.operations)?;
    let next_revision = envelope
        .revision
        .checked_add(1)
        .ok_or(DomainError::ArithmeticOverflow)?;
    Ok(ValidationReport {
        base_revision: envelope.revision,
        next_revision,
        current_document_hash: envelope.document_hash.clone(),
        resulting_document_hash: canonical_document_hash(&outcome.document)?,
        transaction_fingerprint: transaction_fingerprint(transaction)?,
        operation_count: transaction.operations.len(),
        changes: outcome.changes,
        warnings: warnings_for(&transaction.operations),
    })
}

pub fn apply_transaction(
    envelope: &ProjectEnvelope,
    transaction: &EditTransaction,
) -> Result<ApplyOutcome, DomainError> {
    validate_transaction_input(envelope, transaction)?;
    let outcome = apply_operations(&envelope.document, &transaction.operations)?;
    let document_hash = canonical_document_hash(&outcome.document)?;
    let revision = envelope
        .revision
        .checked_add(1)
        .ok_or(DomainError::ArithmeticOverflow)?;
    Ok(ApplyOutcome {
        envelope: ProjectEnvelope {
            document: outcome.document,
            revision,
            document_hash,
        },
        inverse_operations: outcome.inverse_operations,
        changes: outcome.changes,
        transaction_fingerprint: transaction_fingerprint(transaction)?,
    })
}
