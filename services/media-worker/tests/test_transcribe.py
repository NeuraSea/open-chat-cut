from openchatcut_worker.protocol import TranscriptWord
from openchatcut_worker.transcribe import _align_speakers


def test_diarization_alignment_uses_overlap_and_splits_utterances() -> None:
    words = [
        TranscriptWord("w1", "one", "one", 0, 400),
        TranscriptWord("w2", "two", "two", 450, 800),
        TranscriptWord("w3", "three", "three", 900, 1200),
    ]
    segments = [{"id": "original", "speakerId": None, "wordIds": ["w1", "w2", "w3"]}]
    aligned, utterances = _align_speakers(
        words=words,
        segment_items=segments,
        turns=[(0, 700, "SPEAKER_02"), (700, 1300, "SPEAKER_00")],
        source_hash="abcdef0123456789",
    )

    assert [word.speaker_id for word in aligned] == ["speaker_1", "speaker_1", "speaker_2"]
    assert utterances == [
        {
            "id": "utterance_abcdef012345_0",
            "speakerId": "speaker_1",
            "wordIds": ["w1", "w2"],
        },
        {
            "id": "utterance_abcdef012345_1",
            "speakerId": "speaker_2",
            "wordIds": ["w3"],
        },
    ]


def test_diarization_labels_are_stable_by_first_turn() -> None:
    words = [
        TranscriptWord("late", "late", "late", 1000, 1100),
        TranscriptWord("early", "early", "early", 100, 200),
    ]
    aligned, _ = _align_speakers(
        words=words,
        segment_items=[{"wordIds": ["early", "late"]}],
        turns=[(900, 1200, "A"), (0, 300, "Z")],
        source_hash="0123456789abcdef",
    )
    assert [word.speaker_id for word in aligned] == ["speaker_2", "speaker_1"]
