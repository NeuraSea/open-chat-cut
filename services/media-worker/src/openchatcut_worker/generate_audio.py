from __future__ import annotations

import shutil
import subprocess
import wave
from pathlib import Path
from typing import Any, Callable

from .errors import CapabilityUnavailable, WorkerError
from .security import resolve_under_root


Progress = Callable[[float, str], None]


def synthesize_voice(
    *,
    data_root: Path,
    destination: Path,
    options: dict[str, Any],
    progress: Progress,
) -> dict[str, Any]:
    text = options.get("text")
    if not isinstance(text, str) or not text.strip() or len(text.encode("utf-8")) > 100_000:
        raise WorkerError(
            "INVALID_VOICE_TEXT",
            "Voice text must contain 1 to 100000 UTF-8 bytes",
        )
    engine = options.get("engine", "auto")
    if engine not in ("auto", "piper", "kokoro"):
        raise WorkerError("INVALID_VOICE_ENGINE", "Voice engine must be auto, piper, or kokoro")
    speed = _bounded_float(options.get("speed", 1.0), "speed", 0.5, 2.0)

    # Executable selection is owned by the daemon installation, never by an
    # untrusted project/tool request.
    piper = shutil.which("piper")
    model_path = options.get("modelPath")
    if engine == "piper" or (engine == "auto" and piper is not None and model_path):
        if piper is None:
            raise CapabilityUnavailable("local-voice", "Piper executable was not found on PATH")
        if not isinstance(model_path, str) or not model_path:
            raise WorkerError("PIPER_MODEL_REQUIRED", "Piper requires options.modelPath")
        model = resolve_under_root(value=model_path, root=data_root)
        progress(0.15, "Synthesizing with Piper")
        command = [piper, "--model", str(model), "--output_file", str(destination)]
        speaker = options.get("speaker")
        if speaker is not None:
            if not isinstance(speaker, int) or speaker < 0:
                raise WorkerError("INVALID_PIPER_SPEAKER", "Piper speaker must be a non-negative integer")
            command.extend(["--speaker", str(speaker)])
        # Piper's length scale is the inverse of speaking speed.
        command.extend(["--length_scale", str(1.0 / speed)])
        completed = subprocess.run(
            command,
            input=text,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=20 * 60,
            check=False,
        )
        if completed.returncode != 0:
            raise WorkerError(
                "PIPER_FAILED",
                "Piper exited unsuccessfully; inspect the local worker log",
            )
        selected_engine = "piper"
        selected_model = model.name
    else:
        progress(0.1, "Loading Kokoro")
        try:
            import numpy as np  # type: ignore[import-not-found]
            import soundfile as sf  # type: ignore[import-not-found]
            from kokoro import KPipeline  # type: ignore[import-not-found]
        except ImportError as error:
            if engine == "kokoro" or engine == "auto":
                raise CapabilityUnavailable(
                    "local-voice",
                    "Install openchatcut-media-worker[kokoro] or configure Piper",
                ) from error
            raise
        language = options.get("language", "a")
        voice = options.get("voice", "af_heart")
        if not isinstance(language, str) or len(language) != 1 or not language.isascii():
            raise WorkerError("INVALID_KOKORO_LANGUAGE", "Kokoro language must be a one-character code")
        if not isinstance(voice, str) or not voice or len(voice) > 100:
            raise WorkerError("INVALID_KOKORO_VOICE", "Kokoro voice is invalid")
        pipeline = KPipeline(lang_code=language)
        progress(0.25, "Synthesizing with Kokoro")
        chunks = []
        for _graphemes, _phonemes, audio in pipeline(
            text,
            voice=voice,
            speed=speed,
            split_pattern=r"\n+",
        ):
            chunks.append(audio)
        if not chunks:
            raise WorkerError("KOKORO_EMPTY_OUTPUT", "Kokoro produced no audio")
        sf.write(destination, np.concatenate(chunks), 24_000, subtype="PCM_16")
        selected_engine = "kokoro"
        selected_model = voice

    _validate_wave(destination)
    progress(0.95, "Validating synthesized voice")
    return {
        "generatedAssetPath": str(destination),
        "engine": selected_engine,
        "model": selected_model,
        "mimeType": "audio/wav",
    }


def synthesize_sfx(
    *,
    destination: Path,
    options: dict[str, Any],
    progress: Progress,
) -> dict[str, Any]:
    prompt = options.get("prompt")
    if not isinstance(prompt, str) or not prompt.strip() or len(prompt.encode("utf-8")) > 20_000:
        raise WorkerError("INVALID_SFX_PROMPT", "SFX prompt must contain 1 to 20000 UTF-8 bytes")
    duration = _bounded_float(options.get("durationSeconds", 5.0), "durationSeconds", 0.25, 30.0)
    model_name = options.get("model", "facebook/audiogen-medium")
    if not isinstance(model_name, str) or not model_name or len(model_name) > 200:
        raise WorkerError("INVALID_AUDIOGEN_MODEL", "AudioGen model is invalid")
    try:
        import soundfile as sf  # type: ignore[import-not-found]
        from audiocraft.models import AudioGen  # type: ignore[import-not-found]
    except ImportError as error:
        raise CapabilityUnavailable(
            "local-audiogen",
            "Install openchatcut-media-worker[audiogen] to synthesize sound effects",
        ) from error
    progress(0.1, "Loading AudioGen")
    model = AudioGen.get_pretrained(model_name)
    model.set_generation_params(duration=duration)
    progress(0.35, "Synthesizing sound effect")
    waveform = model.generate([prompt], progress=False)[0]
    samples = waveform.detach().cpu().numpy()
    if samples.ndim == 2:
        samples = samples.T
    sf.write(destination, samples, model.sample_rate, subtype="PCM_16")
    _validate_wave(destination)
    progress(0.95, "Validating synthesized sound effect")
    return {
        "generatedAssetPath": str(destination),
        "engine": "audiogen",
        "model": model_name,
        "mimeType": "audio/wav",
    }


def _bounded_float(value: Any, name: str, minimum: float, maximum: float) -> float:
    try:
        parsed = float(value)
    except (TypeError, ValueError) as error:
        raise WorkerError(f"INVALID_{name.upper()}", f"{name} must be numeric") from error
    if not minimum <= parsed <= maximum:
        raise WorkerError(
            f"INVALID_{name.upper()}",
            f"{name} must be between {minimum} and {maximum}",
        )
    return parsed


def _validate_wave(path: Path) -> None:
    if not path.is_file() or path.stat().st_size <= 44:
        raise WorkerError("INVALID_GENERATED_AUDIO", "Audio generator did not produce a non-empty WAV")
    try:
        with wave.open(str(path), "rb") as audio:
            if audio.getnchannels() not in (1, 2) or audio.getframerate() <= 0 or audio.getnframes() <= 0:
                raise WorkerError("INVALID_GENERATED_AUDIO", "Generated WAV has invalid stream metadata")
    except (wave.Error, EOFError) as error:
        raise WorkerError("INVALID_GENERATED_AUDIO", "Generated audio is not a valid WAV") from error
