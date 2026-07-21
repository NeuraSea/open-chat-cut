#!/usr/bin/env python3
"""Run the release-grade 30 s MP4 and ProRes-alpha export acceptance fixture."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "services" / "media-worker" / "src"))

from openchatcut_worker.protocol import JobRequest  # noqa: E402
from openchatcut_worker.runner import JobRunner  # noqa: E402


def command(*args: str) -> None:
    subprocess.run(args, check=True)


def probe(path: Path) -> dict:
    result = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type,profile,pix_fmt,width,height,r_frame_rate,sample_rate,channels,duration:format=duration",
            "-of",
            "json",
            str(path),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


def probe_alpha_range(path: Path) -> tuple[int, int]:
    alpha = subprocess.run(
        [
            "ffmpeg",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(path),
            "-vf",
            "alphaextract",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "pipe:1",
        ],
        check=True,
        capture_output=True,
    ).stdout
    if not alpha:
        raise RuntimeError("FFmpeg returned an empty alpha plane")
    return min(alpha), max(alpha)


def run_export(data_root: Path, request: dict) -> dict:
    return JobRunner(data_root=data_root, emit=lambda event: None).run(
        JobRequest.from_dict(request)
    )


def main() -> int:
    for dependency in ("ffmpeg", "ffprobe"):
        if shutil.which(dependency) is None:
            raise SystemExit(f"{dependency} is required")
    with tempfile.TemporaryDirectory(prefix="openchatcut-export-") as directory:
        data_root = Path(directory) / "data"
        media = data_root / "media"
        media.mkdir(parents=True)
        source = media / "source.mp4"
        command(
            "ffmpeg",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1920x1080:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
            "-t",
            "30",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            str(source),
        )
        common_source = {
            "assetId": "asset-source",
            "mediaKind": "video",
            "sourceStartTicks": 0,
            "durationTicks": 3_600_000,
            "hasAudio": True,
        }
        run_export(
            data_root,
            {
                "jobId": "acceptance-mp4",
                "kind": "export",
                "projectId": "fixture",
                "inputPath": "media/source.mp4",
                "outputDir": "exports",
                "options": {
                    "outputFileName": "fixture-1080p30.mp4",
                    "allowOverwrite": False,
                    "plan": {
                        "renderer": "ffmpeg-single-source-v1",
                        "format": "mp4",
                        "width": 1920,
                        "height": 1080,
                        "fps": {"numerator": 30, "denominator": 1},
                        "timelineStartTicks": 0,
                        "durationTicks": 3_600_000,
                        "ticksPerSecond": 120_000,
                        "source": common_source,
                    },
                },
            },
        )
        mp4 = probe(data_root / "exports" / "fixture-1080p30.mp4")
        video = next(stream for stream in mp4["streams"] if stream["codec_type"] == "video")
        audio = next(stream for stream in mp4["streams"] if stream["codec_type"] == "audio")
        assert video["codec_name"] == "h264"
        assert (video["width"], video["height"]) == (1920, 1080)
        assert video["r_frame_rate"] == "30/1"
        assert audio["codec_name"] == "aac"
        assert abs(float(mp4["format"]["duration"]) - 30.0) <= 1 / 30

        alpha_source = media / "alpha.mov"
        command(
            "ffmpeg",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black@0:size=640x360:rate=30,format=rgba,"
            "drawbox=x=80:y=80:w=240:h=120:color=red@1:t=fill:replace=1",
            "-t",
            "2",
            "-c:v",
            "qtrle",
            str(alpha_source),
        )
        run_export(
            data_root,
            {
                "jobId": "acceptance-prores-alpha",
                "kind": "export",
                "projectId": "fixture",
                "inputPath": "media/alpha.mov",
                "outputDir": "exports",
                "options": {
                    "outputFileName": "fixture-alpha.mov",
                    "allowOverwrite": False,
                    "plan": {
                        "renderer": "ffmpeg-single-source-v1",
                        "format": "prores-4444",
                        "width": 640,
                        "height": 360,
                        "fps": {"numerator": 30, "denominator": 1},
                        "timelineStartTicks": 0,
                        "durationTicks": 240_000,
                        "ticksPerSecond": 120_000,
                        "source": {
                            **common_source,
                            "assetId": "asset-alpha",
                            "durationTicks": 240_000,
                            "hasAudio": False,
                        },
                    },
                },
            },
        )
        prores = probe(data_root / "exports" / "fixture-alpha.mov")["streams"][0]
        assert prores["codec_name"] == "prores"
        assert prores["profile"] == "4444"
        assert prores["pix_fmt"].startswith("yuva")
        assert probe_alpha_range(data_root / "exports" / "fixture-alpha.mov") == (0, 255)
    print("export acceptance passed: 30 s H.264/AAC 1080p30 and ProRes 4444 alpha")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
