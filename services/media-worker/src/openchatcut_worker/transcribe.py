from __future__ import annotations

import hashlib
import math
import os
from dataclasses import replace
from pathlib import Path
from typing import Any, Callable

from .errors import CapabilityUnavailable, WorkerError
from .protocol import TranscriptWord


Progress = Callable[[float, str], None]
MAX_DIARIZATION_TURNS = 100_000


def _word_id(*, source_hash: str, index: int, start_ms: int, end_ms: int) -> str:
    stable = f"{source_hash}:{index}:{start_ms}:{end_ms}".encode("utf-8")
    return f"word_{hashlib.sha256(stable).hexdigest()[:20]}"


def _source_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def transcribe(
    *,
    source: Path,
    options: dict[str, Any],
    progress: Progress,
) -> dict[str, Any]:
    try:
        from faster_whisper import WhisperModel
    except ImportError as error:
        raise CapabilityUnavailable(
            "faster-whisper",
            "pip install 'openchatcut-media-worker[transcription]'",
        ) from error

    model_name = str(options.get("model", "small"))
    device = str(options.get("device", "auto"))
    compute_type = str(options.get("computeType", "default"))
    language = options.get("language")
    progress(0.02, f"Loading faster-whisper {model_name}")
    model = WhisperModel(model_name, device=device, compute_type=compute_type)
    segments, info = model.transcribe(
        str(source),
        language=language if isinstance(language, str) and language != "auto" else None,
        word_timestamps=True,
        vad_filter=True,
        condition_on_previous_text=False,
    )

    source_hash = _source_hash(source)
    words: list[TranscriptWord] = []
    segment_items: list[dict[str, Any]] = []
    word_index = 0
    duration = float(getattr(info, "duration", 0.0) or 0.0)

    for segment_index, segment in enumerate(segments):
        segment_word_ids: list[str] = []
        for word in segment.words or []:
            spoken = str(word.word)
            start_ms = round(float(word.start) * 1000)
            end_ms = max(start_ms + 1, round(float(word.end) * 1000))
            item = TranscriptWord(
                id=_word_id(
                    source_hash=source_hash,
                    index=word_index,
                    start_ms=start_ms,
                    end_ms=end_ms,
                ),
                spoken_text=spoken,
                display_text=spoken,
                start_ms=start_ms,
                end_ms=end_ms,
                confidence=float(word.probability) if word.probability is not None else None,
            )
            word_index += 1
            words.append(item)
            segment_word_ids.append(item.id)
        segment_items.append(
            {
                "id": f"utterance_{source_hash[:12]}_{segment_index}",
                "speakerId": None,
                "wordIds": segment_word_ids,
            }
        )
        if duration > 0:
            progress(min(0.78, max(0.05, float(segment.end) / duration * 0.8)), "Transcribing")

    if not words:
        raise WorkerError("NO_SPEECH_DETECTED", "No speech was detected in the selected media")

    diarization_model = None
    if options.get("diarization") is True:
        progress(0.8, "Loading optional speaker diarization")
        turns, diarization_model = _run_diarization(source=source, options=options)
        words, segment_items = _align_speakers(
            words=words,
            segment_items=segment_items,
            turns=turns,
            source_hash=source_hash,
        )
        progress(0.95, "Aligned speakers to transcript words")

    progress(0.96, "Building word-aligned transcript")
    engine: dict[str, Any] = {"name": "faster-whisper", "model": model_name}
    if diarization_model is not None:
        engine["diarization"] = {"name": "pyannote", "model": diarization_model}
    return {
        "schemaVersion": 1,
        "sourceSha256": source_hash,
        "language": getattr(info, "language", language),
        "languageProbability": getattr(info, "language_probability", None),
        "words": [word.to_dict() for word in words],
        "utterances": segment_items,
        "engine": engine,
    }


def _run_diarization(*, source: Path, options: dict[str, Any]) -> tuple[list[tuple[int, int, str]], str]:
    try:
        from pyannote.audio import Pipeline
    except ImportError as error:
        raise CapabilityUnavailable(
            "pyannote speaker diarization",
            "pip install 'openchatcut-media-worker[diarization]' and authorize the model with HF_TOKEN",
        ) from error

    token = os.environ.get("HF_TOKEN") or os.environ.get("HUGGING_FACE_HUB_TOKEN")
    if not token:
        raise CapabilityUnavailable(
            "pyannote model authorization",
            "accept the pyannote model terms, then set HF_TOKEN for the local media worker; manual speaker correction remains available",
        )
    model_name = os.environ.get(
        "OPENCHATCUT_DIARIZATION_MODEL",
        "pyannote/speaker-diarization-3.1",
    )
    try:
        try:
            pipeline = Pipeline.from_pretrained(model_name, token=token)
        except TypeError:
            pipeline = Pipeline.from_pretrained(model_name, use_auth_token=token)
    except Exception as error:
        raise WorkerError(
            "DIARIZATION_MODEL_UNAVAILABLE",
            "The configured pyannote model could not be loaded; verify model terms and HF_TOKEN",
            details={"model": model_name},
        ) from error

    device = os.environ.get("OPENCHATCUT_DIARIZATION_DEVICE", "cpu").lower()
    if device in ("cuda", "mps"):
        try:
            import torch

            pipeline.to(torch.device(device))
        except Exception as error:
            raise WorkerError(
                "DIARIZATION_ACCELERATOR_UNAVAILABLE",
                f"The requested {device} diarization device is unavailable",
            ) from error
    kwargs: dict[str, int] = {}
    for source_key, target_key in (("minSpeakers", "min_speakers"), ("maxSpeakers", "max_speakers")):
        value = options.get(source_key)
        if isinstance(value, int) and not isinstance(value, bool):
            kwargs[target_key] = value
    try:
        output = pipeline(str(source), **kwargs)
        annotation = getattr(output, "speaker_diarization", output)
        raw_turns = annotation.itertracks(yield_label=True)
        turns: list[tuple[int, int, str]] = []
        for turn, _, label in raw_turns:
            start = float(turn.start)
            end = float(turn.end)
            if not math.isfinite(start) or not math.isfinite(end) or start < 0 or end <= start:
                raise WorkerError("DIARIZATION_INVALID", "pyannote returned an invalid speaker turn")
            turns.append((round(start * 1000), round(end * 1000), str(label)))
            if len(turns) > MAX_DIARIZATION_TURNS:
                raise WorkerError("DIARIZATION_LIMIT", "Speaker diarization returned too many turns")
    except WorkerError:
        raise
    except Exception as error:
        raise WorkerError(
            "DIARIZATION_FAILED",
            "Speaker diarization failed for the selected media",
            details={"model": model_name},
        ) from error
    if not turns:
        raise WorkerError("DIARIZATION_EMPTY", "No speaker turns were detected")
    turns.sort(key=lambda value: (value[0], value[1], value[2]))
    return turns, model_name


def _align_speakers(
    *,
    words: list[TranscriptWord],
    segment_items: list[dict[str, Any]],
    turns: list[tuple[int, int, str]],
    source_hash: str,
) -> tuple[list[TranscriptWord], list[dict[str, Any]]]:
    if not turns:
        return words, segment_items
    turns = sorted(turns, key=lambda value: (value[0], value[1], value[2]))
    labels = {
        label: f"speaker_{index + 1}"
        for index, label in enumerate(dict.fromkeys(turn[2] for turn in turns))
    }
    aligned: list[TranscriptWord] = []
    for word in words:
        best = max(
            turns,
            key=lambda turn: (
                max(0, min(word.end_ms, turn[1]) - max(word.start_ms, turn[0])),
                -abs(((word.start_ms + word.end_ms) / 2) - ((turn[0] + turn[1]) / 2)),
                -turn[0],
            ),
        )
        aligned.append(replace(word, speaker_id=labels[best[2]]))

    by_id = {word.id: word for word in aligned}
    utterances: list[dict[str, Any]] = []
    utterance_index = 0
    for segment in segment_items:
        current_speaker: str | None = None
        current_word_ids: list[str] = []

        def flush() -> None:
            nonlocal utterance_index, current_word_ids
            if not current_word_ids:
                return
            utterances.append(
                {
                    "id": f"utterance_{source_hash[:12]}_{utterance_index}",
                    "speakerId": current_speaker,
                    "wordIds": current_word_ids,
                }
            )
            utterance_index += 1
            current_word_ids = []

        for word_id in segment.get("wordIds", []):
            word = by_id.get(str(word_id))
            if word is None:
                continue
            if current_word_ids and word.speaker_id != current_speaker:
                flush()
            current_speaker = word.speaker_id
            current_word_ids.append(word.id)
        flush()
    return aligned, utterances
