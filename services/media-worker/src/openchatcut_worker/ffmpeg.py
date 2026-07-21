from __future__ import annotations

import json
import hashlib
import math
import os
import re
import shutil
import subprocess
import uuid
from pathlib import Path
from typing import Any, Callable

from .errors import CapabilityUnavailable, WorkerError
from .hardware import h264_encoder_arguments
from .security import resolve_under_root, safe_output_path, sanitized_environment


def require_binary(name: str) -> str:
    path = shutil.which(name)
    if not path:
        raise CapabilityUnavailable(name, f"Install {name} and ensure it is available on PATH")
    return path


def inspect_media(path: Path) -> dict[str, Any]:
    ffprobe = require_binary("ffprobe")
    command = [
        ffprobe,
        "-v",
        "error",
        "-show_streams",
        "-show_format",
        "-of",
        "json",
        str(path),
    ]
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        timeout=60,
        env=sanitized_environment(),
    )
    if completed.returncode != 0:
        raise WorkerError(
            "MEDIA_INSPECTION_FAILED",
            completed.stderr.strip() or "ffprobe could not inspect the media",
        )
    return json.loads(completed.stdout)


def prepare_media(
    *,
    source: Path,
    asset_kind: str,
    thumbnail: Path,
    contact_sheet: Path,
    waveform: Path,
    proxy: Path,
    extracted_audio: Path,
) -> dict[str, Any]:
    """Create deterministic, local editing derivatives with bounded commands."""

    if asset_kind not in ("video", "audio", "image"):
        raise WorkerError("INVALID_ASSET_KIND", "Media preparation supports video, audio, or image")
    ffmpeg = require_binary("ffmpeg")
    inspected = inspect_media(source)
    streams = inspected.get("streams")
    if not isinstance(streams, list):
        raise WorkerError("MEDIA_INSPECTION_FAILED", "ffprobe returned no streams")
    has_video = any(
        isinstance(stream, dict) and stream.get("codec_type") == "video"
        for stream in streams
    )
    has_audio = any(
        isinstance(stream, dict) and stream.get("codec_type") == "audio"
        for stream in streams
    )
    duration = _inspected_duration(inspected)
    result: dict[str, Any] = {
        "assetKind": asset_kind,
        "hasVideo": has_video,
        "hasAudio": has_audio,
    }

    if asset_kind in ("video", "image") and has_video:
        seek = min(10.0, max(0.0, duration * 0.1)) if asset_kind == "video" else 0.0
        command = [ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error", "-y"]
        if seek > 0:
            command.extend(["-ss", f"{seek:.6f}"])
        command.extend(
            [
                "-i",
                str(source),
                "-frames:v",
                "1",
                "-vf",
                "scale=640:640:force_original_aspect_ratio=decrease:force_divisible_by=2",
                "-q:v",
                "3",
                str(thumbnail),
            ]
        )
        _run_derivative_command(command, thumbnail, "THUMBNAIL_GENERATION_FAILED")
        result["thumbnailPath"] = str(thumbnail)

    if has_audio:
        command = [
            ffmpeg,
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(source),
            "-filter_complex",
            "aformat=channel_layouts=mono,showwavespic=s=1600x240:colors=0x55E6C1",
            "-frames:v",
            "1",
            str(waveform),
        ]
        _run_derivative_command(command, waveform, "WAVEFORM_GENERATION_FAILED")
        result["waveformPath"] = str(waveform)

    if asset_kind == "video" and has_video:
        representative_times = _representative_frame_times(duration)
        frames_per_second = 12.0 / duration if duration > 0 else 0.1
        contact_filter = (
            f"fps={frames_per_second:.9f},"
            "scale=320:180:force_original_aspect_ratio=decrease:force_divisible_by=2,"
            "pad=320:180:(ow-iw)/2:(oh-ih)/2:color=black,"
            "tile=4x3:nb_frames=12:padding=4:margin=4:color=black"
        )
        proxy_encoding, proxy_encoder_args = h264_encoder_arguments(
            quality=25, workload="realtime"
        )
        command = [
            ffmpeg,
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(source),
            "-an",
            "-frames:v",
            "1",
            "-vf",
            contact_filter,
            "-q:v",
            "3",
            str(contact_sheet),
        ]
        _run_derivative_command(
            command,
            contact_sheet,
            "CONTACT_SHEET_GENERATION_FAILED",
            timeout=60 * 60,
        )
        result["contactSheetPath"] = str(contact_sheet)
        result["analysis"] = {
            "version": 1,
            "durationSeconds": duration,
            "representativeFrameTimesSeconds": representative_times,
            "sceneChangeTimesSeconds": _detect_scene_changes(source, ffmpeg),
            "sceneThreshold": 0.35,
        }

        command = [
            ffmpeg,
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(source),
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-vf",
            "scale=1280:720:force_original_aspect_ratio=decrease:force_divisible_by=2",
            *proxy_encoder_args,
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-movflags",
            "+faststart",
            str(proxy),
        ]
        _run_derivative_command(command, proxy, "PROXY_GENERATION_FAILED", timeout=24 * 60 * 60)
        result["proxyPath"] = str(proxy)
        result["proxyVideoEncoding"] = proxy_encoding

        if has_audio:
            command = [
                ffmpeg,
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-i",
                str(source),
                "-vn",
                "-c:a",
                "flac",
                "-sample_fmt",
                "s16",
                str(extracted_audio),
            ]
            _run_derivative_command(
                command,
                extracted_audio,
                "AUDIO_EXTRACTION_FAILED",
                timeout=24 * 60 * 60,
            )
            result["extractedAudioPath"] = str(extracted_audio)

    if len(result) == 3:
        raise WorkerError("NO_MEDIA_DERIVATIVES", "The source has no usable audio or video stream")
    return result


def _representative_frame_times(duration: float, count: int = 12) -> list[float]:
    if not 0 < duration <= 7 * 24 * 60 * 60:
        return []
    return [round((index + 0.5) * duration / count, 6) for index in range(count)]


def _detect_scene_changes(source: Path, ffmpeg: str) -> list[float]:
    """Return a bounded list of scene-cut timestamps without exporting frames."""

    command = [
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "info",
        "-i",
        str(source),
        "-map",
        "0:v:0",
        "-an",
        "-vf",
        "select=gt(scene\\,0.35),showinfo",
        "-frames:v",
        "200",
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
            timeout=60 * 60,
            env=sanitized_environment(),
        )
    except (subprocess.SubprocessError, OSError):
        return []
    if completed.returncode != 0:
        return []
    values: list[float] = []
    for match in re.finditer(r"\bpts_time:([0-9]+(?:\.[0-9]+)?)", completed.stderr):
        value = float(match.group(1))
        if values and value <= values[-1]:
            continue
        values.append(round(value, 6))
        if len(values) == 200:
            break
    return values


def _inspected_duration(inspected: dict[str, Any]) -> float:
    raw = inspected.get("format", {}).get("duration")
    try:
        duration = float(raw)
    except (TypeError, ValueError):
        return 0.0
    return duration if 0 < duration <= 7 * 24 * 60 * 60 else 0.0


def _run_derivative_command(
    command: list[str],
    destination: Path,
    error_code: str,
    *,
    timeout: int = 60 * 60,
) -> None:
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        timeout=timeout,
        env=sanitized_environment(),
    )
    if completed.returncode != 0:
        destination.unlink(missing_ok=True)
        raise WorkerError(
            error_code,
            completed.stderr.strip() or "FFmpeg media derivative generation failed",
        )


def audio_duration_seconds(path: Path) -> float:
    inspected = inspect_media(path)
    duration = inspected.get("format", {}).get("duration")
    try:
        value = float(duration)
    except (TypeError, ValueError) as error:
        raise WorkerError(
            "AUDIO_DURATION_UNAVAILABLE",
            "ffprobe did not return a usable audio duration",
        ) from error
    if not 0 < value <= 86_400:
        raise WorkerError(
            "AUDIO_DURATION_UNAVAILABLE",
            "audio duration is outside the supported range",
        )
    return value


def process_audio(*, source: Path, destination: Path, filter_graph: str) -> None:
    ffmpeg = require_binary("ffmpeg")
    command = [
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        str(source),
        "-vn",
        "-af",
        filter_graph,
        "-c:a",
        "pcm_s24le",
        str(destination),
    ]
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        timeout=60 * 60,
        env=sanitized_environment(),
    )
    if completed.returncode != 0:
        destination.unlink(missing_ok=True)
        raise WorkerError(
            "AUDIO_PROCESSING_FAILED",
            completed.stderr.strip() or "ffmpeg audio processing failed",
        )


def normalize_generated_media(
    *, source: Path, destination: Path, requested_kind: str
) -> dict[str, Any]:
    """Transcode a provider result into one stable, editable local format."""

    if requested_kind not in ("video", "image", "voice", "music", "sfx"):
        raise WorkerError("INVALID_GENERATED_KIND", "Unsupported generated media kind")
    inspected = inspect_media(source)
    streams = inspected.get("streams")
    if not isinstance(streams, list):
        raise WorkerError("MEDIA_INSPECTION_FAILED", "ffprobe returned no streams")
    video_streams = [
        stream
        for stream in streams
        if isinstance(stream, dict) and stream.get("codec_type") == "video"
    ]
    audio_streams = [
        stream
        for stream in streams
        if isinstance(stream, dict) and stream.get("codec_type") == "audio"
    ]
    ffmpeg = require_binary("ffmpeg")
    video_encoding = None
    base = [
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        str(source),
    ]
    if requested_kind == "video":
        if not video_streams:
            raise WorkerError("GENERATED_MEDIA_TYPE_MISMATCH", "Provider output has no video")
        video_encoding, video_encoder_args = h264_encoder_arguments(quality=18)
        command = [
            *base,
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            *video_encoder_args,
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-ar",
            "48000",
            "-movflags",
            "+faststart",
            str(destination),
        ]
        mime_type = "video/mp4"
        normalization = "ffmpeg-h264-aac-v1"
    elif requested_kind == "image":
        if not video_streams:
            raise WorkerError("GENERATED_MEDIA_TYPE_MISMATCH", "Provider output has no image")
        width = _bounded_stream_dimension(video_streams[0].get("width"), "width")
        height = _bounded_stream_dimension(video_streams[0].get("height"), "height")
        command = [
            *base,
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-c:v",
            "png",
            str(destination),
        ]
        mime_type = "image/png"
        normalization = "ffmpeg-png-v1"
    else:
        if not audio_streams:
            raise WorkerError("GENERATED_MEDIA_TYPE_MISMATCH", "Provider output has no audio")
        command = [
            *base,
            "-map",
            "0:a:0",
            "-vn",
            "-c:a",
            "pcm_s24le",
            "-ar",
            "48000",
            str(destination),
        ]
        mime_type = "audio/wav"
        normalization = "ffmpeg-pcm-s24le-48k-v1"
        width = None
        height = None
    _run_derivative_command(
        command,
        destination,
        "GENERATED_MEDIA_NORMALIZATION_FAILED",
        timeout=24 * 60 * 60,
    )
    return {
        "normalizedPath": str(destination),
        "requestedKind": requested_kind,
        "mimeType": mime_type,
        "normalization": normalization,
        "width": width if requested_kind == "image" else None,
        "height": height if requested_kind == "image" else None,
        "hasAudio": bool(audio_streams),
        "videoEncoding": video_encoding,
    }


def _bounded_stream_dimension(value: Any, field: str) -> int:
    try:
        result = int(value)
    except (TypeError, ValueError) as error:
        raise WorkerError(
            "GENERATED_MEDIA_DIMENSIONS_INVALID", f"Generated image {field} is invalid"
        ) from error
    if not 1 <= result <= 16_384:
        raise WorkerError(
            "GENERATED_MEDIA_DIMENSIONS_INVALID",
            f"Generated image {field} exceeds 16384 pixels",
        )
    return result


def process_audio_pair(
    *,
    source: Path,
    secondary: Path,
    destination: Path,
    filter_graph: str,
) -> None:
    ffmpeg = require_binary("ffmpeg")
    command = [
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        str(source),
        "-i",
        str(secondary),
        "-filter_complex",
        filter_graph,
        "-map",
        "[out]",
        "-vn",
        "-c:a",
        "pcm_s24le",
        str(destination),
    ]
    _run_audio_command(command, destination)


def loop_audio(
    *,
    source: Path,
    destination: Path,
    duration_seconds: float,
    fade_seconds: float,
) -> None:
    ffmpeg = require_binary("ffmpeg")
    filters = [f"atrim=duration={duration_seconds:.9f}"]
    if fade_seconds > 0:
        filters.extend(
            [
                f"afade=t=in:st=0:d={fade_seconds:.9f}",
                f"afade=t=out:st={max(0.0, duration_seconds - fade_seconds):.9f}:d={fade_seconds:.9f}",
            ]
        )
    command = [
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-stream_loop",
        "-1",
        "-i",
        str(source),
        "-vn",
        "-af",
        ",".join(filters),
        "-t",
        f"{duration_seconds:.9f}",
        "-c:a",
        "pcm_s24le",
        str(destination),
    ]
    _run_audio_command(command, destination)


def _run_audio_command(command: list[str], destination: Path) -> None:
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        timeout=60 * 60,
        env=sanitized_environment(),
    )
    if completed.returncode != 0:
        destination.unlink(missing_ok=True)
        raise WorkerError(
            "AUDIO_PROCESSING_FAILED",
            completed.stderr.strip() or "ffmpeg audio processing failed",
        )


def render_export(
    *,
    source: Path,
    destination: Path,
    options: dict[str, Any],
    progress: Callable[[float, str], None] | None = None,
) -> dict[str, Any]:
    """Execute a daemon-validated, single-source export plan atomically."""

    ffmpeg = require_binary("ffmpeg")
    plan = options.get("plan")
    if not isinstance(plan, dict) or plan.get("renderer") != "ffmpeg-single-source-v1":
        raise WorkerError("INVALID_EXPORT_PLAN", "A validated single-source export plan is required")
    source_plan = plan.get("source")
    fps = plan.get("fps")
    if not isinstance(source_plan, dict) or not isinstance(fps, dict):
        raise WorkerError("INVALID_EXPORT_PLAN", "Export source and frame rate are required")

    format_name = str(plan.get("format", ""))
    ticks_per_second = _positive_int(plan.get("ticksPerSecond"), "ticksPerSecond")
    source_start_ticks = _non_negative_int(source_plan.get("sourceStartTicks"), "sourceStartTicks")
    duration_ticks = _positive_int(plan.get("durationTicks"), "durationTicks")
    width = _positive_int(plan.get("width"), "width")
    height = _positive_int(plan.get("height"), "height")
    fps_numerator = _positive_int(fps.get("numerator"), "fps.numerator")
    fps_denominator = _positive_int(fps.get("denominator"), "fps.denominator")
    has_audio = bool(source_plan.get("hasAudio", False))
    allow_overwrite = bool(options.get("allowOverwrite", False))
    video_encoding = None

    if destination.exists() and not allow_overwrite:
        raise WorkerError(
            "EXPORT_OUTPUT_EXISTS",
            "The export output already exists and overwrite was not approved",
            details={"outputPath": str(destination)},
        )

    temporary = destination.with_name(
        f".{destination.stem}.{uuid.uuid4().hex}.part{destination.suffix}"
    )
    temporary.unlink(missing_ok=True)
    command = [
        ffmpeg,
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        str(source),
        "-ss",
        _seconds(source_start_ticks, ticks_per_second),
        "-t",
        _seconds(duration_ticks, ticks_per_second),
    ]

    if format_name in ("mp4", "webm", "prores-4444", "png"):
        video_filter = (
            f"scale={width}:{height}:force_original_aspect_ratio=decrease,"
            f"pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black,"
            f"fps={fps_numerator}/{fps_denominator},setsar=1"
        )
        command.extend(["-map", "0:v:0", "-vf", video_filter])
    if format_name == "mp4":
        video_encoding, video_encoder_args = h264_encoder_arguments(quality=18)
        command.extend([*video_encoder_args, "-pix_fmt", "yuv420p"])
        if has_audio:
            command.extend(["-map", "0:a:0?", "-c:a", "aac", "-b:a", "192k", "-ar", "48000"])
        else:
            command.append("-an")
        command.extend(["-movflags", "+faststart"])
    elif format_name == "webm":
        command.extend(["-c:v", "libvpx-vp9", "-crf", "24", "-b:v", "0", "-pix_fmt", "yuv420p"])
        if has_audio:
            command.extend(["-map", "0:a:0?", "-c:a", "libopus", "-b:a", "160k"])
        else:
            command.append("-an")
    elif format_name == "prores-4444":
        command.extend(["-c:v", "prores_ks", "-profile:v", "4", "-pix_fmt", "yuva444p10le"])
        if has_audio:
            command.extend(["-map", "0:a:0?", "-c:a", "pcm_s24le"])
        else:
            command.append("-an")
    elif format_name == "png":
        command.extend(["-frames:v", "1", "-an", "-c:v", "png"])
    elif format_name == "wav":
        if not has_audio:
            raise WorkerError("EXPORT_SOURCE_HAS_NO_AUDIO", "The selected source has no audio stream")
        command.extend(["-map", "0:a:0", "-vn", "-c:a", "pcm_s24le", "-ar", "48000"])
    elif format_name == "mp3":
        if not has_audio:
            raise WorkerError("EXPORT_SOURCE_HAS_NO_AUDIO", "The selected source has no audio stream")
        command.extend(["-map", "0:a:0", "-vn", "-c:a", "libmp3lame", "-q:a", "2", "-ar", "48000"])
    else:
        raise WorkerError("UNSUPPORTED_EXPORT_FORMAT", f"Unsupported export format: {format_name}")
    # FFmpeg writes machine-readable progress to stdout while diagnostics stay
    # on stderr. Keeping this inside the worker preserves the JSON-lines daemon
    # protocol and gives durable jobs useful progress during long encodes.
    command.extend(["-progress", "pipe:1", "-nostats", str(temporary)])

    try:
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            env=sanitized_environment(),
        )
        assert process.stdout is not None
        assert process.stderr is not None
        duration_seconds = duration_ticks / ticks_per_second
        for line in process.stdout:
            key, separator, raw_value = line.strip().partition("=")
            if separator and key == "out_time_us" and progress is not None:
                try:
                    encoded_seconds = max(0.0, int(raw_value) / 1_000_000)
                except ValueError:
                    continue
                ratio = min(1.0, encoded_seconds / duration_seconds)
                progress(
                    0.05 + ratio * 0.9,
                    f"Encoding {min(encoded_seconds, duration_seconds):.1f}s / "
                    f"{duration_seconds:.1f}s",
                )
        stderr = process.stderr.read()
        return_code = process.wait(timeout=24 * 60 * 60)
        if return_code != 0:
            raise WorkerError(
                "EXPORT_ENCODING_FAILED",
                stderr.strip() or "FFmpeg could not encode the export",
            )
        _install_export(temporary=temporary, destination=destination, overwrite=allow_overwrite)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise

    return {
        "outputPath": str(destination),
        "byteSize": destination.stat().st_size,
        "sha256": _sha256_file(destination),
        "format": format_name,
        "renderer": plan["renderer"],
        "durationTicks": duration_ticks,
        "ticksPerSecond": ticks_per_second,
        "videoEncoding": video_encoding,
    }


def render_timeline_audio_export(
    *,
    data_root: Path,
    output_dir: Path,
    options: dict[str, Any],
    progress: Any,
) -> dict[str, Any]:
    """Mix a daemon-validated, revision-pinned audio timeline with FFmpeg."""

    ffmpeg = require_binary("ffmpeg")
    plan = options.get("plan")
    revision = options.get("revision")
    document_hash = options.get("documentHash")
    output_name = options.get("outputFileName")
    overwrite = options.get("allowOverwrite") is True
    if not isinstance(plan, dict) or plan.get("renderer") != "ffmpeg-timeline-audio-v1":
        raise WorkerError("INVALID_EXPORT_PLAN", "A validated timeline audio plan is required")
    if type(revision) is not int or revision < 0:
        raise WorkerError("INVALID_EXPORT_PLAN", "Pinned revision is required")
    if not isinstance(document_hash, str) or not document_hash:
        raise WorkerError("INVALID_EXPORT_PLAN", "Pinned document hash is required")
    if not isinstance(output_name, str):
        raise WorkerError("INVALID_OUTPUT_NAME", "Export outputFileName is required")

    format_name = plan.get("format")
    if format_name not in ("wav", "mp3"):
        raise WorkerError("INVALID_EXPORT_PLAN", "Timeline audio format must be WAV or MP3")
    ticks_per_second = _positive_int(plan.get("ticksPerSecond"), "ticksPerSecond")
    duration_ticks = _positive_int(plan.get("durationTicks"), "durationTicks")
    timeline_start_ticks = _non_negative_int(
        plan.get("timelineStartTicks"), "timelineStartTicks"
    )
    duration_seconds = duration_ticks / ticks_per_second
    if duration_seconds > 24 * 60 * 60:
        raise WorkerError("EXPORT_DURATION_LIMIT", "Audio export exceeds the 24-hour safety limit")

    raw_inputs = options.get("audioInputs")
    if not isinstance(raw_inputs, list) or not 1 <= len(raw_inputs) <= 256:
        raise WorkerError("INVALID_EXPORT_PLAN", "Audio input list must contain 1 to 256 clips")
    planned_inputs = plan.get("audioSources")
    if not isinstance(planned_inputs, list) or len(planned_inputs) != len(raw_inputs):
        raise WorkerError("INVALID_EXPORT_PLAN", "Audio inputs do not match the pinned plan")
    audio_inputs: list[dict[str, Any]] = []
    authoritative_fields = (
        "assetId",
        "timelineStartTicks",
        "sourceStartTicks",
        "durationTicks",
        "playbackRate",
        "gain",
        "fadeInTicks",
        "fadeOutTicks",
        "fadeCurve",
    )
    for index, value in enumerate(raw_inputs):
        if not isinstance(value, dict) or not isinstance(value.get("inputPath"), str):
            raise WorkerError("INVALID_EXPORT_PLAN", "Audio input is invalid")
        expected = planned_inputs[index]
        if not isinstance(expected, dict) or any(
            expected.get(field) != value.get(field) for field in authoritative_fields
        ):
            raise WorkerError("INVALID_EXPORT_PLAN", "Audio input timing differs from the pinned plan")
        timeline_ticks = _non_negative_int(
            value.get("timelineStartTicks"), "audio.timelineStartTicks"
        )
        source_ticks = _non_negative_int(
            value.get("sourceStartTicks"), "audio.sourceStartTicks"
        )
        clip_ticks = _positive_int(value.get("durationTicks"), "audio.durationTicks")
        if timeline_ticks + clip_ticks > duration_ticks:
            raise WorkerError("INVALID_EXPORT_PLAN", "Audio clip exceeds the pinned export range")
        rate = value.get("playbackRate")
        gain = value.get("gain")
        if (
            isinstance(rate, bool)
            or not isinstance(rate, (int, float))
            or not math.isfinite(rate)
            or not 0.05 <= rate <= 16
            or isinstance(gain, bool)
            or not isinstance(gain, (int, float))
            or not math.isfinite(gain)
            or not 0 <= gain <= 10
        ):
            raise WorkerError("INVALID_EXPORT_PLAN", "Audio rate or gain is invalid")
        fade_in = _non_negative_int(value.get("fadeInTicks"), "audio.fadeInTicks")
        fade_out = _non_negative_int(value.get("fadeOutTicks"), "audio.fadeOutTicks")
        if (
            fade_in * 2 > clip_ticks
            or fade_out * 2 > clip_ticks
            or value.get("fadeCurve") != "equalPower"
        ):
            raise WorkerError("INVALID_EXPORT_PLAN", "Audio fade is invalid")
        audio_inputs.append(
            {
                "path": resolve_under_root(value=value["inputPath"], root=data_root),
                "timelineStartTicks": timeline_ticks,
                "sourceStartTicks": source_ticks,
                "durationTicks": clip_ticks,
                "playbackRate": float(rate),
                "gain": float(gain),
                "fadeInTicks": fade_in,
                "fadeOutTicks": fade_out,
            }
        )

    destination = safe_output_path(output_dir=output_dir, file_name=output_name)
    if destination.exists() and not overwrite:
        raise WorkerError(
            "EXPORT_OUTPUT_EXISTS",
            "The export output already exists and overwrite was not approved",
            details={"outputPath": str(destination)},
        )
    temporary = destination.with_name(
        f".{destination.stem}.{uuid.uuid4().hex}.part{destination.suffix}"
    )
    temporary.unlink(missing_ok=True)
    command = [ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error", "-y"]
    for audio in audio_inputs:
        source_duration = (
            audio["durationTicks"] / ticks_per_second * audio["playbackRate"]
        )
        command.extend(
            [
                "-ss",
                f"{audio['sourceStartTicks'] / ticks_per_second:.9f}",
                "-t",
                f"{source_duration:.9f}",
                "-i",
                str(audio["path"]),
            ]
        )

    chains: list[str] = []
    labels: list[str] = []
    for index, audio in enumerate(audio_inputs):
        clip_seconds = audio["durationTicks"] / ticks_per_second
        filters = ["asetpts=PTS-STARTPTS", *_audio_atempo_chain(audio["playbackRate"])]
        fade_in_seconds = audio["fadeInTicks"] / ticks_per_second
        fade_out_seconds = audio["fadeOutTicks"] / ticks_per_second
        if fade_in_seconds > 0:
            filters.append(f"afade=t=in:st=0:d={fade_in_seconds:.9f}:curve=qsin")
        if fade_out_seconds > 0:
            fade_start = max(0.0, clip_seconds - fade_out_seconds)
            filters.append(
                f"afade=t=out:st={fade_start:.9f}:d={fade_out_seconds:.9f}:curve=qsin"
            )
        delay_samples = round(audio["timelineStartTicks"] / ticks_per_second * 48_000)
        filters.extend(
            [
                f"volume={audio['gain']:.9g}",
                f"atrim=duration={clip_seconds:.9f}",
                "aresample=48000",
                f"adelay={delay_samples}S:all=1",
            ]
        )
        label = f"a{index}"
        chains.append(f"[{index}:a]{','.join(filters)}[{label}]")
        labels.append(f"[{label}]")
    chains.append(
        f"{''.join(labels)}amix=inputs={len(labels)}:duration=longest:normalize=0,"
        f"apad=whole_dur={duration_seconds:.9f},atrim=duration={duration_seconds:.9f},"
        "aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[aout]"
    )
    command.extend(["-filter_complex", ";".join(chains), "-map", "[aout]", "-vn"])
    if format_name == "wav":
        command.extend(["-c:a", "pcm_s24le", "-ar", "48000"])
    else:
        command.extend(["-c:a", "libmp3lame", "-q:a", "2", "-ar", "48000"])
    command.extend(["-t", f"{duration_seconds:.9f}", str(temporary)])

    progress(0.08, "Mixing pinned audio timeline")
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=24 * 60 * 60,
            env=sanitized_environment(),
        )
        if completed.returncode != 0:
            raise WorkerError(
                "EXPORT_ENCODING_FAILED",
                completed.stderr.strip() or "FFmpeg could not mix the audio timeline",
            )
        _install_export(temporary=temporary, destination=destination, overwrite=overwrite)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise
    progress(0.98, "Finalizing audio export")
    return {
        "outputPath": str(destination),
        "byteSize": destination.stat().st_size,
        "sha256": _sha256_file(destination),
        "format": format_name,
        "renderer": plan["renderer"],
        "revision": revision,
        "documentHash": document_hash,
        "timelineStartTicks": timeline_start_ticks,
        "durationTicks": duration_ticks,
        "ticksPerSecond": ticks_per_second,
        "audioSourceCount": len(audio_inputs),
    }


def _audio_atempo_chain(rate: float) -> list[str]:
    filters: list[str] = []
    remaining = rate
    while remaining > 2.0:
        filters.append("atempo=2")
        remaining /= 2.0
    while remaining < 0.5:
        filters.append("atempo=0.5")
        remaining /= 0.5
    filters.append(f"atempo={remaining:.9g}")
    return filters


def _install_export(*, temporary: Path, destination: Path, overwrite: bool) -> None:
    if overwrite:
        os.replace(temporary, destination)
        return
    try:
        os.link(temporary, destination)
    except FileExistsError as error:
        raise WorkerError(
            "EXPORT_OUTPUT_EXISTS",
            "The export output appeared while encoding and overwrite was not approved",
            details={"outputPath": str(destination)},
        ) from error
    temporary.unlink()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def _positive_int(value: Any, field: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise WorkerError("INVALID_EXPORT_PLAN", f"{field} must be a positive integer")
    return value


def _non_negative_int(value: Any, field: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise WorkerError("INVALID_EXPORT_PLAN", f"{field} must be a non-negative integer")
    return value


def _seconds(ticks: int, ticks_per_second: int) -> str:
    return f"{ticks / ticks_per_second:.9f}"
