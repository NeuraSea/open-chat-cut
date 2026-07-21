use openchatcut_domain::{
    Actor, ApprovalRequirement, EditPlanId, Extensions, LinkGroupId, Operation, ProjectDocument,
    ProjectEnvelope, ProjectId, SegmentId, SpeakerId, StoryClip, StoryClipId, StorySequence,
    StorySequenceId, TranscriptCleanupAction, TranscriptCleanupOptions,
    TranscriptCleanupSuggestionKind, TranscriptDocument, TranscriptId, TranscriptSegment,
    TranscriptSpeaker, TranscriptWord, WordId, analyze_transcript_cleanup, apply_operations,
    build_transcript_cleanup_edit_plan, canonical_document_hash,
};

fn word(id: &str, text: &str, start_ticks: i64, end_ticks: i64) -> TranscriptWord {
    TranscriptWord {
        id: WordId::new(id).unwrap(),
        spoken_text: text.into(),
        display_text: text.into(),
        start_ticks,
        end_ticks,
        speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
        deleted: false,
        confidence: Some(0.98),
        extensions: Extensions::new(),
    }
}

fn fixture() -> (ProjectEnvelope, TranscriptId) {
    let words = vec![
        word("word-filler", "um", 0, 12_000),
        word("word-we", "we", 12_000, 24_000),
        word("word-ship", "ship", 24_000, 36_000),
        word("word-today", "today", 36_000, 48_000),
        word("word-repeat-a-1", "great", 60_000, 72_000),
        word("word-repeat-a-2", "software", 72_000, 84_000),
        word("word-repeat-a-3", "ships", 84_000, 96_000),
        word("word-repeat-a-4", "today", 96_000, 108_000),
        word("word-repeat-b-1", "great", 120_000, 132_000),
        word("word-repeat-b-2", "software", 132_000, 144_000),
        word("word-repeat-b-3", "ships", 144_000, 156_000),
        word("word-repeat-b-4", "today", 156_000, 168_000),
        word("word-highlight-1", "customers", 400_000, 412_000),
        word("word-highlight-2", "love", 412_000, 424_000),
        word("word-highlight-3", "simple", 424_000, 436_000),
        word("word-highlight-4", "reliable", 436_000, 448_000),
        word("word-highlight-5", "editing", 448_000, 460_000),
        word("word-highlight-6", "workflows", 460_000, 472_000),
    ];
    let segment = |id: &str, word_ids: &[&str]| TranscriptSegment {
        id: SegmentId::new(id).unwrap(),
        word_ids: word_ids
            .iter()
            .map(|id| WordId::new(*id).unwrap())
            .collect(),
        speaker_id: Some(SpeakerId::new("speaker-1").unwrap()),
    };
    let segments = vec![
        segment(
            "segment-opening",
            &["word-filler", "word-we", "word-ship", "word-today"],
        ),
        segment(
            "segment-repeat-a",
            &[
                "word-repeat-a-1",
                "word-repeat-a-2",
                "word-repeat-a-3",
                "word-repeat-a-4",
            ],
        ),
        segment(
            "segment-repeat-b",
            &[
                "word-repeat-b-1",
                "word-repeat-b-2",
                "word-repeat-b-3",
                "word-repeat-b-4",
            ],
        ),
        segment(
            "segment-highlight",
            &[
                "word-highlight-1",
                "word-highlight-2",
                "word-highlight-3",
                "word-highlight-4",
                "word-highlight-5",
                "word-highlight-6",
            ],
        ),
    ];
    let transcript_id = TranscriptId::new("transcript-cleanup").unwrap();
    let transcript = TranscriptDocument {
        id: transcript_id.clone(),
        asset_id: None,
        language: "en-US".into(),
        speakers: vec![TranscriptSpeaker {
            id: SpeakerId::new("speaker-1").unwrap(),
            label: "Speaker".into(),
            color: None,
        }],
        words,
        segments: segments.clone(),
        extensions: Extensions::new(),
    };
    let clips = [
        ("opening", 0, 0, 60_000, &segments[0]),
        ("repeat-a", 60_000, 60_000, 120_000, &segments[1]),
        ("repeat-b", 120_000, 120_000, 400_000, &segments[2]),
        ("highlight", 400_000, 400_000, 500_000, &segments[3]),
    ]
    .into_iter()
    .map(
        |(id, timeline_start_ticks, source_start_ticks, source_end_ticks, segment)| StoryClip {
            id: StoryClipId::new(format!("clip-{id}")).unwrap(),
            word_ids: segment.word_ids.clone(),
            timeline_start_ticks,
            source_start_ticks,
            source_end_ticks,
            link_group_id: LinkGroupId::new(format!("link-{id}")).unwrap(),
            extensions: Extensions::new(),
        },
    )
    .collect();
    let sequence = StorySequence {
        id: StorySequenceId::new("story-cleanup").unwrap(),
        transcript_id: transcript_id.clone(),
        clips,
        extensions: Extensions::new(),
    };
    let mut document = ProjectDocument::new(
        ProjectId::new("project-cleanup").unwrap(),
        "Transcript cleanup",
    );
    document.transcripts.push(transcript);
    document.story_sequences.push(sequence);
    (ProjectEnvelope::new(document).unwrap(), transcript_id)
}

#[test]
fn cleanup_analysis_finds_reviewable_fillers_repeats_pauses_and_highlights() {
    let (envelope, transcript_id) = fixture();
    let transcript = envelope
        .document
        .transcripts
        .iter()
        .find(|transcript| transcript.id == transcript_id)
        .unwrap();
    let analysis = analyze_transcript_cleanup(
        transcript,
        TranscriptCleanupOptions {
            highlight_limit: 2,
            ..TranscriptCleanupOptions::default()
        },
    )
    .unwrap();

    assert_eq!(analysis.summary.filler_count, 1);
    assert_eq!(analysis.summary.repeated_take_count, 1);
    assert_eq!(analysis.summary.long_pause_count, 1);
    assert_eq!(analysis.summary.highlight_count, 2);
    let filler = analysis
        .suggestions
        .iter()
        .find(|suggestion| suggestion.kind == TranscriptCleanupSuggestionKind::Filler)
        .unwrap();
    assert!(filler.recommended);
    assert_eq!(filler.word_ids, [WordId::new("word-filler").unwrap()]);
    let repeated = analysis
        .suggestions
        .iter()
        .find(|suggestion| suggestion.kind == TranscriptCleanupSuggestionKind::RepeatedTake)
        .unwrap();
    assert!(matches!(
        &repeated.action,
        TranscriptCleanupAction::DeleteRepeatedTake {
            segment_id,
            keep_segment_id,
        } if segment_id.as_str() == "segment-repeat-a"
            && keep_segment_id.as_str() == "segment-repeat-b"
    ));
    let pause = analysis
        .suggestions
        .iter()
        .find(|suggestion| suggestion.kind == TranscriptCleanupSuggestionKind::LongPause)
        .unwrap();
    assert_eq!(pause.start_ticks, 168_000);
    assert_eq!(pause.end_ticks, 400_000);
    assert_eq!(pause.estimated_removed_ticks, 210_400);
    assert!(
        analysis
            .suggestions
            .iter()
            .filter(|suggestion| suggestion.kind == TranscriptCleanupSuggestionKind::Highlight)
            .all(|suggestion| !suggestion.recommended),
        "heuristic highlights are review-only and never silently cut the project"
    );
}

#[test]
fn cleanup_plan_is_revision_pinned_zero_cost_and_applies_as_one_atomic_edit() {
    let (envelope, transcript_id) = fixture();
    let plan = build_transcript_cleanup_edit_plan(
        &envelope,
        &transcript_id,
        EditPlanId::new("plan-cleanup").unwrap(),
        Actor::system(),
        TranscriptCleanupOptions::default(),
    )
    .unwrap();

    assert_eq!(plan.expected_revision, 0);
    assert_eq!(plan.operations.len(), 3);
    assert!(matches!(
        &plan.operations[0],
        Operation::SetTranscriptWordsDeleted { word_ids, deleted: true, .. }
            if word_ids == &[WordId::new("word-filler").unwrap()]
    ));
    assert!(matches!(
        &plan.operations[1],
        Operation::DeleteTranscriptSegment { segment_id, .. }
            if segment_id.as_str() == "segment-repeat-a"
    ));
    assert!(matches!(
        &plan.operations[2],
        Operation::CloseStoryGaps { sequence_id, .. }
            if sequence_id.as_str() == "story-cleanup"
    ));
    assert!(matches!(plan.approval, ApprovalRequirement::Confirm { .. }));
    assert_eq!(plan.estimated_costs[0].amount_micros, 0);
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.code == "semanticDeletion")
    );
    assert!(plan.extensions.contains_key("transcriptCleanupAnalysis"));

    let outcome = apply_operations(&envelope.document, &plan.operations).unwrap();
    assert_eq!(
        plan.diff.expected_result_hash,
        Some(canonical_document_hash(&outcome.document).unwrap())
    );
    let transcript = &outcome.document.transcripts[0];
    assert!(
        transcript
            .word(&WordId::new("word-filler").unwrap())
            .unwrap()
            .deleted
    );
    let repeated_segment = transcript
        .segments
        .iter()
        .find(|segment| segment.id.as_str() == "segment-repeat-a")
        .unwrap();
    assert!(
        repeated_segment
            .word_ids
            .iter()
            .all(|word_id| { transcript.word(word_id).is_some_and(|word| word.deleted) })
    );
    assert_eq!(outcome.document.story_sequences[0].clips.len(), 3);
    assert_eq!(
        outcome.document.story_sequences[0].clips[2].timeline_start_ticks,
        120_000
    );
}

#[test]
fn ambiguous_discourse_fillers_are_suggestions_but_not_recommended_deletions() {
    let mut transcript = TranscriptDocument {
        id: TranscriptId::new("transcript-ambiguous").unwrap(),
        asset_id: None,
        language: "en".into(),
        speakers: Vec::new(),
        words: vec![word("word-like", "like", 0, 12_000)],
        segments: vec![TranscriptSegment {
            id: SegmentId::new("segment-like").unwrap(),
            word_ids: vec![WordId::new("word-like").unwrap()],
            speaker_id: None,
        }],
        extensions: Extensions::new(),
    };
    transcript.words[0].speaker_id = None;
    let analysis =
        analyze_transcript_cleanup(&transcript, TranscriptCleanupOptions::default()).unwrap();
    assert_eq!(analysis.summary.filler_count, 1);
    assert!(!analysis.suggestions[0].recommended);
    assert_eq!(analysis.suggestions[0].confidence_bps, 6_000);
}
