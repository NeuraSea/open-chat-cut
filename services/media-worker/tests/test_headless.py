from pathlib import Path
from contextlib import contextmanager
import json
import shutil
import struct
import subprocess
import zlib

import pytest

from openchatcut_worker.errors import CapabilityUnavailable, WorkerError
from openchatcut_worker import headless
from openchatcut_worker.headless import (
    _is_allowed_browser_url,
    _loopback_http_origin,
    _png_dimensions,
    capture_web_page,
    render_headless_export,
)
from openchatcut_worker.ffmpeg import render_timeline_audio_export


def _rgba_test_png(width: int, height: int) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    opaque = bytes((255, 48, 24, 255))
    transparent = bytes((0, 0, 0, 0))
    row = b"\x00" + opaque * (width // 2) + transparent * (width - width // 2)
    pixels = row * height
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(pixels))
        + chunk(b"IEND", b"")
    )


def test_headless_renderer_only_accepts_loopback_origins() -> None:
    assert _loopback_http_origin("http://127.0.0.1:3100") == "http://127.0.0.1:3100"
    assert _loopback_http_origin("http://localhost:3100/") == "http://localhost:3100"
    assert _loopback_http_origin("http://[::1]:3100") == "http://[::1]:3100"
    for value in (
        "https://127.0.0.1:3100",
        "http://example.com:3100",
        "http://127.0.0.1:3100/redirect",
        "http://user:pass@127.0.0.1:3100",
    ):
        with pytest.raises(WorkerError):
            _loopback_http_origin(value)


def test_browser_route_blocks_external_network_requests() -> None:
    assert _is_allowed_browser_url("http://localhost:3100/_next/app.js")
    assert _is_allowed_browser_url("http://127.0.0.1:3210/api/v1/status")
    assert _is_allowed_browser_url("blob:http://127.0.0.1:3100/asset")
    assert not _is_allowed_browser_url("https://example.com/tracker")
    assert not _is_allowed_browser_url("file:///etc/passwd")


def test_png_dimensions_reject_non_png_and_reads_ihdr(tmp_path: Path) -> None:
    image = tmp_path / "frame.png"
    image.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\x00" * 8 + (320).to_bytes(4, "big") + (180).to_bytes(4, "big"))
    assert _png_dimensions(image) == (320, 180)
    image.write_bytes(b"not an image")
    with pytest.raises(WorkerError):
        _png_dimensions(image)


def test_web_capture_uses_offline_script_disabled_chromium(tmp_path: Path) -> None:
    source = tmp_path / "page.html"
    destination = tmp_path / "capture.png"
    source.write_text(
        """<!doctype html><html><head><title>Acme</title>
        <meta name="description" content="Private widgets">
        <meta name="theme-color" content="#123456">
        <script>fetch('http://127.0.0.1:9/secret')</script></head>
        <body><h1>Fast widgets</h1><img src="http://127.0.0.1:9/private">
        <button>Buy now</button></body></html>""",
        encoding="utf-8",
    )
    try:
        result = capture_web_page(
            source=source,
            destination=destination,
            source_url="https://example.com/product",
            asset_paths=[],
        )
    except CapabilityUnavailable as error:
        pytest.skip(str(error))
    assert destination.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
    assert result["title"] == "Acme"
    assert result["description"] == "Private widgets"
    assert result["sellingPoints"] == ["Fast widgets", "Buy now"]
    assert "#123456" in result["brandColors"]
    assert result["networkAccess"] == "disabled"
    assert result["javaScriptEnabled"] is False
    assert result["sandboxOrigin"] == "about:blank"


def test_prores_4444_export_preserves_real_alpha_pixels(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    ffmpeg = shutil.which("ffmpeg")
    ffprobe = shutil.which("ffprobe")
    if not ffmpeg or not ffprobe:
        pytest.skip("FFmpeg and ffprobe are required")

    width, height = 64, 36
    png = _rgba_test_png(width, height)

    @contextmanager
    def fake_renderer_page(**_kwargs: object):
        yield object()

    monkeypatch.setattr(headless, "_open_renderer_page", fake_renderer_page)
    monkeypatch.setattr(headless, "_render_canvas_png", lambda *_args, **_kwargs: png)

    options = {
        "editorUrl": "http://127.0.0.1:3100",
        "revision": 9,
        "documentHash": "b" * 64,
        "outputFileName": "alpha.mov",
        "allowOverwrite": False,
        "plan": {
            "renderer": "headless-scene-graph-v1",
            "format": "prores-4444",
            "width": width,
            "height": height,
            "durationTicks": 120_000,
            "timelineStartTicks": 0,
            "ticksPerSecond": 120_000,
            "fps": {"numerator": 2, "denominator": 1},
        },
        "audioInputs": [],
    }
    result = render_headless_export(
        data_root=tmp_path,
        project_id="alpha-fixture",
        output_dir=tmp_path / "exports",
        options=options,
        progress=lambda _value, _message: None,
    )
    output = Path(result["outputPath"])

    probe = json.loads(
        subprocess.check_output(
            [
                ffprobe,
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,profile,pix_fmt,width,height,r_frame_rate",
                "-of",
                "json",
                str(output),
            ],
            text=True,
        )
    )["streams"][0]
    assert probe["codec_name"] == "prores"
    assert "4444" in probe["profile"]
    assert probe["pix_fmt"].startswith("yuva444p")
    assert (probe["width"], probe["height"]) == (width, height)
    assert probe["r_frame_rate"] == "2/1"

    alpha = subprocess.check_output(
        [
            ffmpeg,
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(output),
            "-vf",
            "alphaextract",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "pipe:1",
        ]
    )
    assert min(alpha) <= 1
    assert max(alpha) >= 254
    assert result["renderer"] == "headless-scene-graph-v1"
    assert result["revision"] == 9
    assert result["frameCount"] == 2


def test_timeline_audio_export_mixes_gaps_without_starting_chromium(tmp_path: Path) -> None:
    ffmpeg = shutil.which("ffmpeg")
    ffprobe = shutil.which("ffprobe")
    if not ffmpeg or not ffprobe:
        pytest.skip("FFmpeg and ffprobe are required")
    media = tmp_path / "media"
    media.mkdir()
    for name, frequency in (("first.wav", 440), ("second.wav", 880)):
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
                f"sine=frequency={frequency}:duration=1",
                "-c:a",
                "pcm_s16le",
                str(media / name),
            ],
            check=True,
        )
    ticks = 120_000
    options = {
        "revision": 7,
        "documentHash": "a" * 64,
        "outputFileName": "mixed.wav",
        "allowOverwrite": False,
        "plan": {
            "renderer": "ffmpeg-timeline-audio-v1",
            "format": "wav",
            "timelineStartTicks": 0,
            "durationTicks": 3 * ticks,
            "ticksPerSecond": ticks,
            "audioSources": [
                {
                    "assetId": "first",
                    "timelineStartTicks": 0,
                    "sourceStartTicks": 0,
                    "durationTicks": ticks,
                    "playbackRate": 1.0,
                    "gain": 1.0,
                    "fadeInTicks": 0,
                    "fadeOutTicks": 6_000,
                    "fadeCurve": "equalPower",
                },
                {
                    "assetId": "second",
                    "timelineStartTicks": 2 * ticks,
                    "sourceStartTicks": 0,
                    "durationTicks": ticks,
                    "playbackRate": 1.0,
                    "gain": 0.5,
                    "fadeInTicks": 6_000,
                    "fadeOutTicks": 0,
                    "fadeCurve": "equalPower",
                },
            ],
        },
        "audioInputs": [
            {
                "inputPath": "media/first.wav",
                "assetId": "first",
                "timelineStartTicks": 0,
                "sourceStartTicks": 0,
                "durationTicks": ticks,
                "playbackRate": 1.0,
                "gain": 1.0,
                "fadeInTicks": 0,
                "fadeOutTicks": 6_000,
                "fadeCurve": "equalPower",
            },
            {
                "inputPath": "media/second.wav",
                "assetId": "second",
                "timelineStartTicks": 2 * ticks,
                "sourceStartTicks": 0,
                "durationTicks": ticks,
                "playbackRate": 1.0,
                "gain": 0.5,
                "fadeInTicks": 6_000,
                "fadeOutTicks": 0,
                "fadeCurve": "equalPower",
            },
        ],
    }
    progress: list[tuple[float, str]] = []
    result = render_timeline_audio_export(
        data_root=tmp_path,
        output_dir=tmp_path / "exports",
        options=options,
        progress=lambda value, message: progress.append((value, message)),
    )
    output = Path(result["outputPath"])
    assert output.read_bytes().startswith(b"RIFF")
    probe = json.loads(
        subprocess.check_output(
            [
                ffprobe,
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "json",
                str(output),
            ],
            text=True,
        )
    )
    assert float(probe["format"]["duration"]) == pytest.approx(3.0, abs=0.02)
    assert result["renderer"] == "ffmpeg-timeline-audio-v1"
    assert result["revision"] == 7
    assert result["audioSourceCount"] == 2
    assert progress[-1][0] == pytest.approx(0.98)

    mp3_options = json.loads(json.dumps(options))
    mp3_options["outputFileName"] = "mixed.mp3"
    mp3_options["plan"]["format"] = "mp3"
    mp3_result = render_timeline_audio_export(
        data_root=tmp_path,
        output_dir=tmp_path / "exports",
        options=mp3_options,
        progress=lambda _value, _message: None,
    )
    mp3_output = Path(mp3_result["outputPath"])
    prefix = mp3_output.read_bytes()[:3]
    assert prefix == b"ID3" or prefix[:1] == b"\xff"
    mp3_probe = json.loads(
        subprocess.check_output(
            [
                ffprobe,
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "json",
                str(mp3_output),
            ],
            text=True,
        )
    )
    assert float(mp3_probe["format"]["duration"]) == pytest.approx(3.0, abs=0.1)
