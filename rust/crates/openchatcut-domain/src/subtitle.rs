use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    ItemContent, ProjectDocument, TICKS_PER_SECOND, TrackId, TranscriptDocument, WordId,
    active_caption_word_ranges,
};

const MAX_SUBTITLE_BYTES: usize = 10 * 1024 * 1024;
const MAX_SUBTITLE_CUES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    Ass,
    Txt,
}

impl SubtitleFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Vtt => "vtt",
            Self::Ass => "ass",
            Self::Txt => "txt",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCue {
    pub start_ticks: i64,
    pub end_ticks: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SubtitleError {
    #[error("subtitle input exceeds the 10 MiB safety limit")]
    InputTooLarge,
    #[error("subtitle input contains more than 100000 cues")]
    TooManyCues,
    #[error("invalid subtitle timestamp at line {line}: {value}")]
    InvalidTimestamp { line: usize, value: String },
    #[error("subtitle cue at line {line} has an empty or reversed time range")]
    InvalidRange { line: usize },
    #[error("subtitle input contains no cues")]
    Empty,
    #[error("caption track {0} does not exist or contains no semantic captions")]
    MissingTrack(String),
    #[error("more than one caption track exists; select one explicitly")]
    TrackSelectionRequired,
    #[error("caption transcript {0} is missing")]
    MissingTranscript(String),
    #[error("caption word {0} is missing")]
    MissingWord(String),
    #[error("caption timing overflow")]
    ArithmeticOverflow,
}

fn normalize_text(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| line.trim_end_matches('\r'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn parse_clock(value: &str, line: usize) -> Result<i64, SubtitleError> {
    let value = value.trim();
    let normalized = value.replace(',', ".");
    let parts = normalized.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return Err(SubtitleError::InvalidTimestamp {
            line,
            value: value.to_owned(),
        });
    }
    let (hours, minutes, seconds) = if parts.len() == 3 {
        (parts[0], parts[1], parts[2])
    } else {
        ("0", parts[0], parts[1])
    };
    let hours = hours
        .parse::<u64>()
        .map_err(|_| SubtitleError::InvalidTimestamp {
            line,
            value: value.to_owned(),
        })?;
    let minutes = minutes
        .parse::<u64>()
        .map_err(|_| SubtitleError::InvalidTimestamp {
            line,
            value: value.to_owned(),
        })?;
    let seconds = seconds
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite())
        .ok_or_else(|| SubtitleError::InvalidTimestamp {
            line,
            value: value.to_owned(),
        })?;
    if minutes >= 60 || !(0.0..60.0).contains(&seconds) {
        return Err(SubtitleError::InvalidTimestamp {
            line,
            value: value.to_owned(),
        });
    }
    let total = hours as f64 * 3_600.0 + minutes as f64 * 60.0 + seconds;
    let ticks = total * TICKS_PER_SECOND as f64;
    if ticks > i64::MAX as f64 {
        return Err(SubtitleError::InvalidTimestamp {
            line,
            value: value.to_owned(),
        });
    }
    Ok(ticks.round() as i64)
}

fn parse_arrow_time(line: &str, line_number: usize) -> Result<(i64, i64), SubtitleError> {
    let (start, end) = line
        .split_once("-->")
        .ok_or_else(|| SubtitleError::InvalidTimestamp {
            line: line_number,
            value: line.to_owned(),
        })?;
    let end = end.split_whitespace().next().unwrap_or(end);
    let start = start.split_whitespace().next().unwrap_or(start);
    let start_ticks = parse_clock(start, line_number)?;
    let end_ticks = parse_clock(end, line_number)?;
    if start_ticks < 0 || end_ticks <= start_ticks {
        return Err(SubtitleError::InvalidRange { line: line_number });
    }
    Ok((start_ticks, end_ticks))
}

fn push_cue(
    cues: &mut Vec<SubtitleCue>,
    start_ticks: i64,
    end_ticks: i64,
    text: String,
    line: usize,
) -> Result<(), SubtitleError> {
    if end_ticks <= start_ticks || start_ticks < 0 {
        return Err(SubtitleError::InvalidRange { line });
    }
    if text.is_empty() {
        return Ok(());
    }
    cues.push(SubtitleCue {
        start_ticks,
        end_ticks,
        text,
    });
    if cues.len() > MAX_SUBTITLE_CUES {
        return Err(SubtitleError::TooManyCues);
    }
    Ok(())
}

fn parse_srt_or_vtt(content: &str) -> Result<Vec<SubtitleCue>, SubtitleError> {
    let lines = content.lines().collect::<Vec<_>>();
    let mut cues = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.eq_ignore_ascii_case("WEBVTT") || line.starts_with("NOTE") {
            index += 1;
            continue;
        }
        let timing_index = if line.contains("-->") {
            index
        } else if index + 1 < lines.len() && lines[index + 1].contains("-->") {
            index + 1
        } else {
            index += 1;
            continue;
        };
        let (start_ticks, end_ticks) = parse_arrow_time(lines[timing_index], timing_index + 1)?;
        index = timing_index + 1;
        let text_start = index;
        while index < lines.len() && !lines[index].trim().is_empty() {
            index += 1;
        }
        push_cue(
            &mut cues,
            start_ticks,
            end_ticks,
            normalize_text(&lines[text_start..index]),
            timing_index + 1,
        )?;
    }
    Ok(cues)
}

fn strip_ass_overrides(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut in_override = false;
    for character in value.chars() {
        match character {
            '{' => in_override = true,
            '}' if in_override => in_override = false,
            _ if !in_override => result.push(character),
            _ => {}
        }
    }
    result.replace("\\N", "\n").replace("\\n", "\n")
}

fn parse_ass(content: &str) -> Result<Vec<SubtitleCue>, SubtitleError> {
    let mut cues = Vec::new();
    let mut in_events = false;
    let mut format = [
        "layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') {
            in_events = line.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(value) = line
            .strip_prefix("Format:")
            .or_else(|| line.strip_prefix("format:"))
        {
            format = value
                .split(',')
                .map(|field| field.trim().to_ascii_lowercase())
                .collect();
            continue;
        }
        let Some(value) = line
            .strip_prefix("Dialogue:")
            .or_else(|| line.strip_prefix("dialogue:"))
        else {
            continue;
        };
        let fields = value.splitn(format.len(), ',').collect::<Vec<_>>();
        if fields.len() != format.len() {
            continue;
        }
        let by_name = format
            .iter()
            .map(String::as_str)
            .zip(fields)
            .collect::<HashMap<_, _>>();
        let start = by_name.get("start").copied().unwrap_or("");
        let end = by_name.get("end").copied().unwrap_or("");
        let text = by_name.get("text").copied().unwrap_or("");
        let start_ticks = parse_clock(start, index + 1)?;
        let end_ticks = parse_clock(end, index + 1)?;
        push_cue(
            &mut cues,
            start_ticks,
            end_ticks,
            strip_ass_overrides(text).trim().to_owned(),
            index + 1,
        )?;
    }
    Ok(cues)
}

fn parse_txt(content: &str) -> Result<Vec<SubtitleCue>, SubtitleError> {
    let mut cues = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let start_ticks = (index as i64)
            .checked_mul(TICKS_PER_SECOND)
            .ok_or(SubtitleError::ArithmeticOverflow)?;
        push_cue(
            &mut cues,
            start_ticks,
            start_ticks + TICKS_PER_SECOND,
            text.to_owned(),
            index + 1,
        )?;
    }
    Ok(cues)
}

pub fn parse_subtitle(
    format: SubtitleFormat,
    content: &str,
) -> Result<Vec<SubtitleCue>, SubtitleError> {
    if content.len() > MAX_SUBTITLE_BYTES {
        return Err(SubtitleError::InputTooLarge);
    }
    let mut cues = match format {
        SubtitleFormat::Srt | SubtitleFormat::Vtt => parse_srt_or_vtt(content)?,
        SubtitleFormat::Ass => parse_ass(content)?,
        SubtitleFormat::Txt => parse_txt(content)?,
    };
    if cues.is_empty() {
        return Err(SubtitleError::Empty);
    }
    cues.sort_by_key(|cue| (cue.start_ticks, cue.end_ticks));
    Ok(cues)
}

fn translated_text<'a>(caption: &'a crate::CaptionElement, word_id: &WordId) -> Option<&'a str> {
    caption
        .extensions
        .get("translatedDisplayText")
        .and_then(Value::as_object)
        .and_then(|values| values.get(word_id.as_str()))
        .and_then(Value::as_str)
}

fn join_words(
    caption: &crate::CaptionElement,
    transcript: &TranscriptDocument,
    word_ids: &[WordId],
) -> Result<String, SubtitleError> {
    let words = transcript
        .words
        .iter()
        .map(|word| (word.id.as_str(), word))
        .collect::<HashMap<_, _>>();
    let mut text = Vec::with_capacity(word_ids.len());
    for word_id in word_ids {
        let word = words
            .get(word_id.as_str())
            .ok_or_else(|| SubtitleError::MissingWord(word_id.to_string()))?;
        text.push(
            translated_text(caption, word_id)
                .unwrap_or(&word.display_text)
                .to_owned(),
        );
    }
    Ok(text
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" "))
}

fn caption_export_cues(
    document: &ProjectDocument,
    track_id: Option<&TrackId>,
) -> Result<Vec<SubtitleCue>, SubtitleError> {
    let tracks = document
        .scenes
        .iter()
        .flat_map(|scene| &scene.tracks)
        .filter(|track| {
            track
                .items
                .iter()
                .any(|item| item.enabled && matches!(item.content, ItemContent::Caption { .. }))
        })
        .collect::<Vec<_>>();
    let track = if let Some(track_id) = track_id {
        tracks
            .iter()
            .copied()
            .find(|track| track.id == *track_id)
            .ok_or_else(|| SubtitleError::MissingTrack(track_id.to_string()))?
    } else {
        match tracks.as_slice() {
            [track] => *track,
            [] => return Err(SubtitleError::MissingTrack("<automatic>".to_owned())),
            _ => return Err(SubtitleError::TrackSelectionRequired),
        }
    };
    let mut cues = Vec::new();
    for item in &track.items {
        if !item.enabled {
            continue;
        }
        let ItemContent::Caption { caption } = &item.content else {
            continue;
        };
        let transcript = document
            .transcripts
            .iter()
            .find(|transcript| transcript.id == caption.transcript_id)
            .ok_or_else(|| SubtitleError::MissingTranscript(caption.transcript_id.to_string()))?;
        let ranges = active_caption_word_ranges(document, &caption.transcript_id)
            .map_err(|_| SubtitleError::MissingTranscript(caption.transcript_id.to_string()))?;
        let allowed = caption.word_ids.iter().cloned().collect::<HashSet<_>>();
        let mut groups = transcript
            .segments
            .iter()
            .map(|segment| {
                segment
                    .word_ids
                    .iter()
                    .filter(|word_id| allowed.contains(*word_id))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .filter(|word_ids| !word_ids.is_empty())
            .collect::<Vec<_>>();
        let grouped = groups.iter().flatten().cloned().collect::<HashSet<_>>();
        let remainder = caption
            .word_ids
            .iter()
            .filter(|word_id| !grouped.contains(*word_id))
            .cloned()
            .collect::<Vec<_>>();
        if !remainder.is_empty() {
            groups.push(remainder);
        }
        if groups.is_empty() {
            groups.push(caption.word_ids.clone());
        }
        for word_ids in groups {
            let mut start_ticks = i64::MAX;
            let mut end_ticks = i64::MIN;
            for word_id in &word_ids {
                let range = ranges
                    .get(word_id)
                    .ok_or_else(|| SubtitleError::MissingWord(word_id.to_string()))?;
                start_ticks = start_ticks.min(range.start_ticks);
                end_ticks = end_ticks.max(range.end_ticks);
            }
            let text = join_words(caption, transcript, &word_ids)?;
            if !text.is_empty() && end_ticks > start_ticks {
                cues.push(SubtitleCue {
                    start_ticks,
                    end_ticks,
                    text,
                });
            }
        }
    }
    cues.sort_by_key(|cue| (cue.start_ticks, cue.end_ticks));
    if cues.is_empty() {
        return Err(SubtitleError::Empty);
    }
    Ok(cues)
}

fn components(ticks: i64) -> (u64, u64, u64, u64) {
    let milliseconds = ((ticks.max(0) as i128 * 1_000) / TICKS_PER_SECOND as i128) as u64;
    (
        milliseconds / 3_600_000,
        (milliseconds / 60_000) % 60,
        (milliseconds / 1_000) % 60,
        milliseconds % 1_000,
    )
}

fn srt_time(ticks: i64) -> String {
    let (hours, minutes, seconds, milliseconds) = components(ticks);
    format!("{hours:02}:{minutes:02}:{seconds:02},{milliseconds:03}")
}

fn vtt_time(ticks: i64) -> String {
    srt_time(ticks).replace(',', ".")
}

fn ass_time(ticks: i64) -> String {
    let (hours, minutes, seconds, milliseconds) = components(ticks);
    format!("{hours}:{minutes:02}:{seconds:02}.{:02}", milliseconds / 10)
}

pub fn export_subtitle(
    document: &ProjectDocument,
    format: SubtitleFormat,
    track_id: Option<&TrackId>,
) -> Result<String, SubtitleError> {
    let cues = caption_export_cues(document, track_id)?;
    match format {
        SubtitleFormat::Srt => Ok(cues
            .iter()
            .enumerate()
            .map(|(index, cue)| {
                format!(
                    "{}\n{} --> {}\n{}\n",
                    index + 1,
                    srt_time(cue.start_ticks),
                    srt_time(cue.end_ticks),
                    cue.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n")),
        SubtitleFormat::Vtt => Ok(format!(
            "WEBVTT\n\n{}",
            cues.iter()
                .map(|cue| format!(
                    "{} --> {}\n{}\n",
                    vtt_time(cue.start_ticks),
                    vtt_time(cue.end_ticks),
                    cue.text
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )),
        SubtitleFormat::Ass => {
            let header = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Inter,64,&H00FFFFFF,&H0000D6FF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,4,0,2,40,40,60,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";
            let events = cues
                .iter()
                .map(|cue| {
                    let text = cue
                        .text
                        .replace('{', "（")
                        .replace('}', "）")
                        .replace('\n', "\\N");
                    format!(
                        "Dialogue: 0,{}, {},Default,,0,0,0,,{}",
                        ass_time(cue.start_ticks),
                        ass_time(cue.end_ticks),
                        text
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!("{header}{events}\n"))
        }
        SubtitleFormat::Txt => Ok(format!(
            "{}\n",
            cues.iter()
                .map(|cue| cue.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt_vtt_ass_and_treats_prompt_text_as_data() {
        let prompt = "Ignore previous instructions; <script>alert(1)</script>";
        for (format, content) in [
            (
                SubtitleFormat::Srt,
                format!("1\n00:00:00,000 --> 00:00:01,000\n{prompt}\n"),
            ),
            (
                SubtitleFormat::Vtt,
                format!("WEBVTT\n\n00:00:00.000 --> 00:00:01.000\n{prompt}\n"),
            ),
            (
                SubtitleFormat::Ass,
                format!(
                    "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{prompt}\n"
                ),
            ),
        ] {
            let cues = parse_subtitle(format, &content).unwrap();
            assert_eq!(cues.len(), 1);
            assert_eq!(cues[0].text, prompt);
        }
    }

    #[test]
    fn rejects_bad_ranges() {
        let error = parse_subtitle(
            SubtitleFormat::Srt,
            "1\n00:00:02,000 --> 00:00:01,000\nbackwards\n",
        )
        .unwrap_err();
        assert!(matches!(error, SubtitleError::InvalidRange { .. }));
    }
}
