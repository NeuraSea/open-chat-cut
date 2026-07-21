from __future__ import annotations

import shutil
import subprocess
import wave
from pathlib import Path

import pytest

from openchatcut_worker.ffmpeg import loop_audio, process_audio, process_audio_pair
from openchatcut_worker.errors import CapabilityUnavailable
from openchatcut_worker.protocol import JobRequest
from openchatcut_worker.runner import JobRunner


pytestmark = pytest.mark.skipif(shutil.which("ffmpeg") is None, reason="ffmpeg is required")


def _tone(path: Path, *, frequency: int, duration: float = 0.6) -> None:
    ffmpeg = shutil.which("ffmpeg")
    assert ffmpeg is not None
    subprocess.run(
        [
            ffmpeg,
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            f"sine=frequency={frequency}:sample_rate=48000:duration={duration}",
            "-c:a",
            "pcm_s16le",
            str(path),
        ],
        check=True,
    )


def _duration(path: Path) -> float:
    with wave.open(str(path), "rb") as audio:
        assert audio.getnchannels() in (1, 2)
        assert audio.getsampwidth() == 3
        return audio.getnframes() / audio.getframerate()


def test_dialogue_compression_and_loop_create_new_wav_without_overwriting_source(
    tmp_path: Path,
) -> None:
    source = tmp_path / "source.wav"
    compressed = tmp_path / "compressed.wav"
    looped = tmp_path / "looped.wav"
    _tone(source, frequency=440)
    original = source.read_bytes()

    process_audio(
        source=source,
        destination=compressed,
        filter_graph=(
            "highpass=f=70,acompressor=threshold=-18dB:ratio=3:"
            "attack=15:release=180:makeup=3"
        ),
    )
    loop_audio(
        source=source,
        destination=looped,
        duration_seconds=1.4,
        fade_seconds=0.05,
    )

    assert source.read_bytes() == original
    assert 0.55 <= _duration(compressed) <= 0.65
    assert 1.39 <= _duration(looped) <= 1.41


def test_ducking_and_crossfade_accept_two_managed_audio_inputs(tmp_path: Path) -> None:
    primary = tmp_path / "primary.wav"
    secondary = tmp_path / "secondary.wav"
    ducked = tmp_path / "ducked.wav"
    crossfaded = tmp_path / "crossfaded.wav"
    _tone(primary, frequency=220)
    _tone(secondary, frequency=880)

    process_audio_pair(
        source=primary,
        secondary=secondary,
        destination=ducked,
        filter_graph=(
            "[0:a][1:a]sidechaincompress=threshold=0.05:ratio=8:"
            "attack=20:release=300[ducked];"
            "[ducked]apad=whole_dur=0.600000000[out]"
        ),
    )
    process_audio_pair(
        source=primary,
        secondary=secondary,
        destination=crossfaded,
        filter_graph="[0:a][1:a]acrossfade=d=0.2:c1=tri:c2=tri[out]",
    )

    assert 0.55 <= _duration(ducked) <= 0.65
    assert 0.95 <= _duration(crossfaded) <= 1.05


def test_auto_denoise_falls_back_without_deepfilternet(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    data_root = tmp_path / "data"
    media = data_root / "media"
    media.mkdir(parents=True)
    source = media / "source.wav"
    _tone(source, frequency=440)

    def unavailable(**_kwargs: object) -> None:
        raise CapabilityUnavailable("DeepFilterNet", "install optional denoise dependencies")

    monkeypatch.setattr("openchatcut_worker.runner.deepfilter_denoise", unavailable)
    events = []
    result = JobRunner(data_root=data_root, emit=events.append).run(
        JobRequest.from_dict(
            {
                "jobId": "denoise-auto",
                "kind": "denoise",
                "projectId": "fixture",
                "inputPath": "media/source.wav",
                "outputDir": "derived/audio",
                "options": {
                    "engine": "auto",
                    "filter": "highpass=f=80,afftdn=nf=-25",
                },
            }
        )
    )
    assert result["engine"] == "ffmpeg-afftdn"
    assert Path(result["derivedAssetPath"]).read_bytes().startswith(b"RIFF")
