#!/usr/bin/env python3
"""Download and validate the pinned WhisperX ASR and Silero VAD weights."""

from __future__ import annotations

import argparse
from pathlib import Path

import whisperx


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--model-root", type=Path, required=True)
    parser.add_argument("--threads", type=int, default=16)
    arguments = parser.parse_args()
    arguments.model_root.mkdir(parents=True, exist_ok=True)
    model = whisperx.load_model(
        arguments.model,
        "cpu",
        compute_type="int8",
        vad_method="silero",
        download_root=str(arguments.model_root),
        threads=arguments.threads,
    )
    if model is None:
        raise RuntimeError("WhisperX returned no ASR model")
    print(f"WhisperX model is ready: {arguments.model}")


if __name__ == "__main__":
    main()
