from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest

from openchatcut_worker.protocol import JobRequest
from openchatcut_worker.runner import JobRunner


pytestmark = pytest.mark.skipif(
    shutil.which("ffmpeg") is None or shutil.which("ffprobe") is None,
    reason="FFmpeg media preparation dependencies are unavailable",
)


def test_video_preparation_creates_visual_audio_and_analysis_derivatives(tmp_path: Path) -> None:
    data_root = tmp_path / "data"
    media = data_root / "media"
    media.mkdir(parents=True)
    source = media / "source.mp4"
    subprocess.run(
        [
            "ffmpeg",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "1",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            str(source),
        ],
        check=True,
    )
    request = JobRequest.from_dict(
        {
            "jobId": "derive-fixture",
            "kind": "prepare_media",
            "projectId": "fixture",
            "inputPath": "media/source.mp4",
            "outputDir": "derived/media",
            "options": {"assetKind": "video"},
        }
    )
    events = []
    result = JobRunner(data_root=data_root, emit=events.append).run(request)

    thumbnail = Path(result["thumbnailPath"])
    contact_sheet = Path(result["contactSheetPath"])
    waveform = Path(result["waveformPath"])
    proxy = Path(result["proxyPath"])
    audio = Path(result["extractedAudioPath"])
    assert thumbnail.read_bytes().startswith(b"\xff\xd8\xff")
    assert contact_sheet.read_bytes().startswith(b"\xff\xd8\xff")
    assert waveform.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
    assert proxy.read_bytes()[4:8] == b"ftyp"
    assert audio.read_bytes().startswith(b"fLaC")
    assert result["analysis"]["version"] == 1
    assert len(result["analysis"]["representativeFrameTimesSeconds"]) == 12
    assert result["analysis"]["sceneThreshold"] == 0.35
    assert isinstance(result["analysis"]["sceneChangeTimesSeconds"], list)
    assert events[-1].type == "result"


def test_provider_video_is_normalized_to_editable_h264_aac(tmp_path: Path) -> None:
    data_root = tmp_path / "data"
    media = data_root / "tmp" / "provider-normalization"
    media.mkdir(parents=True)
    source = media / "source.webm"
    subprocess.run(
        [
            "ffmpeg",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:sample_rate=44100",
            "-t",
            "0.5",
            "-c:v",
            "libvpx-vp9",
            "-c:a",
            "libopus",
            str(source),
        ],
        check=True,
    )
    request = JobRequest.from_dict(
        {
            "jobId": "provider-normalize-fixture",
            "kind": "normalize_generated_media",
            "projectId": "fixture",
            "inputPath": str(source),
            "outputDir": "derived/provider-normalized",
            "options": {"requestedKind": "video"},
        }
    )
    result = JobRunner(data_root=data_root, emit=lambda _event: None).run(request)
    normalized = Path(result["normalizedPath"])
    assert normalized.read_bytes()[4:8] == b"ftyp"
    assert result["mimeType"] == "video/mp4"
    assert result["normalization"] == "ffmpeg-h264-aac-v1"
    probed = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type,sample_rate,pix_fmt",
            "-of",
            "json",
            str(normalized),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    import json

    streams = json.loads(probed.stdout)["streams"]
    assert streams[0]["codec_name"] == "h264"
    assert streams[0]["pix_fmt"] == "yuv420p"
    assert streams[1]["codec_name"] == "aac"
    assert streams[1]["sample_rate"] == "48000"
