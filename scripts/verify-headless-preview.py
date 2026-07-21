#!/usr/bin/env python3
"""Exercise daemon -> Playwright -> Web scene graph -> verified PNG end to end."""

from __future__ import annotations

import argparse
import io
import json
import math
import re
import struct
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import wave
import zipfile
import zlib
from pathlib import Path
from typing import Any


def request_json(
    *, api_url: str, token: str, method: str, path: str, body: dict[str, Any] | None = None
) -> dict[str, Any]:
    payload = None if body is None else json.dumps(body).encode("utf-8")
    request = urllib.request.Request(
        f"{api_url.rstrip('/')}{path}",
        data=payload,
        method=method,
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        details = error.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"{method} {path} failed with HTTP {error.code}: {details}") from error


def upload_wav(*, api_url: str, token: str, project_id: str, suffix: str) -> dict[str, Any]:
    sample_rate = 48_000
    output = io.BytesIO()
    with wave.open(output, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate)
        samples = bytearray()
        for index in range(sample_rate):
            value = round(math.sin(index * 2 * math.pi * 440 / sample_rate) * 6_000)
            samples.extend(struct.pack("<h", value))
        wav.writeframes(samples)
    query = urllib.parse.urlencode(
        {
            "assetId": "asset-tone",
            "name": "acceptance-tone.wav",
            "durationTicks": 120_000,
            "hasAudio": "true",
        }
    )
    request = urllib.request.Request(
        f"{api_url.rstrip('/')}/projects/{project_id}/media?{query}",
        data=output.getvalue(),
        method="POST",
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "audio/wav",
            "Idempotency-Key": f"upload-tone-{suffix}",
            "X-OpenChatCut-Expected-Revision": "0",
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def png_dimensions(path: Path) -> tuple[int, int]:
    with path.open("rb") as source:
        header = source.read(24)
    if len(header) < 24 or not header.startswith(b"\x89PNG\r\n\x1a\n"):
        raise RuntimeError(f"Preview is not a PNG: {path}")
    return struct.unpack(">II", header[16:24])


def png_dimensions_bytes(data: bytes) -> tuple[int, int]:
    if len(data) < 24 or not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise RuntimeError("PNG sequence entry is not a PNG")
    return struct.unpack(">II", data[16:24])


def png_pixel_rgb(path: Path, x: int, y: int) -> tuple[int, int, int]:
    data = path.read_bytes()
    offset = 8
    idat = bytearray()
    width = height = color_type = bit_depth = 0
    while offset + 12 <= len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        chunk_type = data[offset + 4 : offset + 8]
        chunk = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if chunk_type == b"IHDR":
            width, height, bit_depth, color_type = struct.unpack(">IIBB", chunk[:10])
        elif chunk_type == b"IDAT":
            idat.extend(chunk)
        elif chunk_type == b"IEND":
            break
    channels = {2: 3, 6: 4}.get(color_type)
    if bit_depth != 8 or channels is None or not (0 <= x < width and 0 <= y < height):
        raise RuntimeError(
            f"Unsupported preview PNG for pixel validation: depth={bit_depth}, type={color_type}"
        )
    raw = zlib.decompress(idat)
    stride = width * channels
    rows: list[bytearray] = []
    cursor = 0
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        row = bytearray(raw[cursor : cursor + stride])
        cursor += stride
        previous = rows[-1] if rows else bytearray(stride)
        for index in range(stride):
            left = row[index - channels] if index >= channels else 0
            above = previous[index]
            upper_left = previous[index - channels] if index >= channels else 0
            if filter_type == 1:
                row[index] = (row[index] + left) & 0xFF
            elif filter_type == 2:
                row[index] = (row[index] + above) & 0xFF
            elif filter_type == 3:
                row[index] = (row[index] + ((left + above) // 2)) & 0xFF
            elif filter_type == 4:
                predictor = left + above - upper_left
                pa = abs(predictor - left)
                pb = abs(predictor - above)
                pc = abs(predictor - upper_left)
                nearest = left if pa <= pb and pa <= pc else above if pb <= pc else upper_left
                row[index] = (row[index] + nearest) & 0xFF
            elif filter_type != 0:
                raise RuntimeError(f"Unsupported PNG filter: {filter_type}")
        rows.append(row)
    start = x * channels
    return tuple(rows[y][start : start + 3])  # type: ignore[return-value]


def wait_for_job(*, api_url: str, token: str, job_id: str, deadline: float) -> dict[str, Any]:
    while True:
        job = request_json(
            api_url=api_url,
            token=token,
            method="GET",
            path=f"/jobs/{job_id}",
        )["job"]
        if job["state"] == "succeeded":
            return job
        if job["state"] in ("failed", "cancelled"):
            raise RuntimeError(f"Job {job_id} {job['state']}: {job.get('error')}")
        if time.monotonic() >= deadline:
            raise TimeoutError(f"Job {job_id} did not finish: {job}")
        time.sleep(0.2)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--api-url", default="http://127.0.0.1:3210/api/v1")
    parser.add_argument("--token-path", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=120.0)
    args = parser.parse_args()
    token = args.token_path.read_text(encoding="utf-8").strip()
    suffix = uuid.uuid4().hex
    created = request_json(
        api_url=args.api_url,
        token=token,
        method="POST",
        path="/projects",
        body={"name": "Headless preview acceptance", "idempotencyKey": f"create-{suffix}"},
    )
    project_id = created["envelope"]["document"]["id"]
    uploaded = upload_wav(
        api_url=args.api_url,
        token=token,
        project_id=project_id,
        suffix=suffix,
    )
    if uploaded["revision"] != 1:
        raise RuntimeError(f"Managed audio upload did not create revision 1: {uploaded}")
    committed = request_json(
        api_url=args.api_url,
        token=token,
        method="POST",
        path=f"/projects/{project_id}/transactions",
        body={
            "transactionId": f"timeline-{suffix}",
            "projectId": project_id,
            "baseRevision": 1,
            "idempotencyKey": f"timeline-{suffix}",
            "actor": {"kind": "user", "id": "acceptance"},
            "operations": [
                {
                    "type": "replaceSceneGraph",
                    "currentSceneId": "scene-main",
                    "scenes": [
                        {
                            "id": "scene-main",
                            "name": "Main",
                            "isMain": True,
                            "tracks": [
                                {
                                    "id": "track-text",
                                    "name": "Text",
                                    "kind": "text",
                                    "muted": False,
                                    "hidden": False,
                                    "locked": False,
                                    "items": [
                                        {
                                            "id": "item-title",
                                            "name": "Acceptance title",
                                            "startTicks": 0,
                                            "durationTicks": 120_000,
                                            "enabled": True,
                                            "content": {
                                                "type": "text",
                                                "text": "HEADLESS V1",
                                            },
                                            "classicElement": {
                                                "params": {
                                                    "content": "HEADLESS V1",
                                                    "fontFamily": "Arial",
                                                    "fontSize": 10,
                                                    "fontWeight": 700,
                                                    "color": "#ffffff",
                                                    "textAlign": "center",
                                                    "transform.positionX": 0,
                                                    "transform.positionY": 0,
                                                    "transform.scaleX": 1,
                                                    "transform.scaleY": 1,
                                                    "transform.rotate": 0,
                                                    "opacity": 1,
                                                },
                                                "trimStart": 0,
                                                "trimEnd": 0,
                                            },
                                        }
                                    ],
                                },
                                {
                                    "id": "track-audio",
                                    "name": "Audio",
                                    "kind": "audio",
                                    "muted": False,
                                    "hidden": False,
                                    "locked": False,
                                    "items": [
                                        {
                                            "id": "item-tone",
                                            "name": "Acceptance tone",
                                            "startTicks": 0,
                                            "durationTicks": 120_000,
                                            "sourceRange": {
                                                "inTicks": 0,
                                                "outTicks": 120_000,
                                            },
                                            "sourceDurationTicks": 120_000,
                                            "enabled": True,
                                            "content": {
                                                "type": "media",
                                                "assetId": "asset-tone",
                                                "mediaKind": "audio",
                                            },
                                            "classicElement": {
                                                "params": {"volume": 0, "muted": False},
                                                "sourceType": "upload",
                                                "mediaId": "asset-tone",
                                                "trimStart": 0,
                                                "trimEnd": 0,
                                            },
                                        }
                                    ],
                                },
                            ],
                            "bookmarks": [],
                        }
                    ],
                }
            ],
        },
    )
    revision = committed["envelope"]["revision"]
    motion_graphic = request_json(
        api_url=args.api_url,
        token=token,
        method="POST",
        path="/tools/create_motion_graphic",
        body={
            "idempotencyKey": f"acceptance-mg-{suffix}",
            "arguments": {
                "projectId": project_id,
                "expectedRevision": revision,
                "mode": "dsl",
                "startSeconds": 0,
                "durationSeconds": 1,
                "definition": {
                    "version": 1,
                    "width": 1920,
                    "height": 1080,
                    "durationSeconds": 1,
                    "designStyle": "acceptance-title-card",
                    "background": "#8b1e3f",
                    "nodes": [
                        {
                            "id": "accent",
                            "type": "shape",
                            "shape": "rectangle",
                            "x": 960,
                            "y": 860,
                            "width": 800,
                            "height": 18,
                            "fill": "#ffc857",
                        },
                        {
                            "id": "title",
                            "type": "text",
                            "text": "EDITABLE MG",
                            "x": 960,
                            "y": 540,
                            "fontSize": 120,
                            "fontWeight": 700,
                            "color": "#ffffff",
                            "animations": {
                                "opacity": [
                                    {"time": 0, "value": 0},
                                    {"time": 0.2, "value": 1, "easing": "ease-out"},
                                ]
                            },
                        },
                    ],
                },
            },
        },
    )
    revision = motion_graphic["data"]["revision"]
    queued = request_json(
        api_url=args.api_url,
        token=token,
        method="POST",
        path="/tools/render_preview_frames",
        body={
            "arguments": {
                "projectId": project_id,
                "revision": revision,
                "timesSeconds": [0.25],
                "width": 640,
            }
        },
    )
    job_id = queued["jobId"]
    job = wait_for_job(
        api_url=args.api_url,
        token=token,
        job_id=job_id,
        deadline=time.monotonic() + args.timeout,
    )

    frame = job["output"]["frames"][0]
    frame_path = Path(frame["path"])
    width, height = png_dimensions(frame_path)
    if (width, height) != (640, 360):
        raise RuntimeError(f"Unexpected preview dimensions: {width}x{height}")
    if frame_path.stat().st_size < 1_000:
        raise RuntimeError("Preview PNG is suspiciously small")
    corner = png_pixel_rgb(frame_path, 20, 20)
    expected = (0x8B, 0x1E, 0x3F)
    if any(abs(actual - target) > 8 for actual, target in zip(corner, expected)):
        raise RuntimeError(
            f"Safe motion graphic DSL did not render in the shared scene graph: {corner}"
        )
    output_name = f"headless-{suffix}.mp4"
    export = request_json(
        api_url=args.api_url,
        token=token,
        method="POST",
        path="/tools/start_export",
        body={
            "arguments": {
                "projectId": project_id,
                "expectedRevision": revision,
                "format": "mp4",
                "outputPath": output_name,
                "allowOverwrite": False,
                "settings": {
                    "resolution": {"width": 640, "height": 360},
                    "fps": 10,
                    "range": {"startSeconds": 0, "endSeconds": 1},
                },
            },
            "idempotencyKey": f"headless-export-{suffix}",
        },
    )
    if export["data"]["renderer"] != "headless-scene-graph-v1":
        raise RuntimeError(f"Complex timeline selected the wrong renderer: {export}")
    export_job = wait_for_job(
        api_url=args.api_url,
        token=token,
        job_id=export["jobId"],
        deadline=time.monotonic() + args.timeout,
    )
    output_path = Path(export_job["output"]["outputPath"])
    probe = json.loads(
        subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
                str(output_path),
            ],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    video = next(stream for stream in probe["streams"] if stream["codec_type"] == "video")
    if video["codec_name"] != "h264" or (video["width"], video["height"]) != (640, 360):
        raise RuntimeError(f"Unexpected headless export video stream: {video}")
    if video["avg_frame_rate"] != "10/1":
        raise RuntimeError(f"Unexpected headless export frame rate: {video['avg_frame_rate']}")
    audio = next(stream for stream in probe["streams"] if stream["codec_type"] == "audio")
    if audio["codec_name"] != "aac" or int(audio["sample_rate"]) != 48_000:
        raise RuntimeError(f"Unexpected headless export audio stream: {audio}")
    duration = float(probe["format"]["duration"])
    if not 0.9 <= duration <= 1.1:
        raise RuntimeError(f"Unexpected headless export duration: {duration}")
    volume_probe = subprocess.run(
        [
            "ffmpeg",
            "-hide_banner",
            "-i",
            str(output_path),
            "-map",
            "0:a:0",
            "-af",
            "volumedetect",
            "-f",
            "null",
            "-",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    match = re.search(r"mean_volume:\s*(-?[0-9.]+) dB", volume_probe.stderr)
    if match is None or float(match.group(1)) < -40:
        raise RuntimeError("Headless export audio is missing or effectively silent")

    sequence_name = f"headless-{suffix}-frames.zip"
    sequence_export = request_json(
        api_url=args.api_url,
        token=token,
        method="POST",
        path="/tools/start_export",
        body={
            "arguments": {
                "projectId": project_id,
                "expectedRevision": revision,
                "format": "png-sequence",
                "outputPath": sequence_name,
                "allowOverwrite": False,
                "settings": {
                    "resolution": {"width": 320, "height": 180},
                    "fps": 10,
                    "range": {"startSeconds": 0, "endSeconds": 0.3},
                },
            },
            "idempotencyKey": f"headless-sequence-{suffix}",
        },
    )
    if sequence_export["data"]["renderer"] != "headless-scene-graph-v1":
        raise RuntimeError(f"PNG sequence selected the wrong renderer: {sequence_export}")
    sequence_job = wait_for_job(
        api_url=args.api_url,
        token=token,
        job_id=sequence_export["jobId"],
        deadline=time.monotonic() + args.timeout,
    )
    if sequence_job["output"].get("verified") is not True:
        raise RuntimeError(f"PNG sequence was not verified by the daemon: {sequence_job}")
    sequence_path = Path(sequence_job["output"]["outputPath"])
    with zipfile.ZipFile(sequence_path) as sequence:
        if any(entry.compress_type != zipfile.ZIP_STORED for entry in sequence.infolist()):
            raise RuntimeError("PNG sequence must use stored ZIP entries")
        manifest = json.loads(sequence.read("sequence.json"))
        if (
            manifest.get("format") != "openchatcut-png-sequence"
            or manifest.get("revision") != revision
            or manifest.get("documentHash") != motion_graphic["data"]["documentHash"]
            or manifest.get("frameCount") != 3
            or (manifest.get("width"), manifest.get("height")) != (320, 180)
        ):
            raise RuntimeError(f"Invalid PNG sequence manifest: {manifest}")
        frame_names = [f"frames/frame_{index:06d}.png" for index in range(3)]
        if sorted(name for name in sequence.namelist() if name.endswith(".png")) != frame_names:
            raise RuntimeError(f"PNG sequence frame names are not contiguous: {sequence.namelist()}")
        for frame_name in frame_names:
            if png_dimensions_bytes(sequence.read(frame_name)) != (320, 180):
                raise RuntimeError(f"Unexpected PNG sequence dimensions in {frame_name}")
    print(
        f"headless render acceptance passed: preview={width}x{height}, "
        f"export=h264/aac 640x360 10fps {duration:.3f}s, png-sequence=3x320x180"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
