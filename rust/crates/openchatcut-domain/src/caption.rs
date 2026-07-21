use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{CaptionStyle, CaptionTextAlign, DomainError, ProjectDocument, TranscriptId, WordId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionPresetDescriptor {
    pub id: String,
    pub name: String,
    pub max_lines: u32,
    pub max_characters_per_line: u32,
    pub word_highlight: bool,
    pub style: CaptionStyle,
}

#[allow(clippy::too_many_arguments)]
fn preset(
    id: &str,
    name: &str,
    font_family: &str,
    font_size: f32,
    text_color: &str,
    active_word_color: &str,
    background_color: &str,
    text_align: CaptionTextAlign,
    max_lines: u32,
    max_characters_per_line: u32,
    word_highlight: bool,
) -> CaptionPresetDescriptor {
    CaptionPresetDescriptor {
        id: id.to_owned(),
        name: name.to_owned(),
        max_lines,
        max_characters_per_line,
        word_highlight,
        style: CaptionStyle {
            font_family: font_family.to_owned(),
            font_size,
            text_color: text_color.to_owned(),
            active_word_color: active_word_color.to_owned(),
            background_color: background_color.to_owned(),
            text_align,
            ..CaptionStyle::default()
        },
    }
}

/// Twelve independently designed local presets mirrored by each replaceable UI
/// shell. The identifiers and domain style values are authoritative here.
pub fn builtin_caption_presets() -> Vec<CaptionPresetDescriptor> {
    vec![
        preset(
            "studio-clean",
            "Studio Clean",
            "Inter",
            64.0,
            "#ffffff",
            "#67e8f9",
            "#00000000",
            CaptionTextAlign::Center,
            2,
            28,
            true,
        ),
        preset(
            "ink-card",
            "Ink Card",
            "Georgia",
            60.0,
            "#fff7df",
            "#fbbf24",
            "#171512e8",
            CaptionTextAlign::Center,
            2,
            30,
            true,
        ),
        preset(
            "signal-yellow",
            "Signal Yellow",
            "Arial",
            72.0,
            "#ffffff",
            "#fde047",
            "#00000000",
            CaptionTextAlign::Center,
            2,
            22,
            true,
        ),
        preset(
            "editorial-serif",
            "Editorial Serif",
            "Georgia",
            62.0,
            "#fffaf0",
            "#fb7185",
            "#00000000",
            CaptionTextAlign::Center,
            2,
            32,
            true,
        ),
        preset(
            "electric-blue",
            "Electric Blue",
            "Inter",
            60.0,
            "#dbeafe",
            "#22d3ee",
            "#071a36d9",
            CaptionTextAlign::Center,
            2,
            28,
            true,
        ),
        preset(
            "mono-terminal",
            "Mono Terminal",
            "Courier New",
            56.0,
            "#d1fae5",
            "#34d399",
            "#04130ee6",
            CaptionTextAlign::Start,
            3,
            34,
            true,
        ),
        preset(
            "paper-label",
            "Paper Label",
            "Arial",
            60.0,
            "#18181b",
            "#dc2626",
            "#f5f1e8f2",
            CaptionTextAlign::Center,
            2,
            29,
            true,
        ),
        preset(
            "neon-magenta",
            "Neon Magenta",
            "Arial",
            66.0,
            "#ffffff",
            "#f472b6",
            "#19051bcc",
            CaptionTextAlign::Center,
            2,
            25,
            true,
        ),
        preset(
            "documentary",
            "Documentary",
            "Arial",
            52.0,
            "#ffffff",
            "#ffffff",
            "#000000b8",
            CaptionTextAlign::Start,
            2,
            38,
            false,
        ),
        preset(
            "sports-score",
            "Sports Score",
            "Arial",
            76.0,
            "#ffffff",
            "#a3e635",
            "#00000000",
            CaptionTextAlign::Center,
            2,
            20,
            true,
        ),
        preset(
            "soft-lavender",
            "Soft Lavender",
            "Inter",
            60.0,
            "#faf5ff",
            "#d8b4fe",
            "#3b1e4dcc",
            CaptionTextAlign::Center,
            2,
            28,
            true,
        ),
        preset(
            "cjk-focus",
            "CJK Focus",
            "Noto Sans CJK SC",
            68.0,
            "#ffffff",
            "#fca5a5",
            "#111827d9",
            CaptionTextAlign::Center,
            2,
            16,
            true,
        ),
    ]
}

pub fn builtin_caption_preset(id: &str) -> Option<CaptionPresetDescriptor> {
    builtin_caption_presets()
        .into_iter()
        .find(|preset| preset.id == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionWordTimelineRange {
    pub start_ticks: i64,
    pub end_ticks: i64,
}

/// Resolve every active transcript word to its current materialized timeline
/// range. StorySequence placement wins over immutable source time.
pub fn active_caption_word_ranges(
    document: &ProjectDocument,
    transcript_id: &TranscriptId,
) -> Result<HashMap<WordId, CaptionWordTimelineRange>, DomainError> {
    let transcript = document
        .transcripts
        .iter()
        .find(|transcript| transcript.id == *transcript_id)
        .ok_or_else(|| DomainError::EntityNotFound {
            entity: "caption transcript".into(),
            id: transcript_id.to_string(),
        })?;
    let source_words = transcript
        .words
        .iter()
        .map(|word| (word.id.clone(), word))
        .collect::<HashMap<_, _>>();
    let mut mapped = HashMap::new();
    if let Some(sequence) = document
        .story_sequences
        .iter()
        .find(|sequence| sequence.transcript_id == *transcript_id)
    {
        for clip in &sequence.clips {
            for word_id in &clip.word_ids {
                let Some(word) = source_words.get(word_id) else {
                    continue;
                };
                if word.deleted {
                    continue;
                }
                let start_offset = word
                    .start_ticks
                    .checked_sub(clip.source_start_ticks)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                let end_offset = word
                    .end_ticks
                    .checked_sub(clip.source_start_ticks)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                let start_ticks = clip
                    .timeline_start_ticks
                    .checked_add(start_offset)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                let end_ticks = clip
                    .timeline_start_ticks
                    .checked_add(end_offset)
                    .ok_or(DomainError::ArithmeticOverflow)?;
                if end_ticks > start_ticks {
                    mapped
                        .entry(word_id.clone())
                        .or_insert(CaptionWordTimelineRange {
                            start_ticks,
                            end_ticks,
                        });
                }
            }
        }
    } else {
        for word in &transcript.words {
            if !word.deleted && word.end_ticks > word.start_ticks {
                mapped.insert(
                    word.id.clone(),
                    CaptionWordTimelineRange {
                        start_ticks: word.start_ticks,
                        end_ticks: word.end_ticks,
                    },
                );
            }
        }
    }
    Ok(mapped)
}

pub fn caption_timeline_range(
    document: &ProjectDocument,
    transcript_id: &TranscriptId,
    word_ids: &[WordId],
) -> Result<Option<CaptionWordTimelineRange>, DomainError> {
    let ranges = active_caption_word_ranges(document, transcript_id)?;
    let mut start_ticks = i64::MAX;
    let mut end_ticks = i64::MIN;
    for word_id in word_ids {
        let range = ranges
            .get(word_id)
            .ok_or_else(|| DomainError::EntityNotFound {
                entity: "active caption word".into(),
                id: word_id.to_string(),
            })?;
        start_ticks = start_ticks.min(range.start_ticks);
        end_ticks = end_ticks.max(range.end_ticks);
    }
    Ok(
        (start_ticks != i64::MAX && end_ticks > start_ticks).then_some(CaptionWordTimelineRange {
            start_ticks,
            end_ticks,
        }),
    )
}
