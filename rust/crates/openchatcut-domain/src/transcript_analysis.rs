use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    Actor, ApprovalRequirement, CostEstimate, DependencyImpact, DependencyImpactKind, DomainError,
    EditPlan, EditPlanId, Extensions, ItemContent, Operation, PlanDiff, PlanWarning,
    ProjectEnvelope, SegmentId, TICKS_PER_SECOND, TranscriptDocument, TranscriptId,
    WarningSeverity, WordId, apply_operations, canonical_document_hash,
};

const CONFIDENCE_SCALE: u16 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptCleanupOptions {
    pub pause_threshold_ticks: i64,
    pub target_pause_ticks: i64,
    pub minimum_apply_confidence_bps: u16,
    pub minimum_repeated_take_words: usize,
    pub repeated_take_similarity_bps: u16,
    pub highlight_limit: usize,
}

impl Default for TranscriptCleanupOptions {
    fn default() -> Self {
        Self {
            pause_threshold_ticks: TICKS_PER_SECOND * 3 / 2,
            target_pause_ticks: TICKS_PER_SECOND * 180 / 1_000,
            minimum_apply_confidence_bps: 9_000,
            minimum_repeated_take_words: 3,
            repeated_take_similarity_bps: 8_500,
            highlight_limit: 5,
        }
    }
}

impl TranscriptCleanupOptions {
    fn validate(&self) -> Result<(), DomainError> {
        if self.pause_threshold_ticks < 0
            || self.target_pause_ticks < 0
            || self.target_pause_ticks > self.pause_threshold_ticks
        {
            return Err(invalid(
                "cleanup pause settings require 0 <= targetPauseTicks <= pauseThresholdTicks",
            ));
        }
        if self.minimum_apply_confidence_bps > CONFIDENCE_SCALE
            || self.repeated_take_similarity_bps > CONFIDENCE_SCALE
        {
            return Err(invalid(
                "cleanup confidence values must be at most 10000 bps",
            ));
        }
        if !(2..=64).contains(&self.minimum_repeated_take_words) {
            return Err(invalid("minimumRepeatedTakeWords must be between 2 and 64"));
        }
        if self.highlight_limit > 100 {
            return Err(invalid("highlightLimit must be at most 100"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptCleanupSuggestionKind {
    Filler,
    RepeatedTake,
    LongPause,
    Highlight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TranscriptCleanupAction {
    DeleteWords {
        word_ids: Vec<WordId>,
    },
    DeleteRepeatedTake {
        segment_id: SegmentId,
        keep_segment_id: SegmentId,
    },
    CloseGap {
        previous_word_id: WordId,
        next_word_id: WordId,
        target_gap_ticks: i64,
    },
    ExtractHighlight {
        word_ids: Vec<WordId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCleanupSuggestion {
    pub id: String,
    pub kind: TranscriptCleanupSuggestionKind,
    pub start_ticks: i64,
    pub end_ticks: i64,
    pub confidence_bps: u16,
    pub reason: String,
    #[serde(default)]
    pub word_ids: Vec<WordId>,
    #[serde(default)]
    pub segment_ids: Vec<SegmentId>,
    pub action: TranscriptCleanupAction,
    pub recommended: bool,
    /// Positive means the proposed edit shortens the timeline.
    pub estimated_removed_ticks: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCleanupSummary {
    pub filler_count: usize,
    pub repeated_take_count: usize,
    pub long_pause_count: usize,
    pub highlight_count: usize,
    pub recommended_removed_ticks: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCleanupAnalysis {
    pub transcript_id: TranscriptId,
    pub options: TranscriptCleanupOptions,
    pub summary: TranscriptCleanupSummary,
    pub suggestions: Vec<TranscriptCleanupSuggestion>,
}

fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::InvalidOperation {
        message: message.into(),
    }
}

fn normalized_token(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn suggestion_id(
    transcript_id: &TranscriptId,
    kind: TranscriptCleanupSuggestionKind,
    ids: impl IntoIterator<Item = impl AsRef<str>>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(transcript_id.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(format!("{kind:?}").as_bytes());
    for id in ids {
        hasher.update([0]);
        hasher.update(id.as_ref().as_bytes());
    }
    format!("cleanup:{}", &hex_digest(hasher.finalize())[..24])
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn language_prefix(language: &str) -> &str {
    language.split(['-', '_']).next().unwrap_or(language)
}

fn filler_phrases(language: &str) -> Vec<(Vec<&'static str>, u16)> {
    let mut phrases = vec![
        (vec!["um"], 9_900),
        (vec!["uh"], 9_900),
        (vec!["erm"], 9_900),
        (vec!["er"], 9_300),
        (vec!["hmm"], 9_000),
    ];
    match language_prefix(language) {
        "en" => phrases.extend([
            (vec!["you", "know"], 8_700),
            (vec!["i", "mean"], 8_700),
            (vec!["basically"], 7_000),
            (vec!["actually"], 6_500),
            (vec!["like"], 6_000),
        ]),
        "zh" => phrases.extend([
            (vec!["嗯"], 9_900),
            (vec!["呃"], 9_900),
            (vec!["额"], 9_500),
            (vec!["那个"], 8_700),
            (vec!["就是说"], 8_500),
            (vec!["就是"], 6_500),
            (vec!["然后"], 6_000),
        ]),
        "ja" => phrases.extend([
            (vec!["えー"], 9_500),
            (vec!["あの"], 8_500),
            (vec!["その"], 6_500),
        ]),
        _ => {}
    }
    phrases.sort_by_key(|(tokens, _)| std::cmp::Reverse(tokens.len()));
    phrases
}

fn segment_word_map(transcript: &TranscriptDocument) -> HashMap<WordId, SegmentId> {
    transcript
        .segments
        .iter()
        .flat_map(|segment| {
            segment
                .word_ids
                .iter()
                .cloned()
                .map(move |word_id| (word_id, segment.id.clone()))
        })
        .collect()
}

fn active_segment_words<'a>(
    transcript: &'a TranscriptDocument,
    segment_id: &SegmentId,
) -> Vec<&'a crate::TranscriptWord> {
    let Some(segment) = transcript
        .segments
        .iter()
        .find(|segment| segment.id == *segment_id)
    else {
        return Vec::new();
    };
    segment
        .word_ids
        .iter()
        .filter_map(|word_id| transcript.word(word_id))
        .filter(|word| !word.deleted)
        .collect()
}

fn filler_suggestions(
    transcript: &TranscriptDocument,
    options: &TranscriptCleanupOptions,
    word_segments: &HashMap<WordId, SegmentId>,
) -> Vec<TranscriptCleanupSuggestion> {
    let phrases = filler_phrases(&transcript.language);
    let mut groups = transcript
        .segments
        .iter()
        .map(|segment| active_segment_words(transcript, &segment.id))
        .collect::<Vec<_>>();
    let referenced = word_segments.keys().collect::<HashSet<_>>();
    let unsegmented = transcript
        .words
        .iter()
        .filter(|word| !word.deleted && !referenced.contains(&word.id))
        .collect::<Vec<_>>();
    if !unsegmented.is_empty() {
        groups.push(unsegmented);
    }

    let mut suggestions = Vec::new();
    for words in groups {
        let tokens = words
            .iter()
            .map(|word| normalized_token(&word.display_text))
            .collect::<Vec<_>>();
        let mut occupied = vec![false; words.len()];
        for index in 0..words.len() {
            if occupied[index] {
                continue;
            }
            let Some((phrase, confidence_bps)) = phrases.iter().find(|(phrase, _)| {
                index + phrase.len() <= tokens.len()
                    && !occupied[index..index + phrase.len()]
                        .iter()
                        .any(|value| *value)
                    && tokens[index..index + phrase.len()]
                        .iter()
                        .map(String::as_str)
                        .eq(phrase.iter().copied())
            }) else {
                continue;
            };
            occupied[index..index + phrase.len()].fill(true);
            let matched = &words[index..index + phrase.len()];
            let word_ids = matched
                .iter()
                .map(|word| word.id.clone())
                .collect::<Vec<_>>();
            let segment_ids = word_ids
                .iter()
                .filter_map(|word_id| word_segments.get(word_id).cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let start_ticks = matched
                .first()
                .expect("matched phrase is nonempty")
                .start_ticks;
            let end_ticks = matched
                .last()
                .expect("matched phrase is nonempty")
                .end_ticks;
            suggestions.push(TranscriptCleanupSuggestion {
                id: suggestion_id(
                    &transcript.id,
                    TranscriptCleanupSuggestionKind::Filler,
                    word_ids.iter().map(WordId::as_str),
                ),
                kind: TranscriptCleanupSuggestionKind::Filler,
                start_ticks,
                end_ticks,
                confidence_bps: *confidence_bps,
                reason: format!("Matched the {} filler expression", phrase.join(" ")),
                word_ids: word_ids.clone(),
                segment_ids,
                action: TranscriptCleanupAction::DeleteWords { word_ids },
                recommended: *confidence_bps >= options.minimum_apply_confidence_bps,
                estimated_removed_ticks: end_ticks.saturating_sub(start_ticks),
            });
        }
    }
    suggestions
}

fn token_similarity_bps(left: &[String], right: &[String]) -> u16 {
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    let left_set = left.iter().collect::<HashSet<_>>();
    let right_set = right.iter().collect::<HashSet<_>>();
    let intersection = left_set.intersection(&right_set).count() as u64;
    let union = left_set.union(&right_set).count() as u64;
    let jaccard = intersection * u64::from(CONFIDENCE_SCALE) / union.max(1);
    let length_ratio = left.len().min(right.len()) as u64 * u64::from(CONFIDENCE_SCALE)
        / left.len().max(right.len()) as u64;
    ((jaccard * 2 + length_ratio) / 3) as u16
}

fn repeated_take_suggestions(
    transcript: &TranscriptDocument,
    options: &TranscriptCleanupOptions,
) -> Vec<TranscriptCleanupSuggestion> {
    let mut suggestions = Vec::new();
    for pair in transcript.segments.windows(2) {
        let previous = &pair[0];
        let next = &pair[1];
        if previous.speaker_id.is_some()
            && next.speaker_id.is_some()
            && previous.speaker_id != next.speaker_id
        {
            continue;
        }
        let previous_words = active_segment_words(transcript, &previous.id);
        let next_words = active_segment_words(transcript, &next.id);
        if previous_words.len() < options.minimum_repeated_take_words
            || next_words.len() < options.minimum_repeated_take_words
        {
            continue;
        }
        let previous_tokens = previous_words
            .iter()
            .map(|word| normalized_token(&word.display_text))
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        let next_tokens = next_words
            .iter()
            .map(|word| normalized_token(&word.display_text))
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        let confidence_bps = token_similarity_bps(&previous_tokens, &next_tokens);
        if confidence_bps < options.repeated_take_similarity_bps {
            continue;
        }
        let start_ticks = previous_words.first().expect("length checked").start_ticks;
        let end_ticks = previous_words.last().expect("length checked").end_ticks;
        let word_ids = previous_words
            .iter()
            .map(|word| word.id.clone())
            .collect::<Vec<_>>();
        suggestions.push(TranscriptCleanupSuggestion {
            id: suggestion_id(
                &transcript.id,
                TranscriptCleanupSuggestionKind::RepeatedTake,
                [previous.id.as_str(), next.id.as_str()],
            ),
            kind: TranscriptCleanupSuggestionKind::RepeatedTake,
            start_ticks,
            end_ticks,
            confidence_bps,
            reason: format!(
                "Adjacent takes have {:.0}% token similarity; keep the later take for review",
                f64::from(confidence_bps) / 100.0
            ),
            word_ids,
            segment_ids: vec![previous.id.clone(), next.id.clone()],
            action: TranscriptCleanupAction::DeleteRepeatedTake {
                segment_id: previous.id.clone(),
                keep_segment_id: next.id.clone(),
            },
            recommended: confidence_bps >= options.minimum_apply_confidence_bps,
            estimated_removed_ticks: end_ticks.saturating_sub(start_ticks),
        });
    }
    suggestions
}

fn pause_suggestions(
    transcript: &TranscriptDocument,
    options: &TranscriptCleanupOptions,
    word_segments: &HashMap<WordId, SegmentId>,
) -> Vec<TranscriptCleanupSuggestion> {
    let mut words = transcript
        .words
        .iter()
        .filter(|word| !word.deleted)
        .collect::<Vec<_>>();
    words.sort_by_key(|word| (word.start_ticks, word.end_ticks, word.id.clone()));
    words
        .windows(2)
        .filter_map(|pair| {
            let previous = pair[0];
            let next = pair[1];
            let gap = next.start_ticks.checked_sub(previous.end_ticks)?;
            if gap <= options.pause_threshold_ticks {
                return None;
            }
            let word_ids = vec![previous.id.clone(), next.id.clone()];
            let segment_ids = word_ids
                .iter()
                .filter_map(|word_id| word_segments.get(word_id).cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            Some(TranscriptCleanupSuggestion {
                id: suggestion_id(
                    &transcript.id,
                    TranscriptCleanupSuggestionKind::LongPause,
                    word_ids.iter().map(WordId::as_str),
                ),
                kind: TranscriptCleanupSuggestionKind::LongPause,
                start_ticks: previous.end_ticks,
                end_ticks: next.start_ticks,
                confidence_bps: CONFIDENCE_SCALE,
                reason: format!(
                    "Spoken-word timestamps contain a {:.2}s pause",
                    gap as f64 / TICKS_PER_SECOND as f64
                ),
                word_ids: word_ids.clone(),
                segment_ids,
                action: TranscriptCleanupAction::CloseGap {
                    previous_word_id: previous.id.clone(),
                    next_word_id: next.id.clone(),
                    target_gap_ticks: options.target_pause_ticks,
                },
                recommended: true,
                estimated_removed_ticks: gap.saturating_sub(options.target_pause_ticks),
            })
        })
        .collect()
}

fn highlight_suggestions(
    transcript: &TranscriptDocument,
    options: &TranscriptCleanupOptions,
    repeated_segments: &HashSet<SegmentId>,
) -> Vec<TranscriptCleanupSuggestion> {
    let mut candidates = transcript
        .segments
        .iter()
        .filter(|segment| !repeated_segments.contains(&segment.id))
        .filter_map(|segment| {
            let words = active_segment_words(transcript, &segment.id);
            if !(4..=80).contains(&words.len()) {
                return None;
            }
            let start_ticks = words.first()?.start_ticks;
            let end_ticks = words.last()?.end_ticks;
            let duration = end_ticks.checked_sub(start_ticks)?;
            if duration <= 0 || duration > TICKS_PER_SECOND * 45 {
                return None;
            }
            let tokens = words
                .iter()
                .map(|word| normalized_token(&word.display_text))
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>();
            if tokens.len() < 4 {
                return None;
            }
            let unique = tokens.iter().collect::<HashSet<_>>().len();
            let diversity = unique as u64 * u64::from(CONFIDENCE_SCALE) / tokens.len() as u64;
            let average_confidence = words
                .iter()
                .map(|word| {
                    word.confidence
                        .map(|value| (value.clamp(0.0, 1.0) * 10_000.0).round() as u64)
                        .unwrap_or(8_000)
                })
                .sum::<u64>()
                / words.len() as u64;
            let words_per_second = words.len() as f64 * TICKS_PER_SECOND as f64 / duration as f64;
            let density =
                (10_000.0 - (words_per_second - 3.0).abs() * 2_000.0).clamp(0.0, 10_000.0) as u64;
            let score = ((average_confidence + diversity + density) / 3) as u16;
            let word_ids = words.iter().map(|word| word.id.clone()).collect::<Vec<_>>();
            Some(TranscriptCleanupSuggestion {
                id: suggestion_id(
                    &transcript.id,
                    TranscriptCleanupSuggestionKind::Highlight,
                    [segment.id.as_str()],
                ),
                kind: TranscriptCleanupSuggestionKind::Highlight,
                start_ticks,
                end_ticks,
                confidence_bps: score,
                reason: "Compact, confident speech with strong lexical density".into(),
                word_ids: word_ids.clone(),
                segment_ids: vec![segment.id.clone()],
                action: TranscriptCleanupAction::ExtractHighlight { word_ids },
                recommended: false,
                estimated_removed_ticks: 0,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| {
        (
            std::cmp::Reverse(candidate.confidence_bps),
            candidate.start_ticks,
        )
    });
    candidates.truncate(options.highlight_limit);
    candidates
}

/// Analyze immutable spoken/display text and source word timestamps without
/// calling a model or mutating the project. Every candidate remains anchored to
/// stable word/segment IDs so a later proposal can be revalidated by revision.
pub fn analyze_transcript_cleanup(
    transcript: &TranscriptDocument,
    options: TranscriptCleanupOptions,
) -> Result<TranscriptCleanupAnalysis, DomainError> {
    options.validate()?;
    let word_segments = segment_word_map(transcript);
    let mut suggestions = filler_suggestions(transcript, &options, &word_segments);
    let repeated = repeated_take_suggestions(transcript, &options);
    let repeated_segments = repeated
        .iter()
        .filter_map(|suggestion| match &suggestion.action {
            TranscriptCleanupAction::DeleteRepeatedTake { segment_id, .. } => {
                Some(segment_id.clone())
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    suggestions.extend(repeated);
    suggestions.extend(pause_suggestions(transcript, &options, &word_segments));
    suggestions.extend(highlight_suggestions(
        transcript,
        &options,
        &repeated_segments,
    ));
    suggestions.sort_by_key(|suggestion| {
        let kind = match suggestion.kind {
            TranscriptCleanupSuggestionKind::Filler => 0,
            TranscriptCleanupSuggestionKind::RepeatedTake => 1,
            TranscriptCleanupSuggestionKind::LongPause => 2,
            TranscriptCleanupSuggestionKind::Highlight => 3,
        };
        (suggestion.start_ticks, kind, suggestion.id.clone())
    });
    let summary = TranscriptCleanupSummary {
        filler_count: suggestions
            .iter()
            .filter(|candidate| candidate.kind == TranscriptCleanupSuggestionKind::Filler)
            .count(),
        repeated_take_count: suggestions
            .iter()
            .filter(|candidate| candidate.kind == TranscriptCleanupSuggestionKind::RepeatedTake)
            .count(),
        long_pause_count: suggestions
            .iter()
            .filter(|candidate| candidate.kind == TranscriptCleanupSuggestionKind::LongPause)
            .count(),
        highlight_count: suggestions
            .iter()
            .filter(|candidate| candidate.kind == TranscriptCleanupSuggestionKind::Highlight)
            .count(),
        recommended_removed_ticks: suggestions
            .iter()
            .filter(|candidate| candidate.recommended)
            .map(|candidate| candidate.estimated_removed_ticks)
            .fold(0i64, i64::saturating_add),
    };
    Ok(TranscriptCleanupAnalysis {
        transcript_id: transcript.id.clone(),
        options,
        summary,
        suggestions,
    })
}

fn cleanup_operations(
    envelope: &ProjectEnvelope,
    analysis: &TranscriptCleanupAnalysis,
) -> Vec<Operation> {
    let mut filler_word_ids = BTreeSet::new();
    let mut repeated_segment_ids = BTreeSet::new();
    let mut has_pause = false;
    for suggestion in &analysis.suggestions {
        if !suggestion.recommended {
            continue;
        }
        match &suggestion.action {
            TranscriptCleanupAction::DeleteWords { word_ids } => {
                filler_word_ids.extend(word_ids.iter().cloned());
            }
            TranscriptCleanupAction::DeleteRepeatedTake { segment_id, .. } => {
                repeated_segment_ids.insert(segment_id.clone());
            }
            TranscriptCleanupAction::CloseGap { .. } => has_pause = true,
            TranscriptCleanupAction::ExtractHighlight { .. } => {}
        }
    }
    let mut operations = Vec::new();
    if !filler_word_ids.is_empty() {
        operations.push(Operation::SetTranscriptWordsDeleted {
            transcript_id: analysis.transcript_id.clone(),
            word_ids: filler_word_ids.into_iter().collect(),
            deleted: true,
        });
    }
    operations.extend(repeated_segment_ids.into_iter().map(|segment_id| {
        Operation::DeleteTranscriptSegment {
            transcript_id: analysis.transcript_id.clone(),
            segment_id,
        }
    }));
    if has_pause {
        operations.extend(
            envelope
                .document
                .story_sequences
                .iter()
                .filter(|sequence| sequence.transcript_id == analysis.transcript_id)
                .map(|sequence| Operation::CloseStoryGaps {
                    sequence_id: sequence.id.clone(),
                    threshold_ticks: analysis.options.pause_threshold_ticks,
                    target_gap_ticks: analysis.options.target_pause_ticks,
                }),
        );
    }
    operations
}

fn cleanup_dependency_impacts(
    envelope: &ProjectEnvelope,
    transcript_id: &TranscriptId,
) -> Vec<DependencyImpact> {
    let mut impacts = envelope
        .document
        .story_sequences
        .iter()
        .filter(|sequence| sequence.transcript_id == *transcript_id)
        .map(|sequence| DependencyImpact {
            entity_id: sequence.id.to_string(),
            kind: DependencyImpactKind::Reanchor,
            reason: "Spoken-word edits rematerialize linked A/V clips".into(),
        })
        .collect::<Vec<_>>();
    impacts.extend(
        envelope
            .document
            .scenes
            .iter()
            .flat_map(|scene| &scene.tracks)
            .flat_map(|track| &track.items)
            .filter(|item| {
                matches!(
                    &item.content,
                    ItemContent::Caption { caption } if caption.transcript_id == *transcript_id
                ) || item
                    .timeline_anchor
                    .as_ref()
                    .is_some_and(|anchor| anchor.transcript_id == *transcript_id)
            })
            .map(|item| DependencyImpact {
                entity_id: item.id.to_string(),
                kind: DependencyImpactKind::Reanchor,
                reason: "Transcript-anchored timeline content follows the surviving words".into(),
            }),
    );
    impacts
}

/// Build a revision-pinned, model-free cleanup proposal. The returned plan is
/// never applied by this function; callers must still use the normal proposal
/// gate and revision CAS.
pub fn build_transcript_cleanup_edit_plan(
    envelope: &ProjectEnvelope,
    transcript_id: &TranscriptId,
    plan_id: EditPlanId,
    actor: Actor,
    options: TranscriptCleanupOptions,
) -> Result<EditPlan, DomainError> {
    envelope.verify()?;
    let transcript = envelope
        .document
        .transcripts
        .iter()
        .find(|transcript| transcript.id == *transcript_id)
        .ok_or_else(|| DomainError::EntityNotFound {
            entity: "transcript".into(),
            id: transcript_id.to_string(),
        })?;
    let analysis = analyze_transcript_cleanup(transcript, options)?;
    let operations = cleanup_operations(envelope, &analysis);
    let (changes, expected_result_hash) = if operations.is_empty() {
        (Vec::new(), envelope.document_hash.clone())
    } else {
        let outcome = apply_operations(&envelope.document, &operations)?;
        (
            outcome.changes,
            canonical_document_hash(&outcome.document)
                .map_err(|error| invalid(error.to_string()))?,
        )
    };
    let has_deletion = operations.iter().any(|operation| {
        matches!(
            operation,
            Operation::SetTranscriptWordsDeleted { deleted: true, .. }
                | Operation::DeleteTranscriptSegment { .. }
        )
    });
    let has_pause_candidates = analysis.summary.long_pause_count > 0;
    let has_materialized_story = envelope
        .document
        .story_sequences
        .iter()
        .any(|sequence| sequence.transcript_id == *transcript_id);
    let mut warnings = Vec::new();
    if has_deletion {
        warnings.push(PlanWarning {
            code: "semanticDeletion".into(),
            message: "Recommended filler/repeated-take removals change spoken content; review every stable word and segment anchor before applying.".into(),
            severity: WarningSeverity::Destructive,
            requires_confirmation: true,
        });
    }
    if has_pause_candidates && !has_materialized_story {
        warnings.push(PlanWarning {
            code: "storyMaterializationRequired".into(),
            message: "Long pauses were detected, but no StorySequence exists yet; transcribe a timeline-placed source before applying pause compression.".into(),
            severity: WarningSeverity::Warning,
            requires_confirmation: false,
        });
    }
    let approval = if has_deletion {
        ApprovalRequirement::Confirm {
            reasons: vec!["semantic spoken-content deletion".into()],
        }
    } else if operations.is_empty() {
        ApprovalRequirement::None
    } else {
        ApprovalRequirement::AutoApplyEligible
    };
    let objective = format!(
        "Review {} filler, {} repeated-take, {} long-pause, and {} highlight candidate(s)",
        analysis.summary.filler_count,
        analysis.summary.repeated_take_count,
        analysis.summary.long_pause_count,
        analysis.summary.highlight_count,
    );
    let mut extensions = Extensions::new();
    extensions.insert(
        "transcriptCleanupAnalysis".into(),
        serde_json::to_value(&analysis).map_err(|error| invalid(error.to_string()))?,
    );
    Ok(EditPlan {
        id: plan_id,
        project_id: envelope.document.id.clone(),
        expected_revision: envelope.revision,
        objective: objective.clone(),
        actor,
        operations,
        diff: PlanDiff {
            summary: objective,
            changes,
            dependency_impacts: cleanup_dependency_impacts(envelope, transcript_id),
            expected_result_hash: Some(expected_result_hash),
        },
        warnings,
        estimated_costs: vec![CostEstimate {
            amount_micros: 0,
            currency: "USD".into(),
            unit: Some("local-analysis".into()),
            is_estimate: false,
        }],
        approval,
        extensions,
    })
}
