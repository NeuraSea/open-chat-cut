from __future__ import annotations

import json
import importlib.util
import os
import platform
import shutil
import subprocess
from functools import lru_cache
from typing import Any

from .errors import CapabilityUnavailable
from .security import sanitized_environment


VALID_ACCELERATION_PREFERENCES = ("auto", "cpu", "apple", "nvidia")
VERIFIED_ADAPTER_ENV = "OPENCHATCUT_VERIFIED_VIDEO_ADAPTER"


def _module_available(name: str) -> bool:
    try:
        return importlib.util.find_spec(name) is not None
    except (AttributeError, ImportError, ModuleNotFoundError, ValueError):
        return False


def probe_runtime_features() -> dict[str, bool]:
    return {
        "fasterWhisper": _module_available("faster_whisper"),
        "speakerDiarization": _module_available("pyannote.audio"),
        "deepFilterNet": _module_available("df"),
        "playwright": _module_available("playwright.sync_api"),
        "kokoro": _module_available("kokoro"),
        "audioGen": _module_available("audiocraft"),
    }


def acceleration_preference(value: object | None = None) -> str:
    raw = value if isinstance(value, str) else os.environ.get(
        "OPENCHATCUT_VIDEO_ACCELERATION", "auto"
    )
    preference = raw.strip().lower()
    if preference not in VALID_ACCELERATION_PREFERENCES:
        return "auto"
    return preference


@lru_cache(maxsize=4)
def probe_hardware_capabilities(preference: str | None = None) -> dict[str, Any]:
    requested = acceleration_preference(preference)
    ffmpeg = shutil.which("ffmpeg")
    system = platform.system().lower()
    machine = platform.machine().lower()
    encoder_listing = _encoder_listing(ffmpeg) if ffmpeg else ""

    adapters = [
        _probe_adapter(
            ffmpeg=ffmpeg,
            encoder_listing=encoder_listing,
            adapter_id="cpu",
            encoder="libx264",
            supported_platform=True,
        ),
        _probe_adapter(
            ffmpeg=ffmpeg,
            encoder_listing=encoder_listing,
            adapter_id="apple",
            encoder="h264_videotoolbox",
            supported_platform=system == "darwin" and machine in ("arm64", "aarch64"),
        ),
        _probe_adapter(
            ffmpeg=ffmpeg,
            encoder_listing=encoder_listing,
            adapter_id="nvidia",
            encoder="h264_nvenc",
            supported_platform=system in ("linux", "windows"),
        ),
    ]
    by_id = {adapter["id"]: adapter for adapter in adapters}

    candidates = {
        "auto": ("apple", "nvidia", "cpu"),
        "cpu": ("cpu",),
        "apple": ("apple", "cpu"),
        "nvidia": ("nvidia", "cpu"),
    }[requested]
    selected = next(
        (candidate for candidate in candidates if by_id[candidate]["available"]),
        None,
    )
    fallback_reason = None
    if requested in ("apple", "nvidia") and selected != requested:
        fallback_reason = by_id[requested]["reason"]
    if selected is None:
        fallback_reason = fallback_reason or "No verified H.264 encoder is available"

    return {
        "schemaVersion": 1,
        "platform": {"system": system, "machine": machine},
        "ffmpegAvailable": ffmpeg is not None,
        "runtimeFeatures": probe_runtime_features(),
        "videoEncoding": {
            "requested": requested,
            "selected": selected,
            "accelerated": selected in ("apple", "nvidia"),
            "fallbackReason": fallback_reason,
            "adapters": adapters,
        },
    }


def h264_encoder_arguments(
    preference: object | None = None,
    *,
    quality: int = 18,
    workload: str = "quality",
) -> tuple[dict[str, Any], list[str]]:
    requested = acceleration_preference(preference)
    # The daemon performs the expensive encoder smoke test once at startup and
    # injects only its validated result into each isolated worker process. This
    # avoids running several nested FFmpeg probes for every export, which can
    # produce a false negative when the machine is busy. Standalone workers do
    # not receive this variable and retain the full local probe below.
    verified = os.environ.get(VERIFIED_ADAPTER_ENV, "").strip().lower()
    if verified in ("cpu", "apple", "nvidia"):
        selected = verified
        fallback_reason = None
    else:
        capabilities = probe_hardware_capabilities(requested)
        selected = capabilities["videoEncoding"]["selected"]
        fallback_reason = capabilities["videoEncoding"]["fallbackReason"]
    if selected is None:
        raise CapabilityUnavailable(
            "h264-encoding",
            fallback_reason or "Install an FFmpeg build with libx264",
        )
    quality = max(0, min(51, int(quality)))
    if selected == "apple":
        # VideoToolbox's quality scale is inverse to CRF. Keeping allow_sw=0
        # prevents an unavailable hardware path from becoming a silent CPU job.
        vt_quality = max(1, min(100, round(100 - quality * 1.8)))
        arguments = [
            "-c:v",
            "h264_videotoolbox",
            "-allow_sw",
            "0",
            "-q:v",
            str(vt_quality),
        ]
    elif selected == "nvidia":
        preset = "p4" if workload == "realtime" else "p5"
        arguments = [
            "-c:v",
            "h264_nvenc",
            "-preset",
            preset,
            "-tune",
            "hq",
            "-rc",
            "vbr",
            "-cq",
            str(quality),
            "-b:v",
            "0",
        ]
    else:
        preset = "veryfast" if workload == "realtime" else "medium"
        arguments = [
            "-c:v",
            "libx264",
            "-preset",
            preset,
            "-crf",
            str(quality),
        ]
    return {
        "requested": requested,
        "selected": selected,
        "encoder": {
            "cpu": "libx264",
            "apple": "h264_videotoolbox",
            "nvidia": "h264_nvenc",
        }[selected],
        "accelerated": selected in ("apple", "nvidia"),
        "fallbackReason": fallback_reason,
    }, arguments


def capabilities_json(preference: str | None = None) -> str:
    return json.dumps(
        probe_hardware_capabilities(preference),
        ensure_ascii=True,
        separators=(",", ":"),
    )


def _encoder_listing(ffmpeg: str) -> str:
    try:
        completed = subprocess.run(
            [ffmpeg, "-hide_banner", "-encoders"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
            env=sanitized_environment(),
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return completed.stdout if completed.returncode == 0 else ""


def _probe_adapter(
    *,
    ffmpeg: str | None,
    encoder_listing: str,
    adapter_id: str,
    encoder: str,
    supported_platform: bool,
) -> dict[str, Any]:
    if not supported_platform:
        return {
            "id": adapter_id,
            "encoder": encoder,
            "available": False,
            "verified": False,
            "reason": "The current operating system or architecture does not support this adapter",
        }
    if ffmpeg is None:
        return {
            "id": adapter_id,
            "encoder": encoder,
            "available": False,
            "verified": False,
            "reason": "FFmpeg is not installed",
        }
    if encoder not in encoder_listing:
        return {
            "id": adapter_id,
            "encoder": encoder,
            "available": False,
            "verified": False,
            "reason": f"FFmpeg does not include {encoder}",
        }

    command = [
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=64x64:r=1:d=1",
        "-frames:v",
        "1",
        "-an",
        "-c:v",
        encoder,
        "-pix_fmt",
        "yuv420p",
        "-f",
        "null",
        "-",
    ]
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=15,
            env=sanitized_environment(),
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return {
            "id": adapter_id,
            "encoder": encoder,
            "available": False,
            "verified": False,
            "reason": f"Encoder smoke test failed: {error}",
        }
    if completed.returncode != 0:
        detail = completed.stderr.strip().splitlines()
        reason = detail[-1][:300] if detail else "Encoder smoke test failed"
        return {
            "id": adapter_id,
            "encoder": encoder,
            "available": False,
            "verified": False,
            "reason": reason,
        }
    return {
        "id": adapter_id,
        "encoder": encoder,
        "available": True,
        "verified": True,
        "reason": None,
    }
