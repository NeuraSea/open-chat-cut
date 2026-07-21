from __future__ import annotations

import os
from pathlib import Path

import pytest

from openchatcut_worker.errors import CapabilityUnavailable, WorkerError
from openchatcut_worker.generate_audio import synthesize_sfx, synthesize_voice


def test_piper_voice_is_generated_to_a_valid_wave_under_the_data_root(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = tmp_path / "data"
    model = root / "models" / "voice.onnx"
    model.parent.mkdir(parents=True)
    model.write_bytes(b"model")
    binary = tmp_path / "bin" / "piper"
    binary.parent.mkdir()
    binary.write_text(
        """#!/usr/bin/env python3
import sys, wave
from pathlib import Path
args = sys.argv[1:]
destination = Path(args[args.index('--output_file') + 1])
sys.stdin.read()
with wave.open(str(destination), 'wb') as output:
    output.setnchannels(1)
    output.setsampwidth(2)
    output.setframerate(16000)
    output.writeframes(b'\\0\\0' * 160)
""",
        encoding="utf-8",
    )
    binary.chmod(0o700)
    monkeypatch.setenv("PATH", f"{binary.parent}{os.pathsep}{os.environ.get('PATH', '')}")
    destination = root / "derived" / "voice.wav"
    destination.parent.mkdir(parents=True)

    result = synthesize_voice(
        data_root=root,
        destination=destination,
        options={"text": "Hello", "engine": "piper", "modelPath": "models/voice.onnx"},
        progress=lambda _value, _message: None,
    )

    assert destination.read_bytes().startswith(b"RIFF")
    assert result["engine"] == "piper"
    assert result["mimeType"] == "audio/wav"


def test_piper_model_cannot_escape_the_data_root(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    binary = tmp_path / "piper"
    binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    binary.chmod(0o700)
    monkeypatch.setenv("PATH", str(tmp_path))
    (tmp_path / "data").mkdir()
    (tmp_path / "secret.onnx").write_bytes(b"secret")
    with pytest.raises(WorkerError) as captured:
        synthesize_voice(
            data_root=tmp_path / "data",
            destination=tmp_path / "voice.wav",
            options={"text": "Hello", "engine": "piper", "modelPath": "../secret.onnx"},
            progress=lambda _value, _message: None,
        )
    assert captured.value.code == "PATH_OUTSIDE_AUTHORIZED_ROOT"


def test_audiogen_reports_an_honest_optional_capability_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    # The baseline worker intentionally has no heavyweight AudioGen dependency.
    monkeypatch.setitem(__import__("sys").modules, "audiocraft", None)
    with pytest.raises(CapabilityUnavailable):
        synthesize_sfx(
            destination=tmp_path / "sfx.wav",
            options={"prompt": "short click"},
            progress=lambda _value, _message: None,
        )
