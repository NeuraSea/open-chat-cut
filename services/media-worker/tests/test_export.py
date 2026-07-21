from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

import pytest

from openchatcut_worker.errors import WorkerError
from openchatcut_worker.protocol import JobRequest
from openchatcut_worker.runner import JobRunner


@pytest.mark.skipif(
    shutil.which("ffmpeg") is None or shutil.which("ffprobe") is None,
    reason="FFmpeg acceptance dependency is unavailable",
)
def test_single_source_mp4_export_is_atomic_and_probeable(tmp_path: Path) -> None:
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
            "testsrc2=size=320x180:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
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
    events = []
    request = JobRequest.from_dict(
        {
            "jobId": "export-fixture",
            "kind": "export",
            "projectId": "fixture",
            "inputPath": "media/source.mp4",
            "outputDir": "exports",
            "options": {
                "outputFileName": "fixture.mp4",
                "allowOverwrite": False,
                "plan": {
                    "renderer": "ffmpeg-single-source-v1",
                    "format": "mp4",
                    "width": 320,
                    "height": 180,
                    "fps": {"numerator": 30, "denominator": 1},
                    "timelineStartTicks": 0,
                    "durationTicks": 120_000,
                    "ticksPerSecond": 120_000,
                    "source": {
                        "assetId": "asset-source",
                        "mediaKind": "video",
                        "sourceStartTicks": 0,
                        "durationTicks": 120_000,
                        "hasAudio": True,
                    },
                },
            },
        }
    )
    result = JobRunner(data_root=data_root, emit=events.append).run(request)
    destination = data_root / "exports" / "fixture.mp4"
    assert destination.is_file()
    assert result["sha256"]
    assert not list(destination.parent.glob("*.part.*"))

    probe = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type,width,height,r_frame_rate,duration:format=duration",
            "-of",
            "json",
            str(destination),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(probe.stdout)
    video = next(stream for stream in metadata["streams"] if stream["codec_type"] == "video")
    audio = next(stream for stream in metadata["streams"] if stream["codec_type"] == "audio")
    assert video["codec_name"] == "h264"
    assert (video["width"], video["height"]) == (320, 180)
    assert video["r_frame_rate"] == "30/1"
    assert audio["codec_name"] == "aac"
    assert float(metadata["format"]["duration"]) == pytest.approx(1.0, abs=1 / 30)
    encoding_progress = [
        event.payload["progress"]
        for event in events
        if event.type == "progress" and event.payload.get("message", "").startswith("Encoding ")
    ]
    assert encoding_progress
    assert all(0.05 <= value <= 0.95 for value in encoding_progress)

    with pytest.raises(WorkerError) as captured:
        JobRunner(data_root=data_root, emit=events.append).run(request)
    assert captured.value.code == "EXPORT_OUTPUT_EXISTS"
