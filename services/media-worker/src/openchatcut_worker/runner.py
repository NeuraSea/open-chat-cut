from __future__ import annotations

import json
from pathlib import Path
from typing import Callable

from .denoise import deepfilter_denoise
from .errors import CapabilityUnavailable, WorkerError
from .ffmpeg import (
    audio_duration_seconds,
    inspect_media,
    loop_audio,
    normalize_generated_media,
    prepare_media,
    process_audio,
    process_audio_pair,
    render_export,
    render_timeline_audio_export,
)
from .generate_audio import synthesize_sfx, synthesize_voice
from .headless import capture_web_page, render_headless_export, render_preview_frames
from .protocol import JobRequest, WorkerEvent
from .security import resolve_under_root, safe_output_path
from .transcribe import transcribe


Emit = Callable[[WorkerEvent], None]


class JobRunner:
    def __init__(self, *, data_root: Path, emit: Emit):
        self.data_root = data_root
        self.emit = emit

    def run(self, request: JobRequest) -> dict:
        output_dir = resolve_under_root(
            value=request.output_dir,
            root=self.data_root,
            must_exist=False,
        )
        output_dir.mkdir(parents=True, exist_ok=True)

        self._progress(request, 0.0, "Starting")
        source = None
        if request.kind not in (
            "render_preview_frames",
            "synthesize_voice",
            "synthesize_sfx",
            "timeline_audio_export",
        ):
            source = resolve_under_root(value=request.input_path, root=self.data_root)
        if request.kind == "inspect_media":
            assert source is not None
            result = inspect_media(source)
        elif request.kind == "prepare_media":
            assert source is not None
            result = prepare_media(
                source=source,
                asset_kind=str(request.options.get("assetKind", "")),
                thumbnail=safe_output_path(
                    output_dir=output_dir,
                    file_name=f"{request.job_id}.thumbnail.jpg",
                ),
                contact_sheet=safe_output_path(
                    output_dir=output_dir,
                    file_name=f"{request.job_id}.contact-sheet.jpg",
                ),
                waveform=safe_output_path(
                    output_dir=output_dir,
                    file_name=f"{request.job_id}.waveform.png",
                ),
                proxy=safe_output_path(
                    output_dir=output_dir,
                    file_name=f"{request.job_id}.proxy.mp4",
                ),
                extracted_audio=safe_output_path(
                    output_dir=output_dir,
                    file_name=f"{request.job_id}.audio.flac",
                ),
            )
        elif request.kind == "transcribe":
            assert source is not None
            result = transcribe(
                source=source,
                options=request.options,
                progress=lambda value, message: self._progress(request, value, message),
            )
            destination = safe_output_path(
                output_dir=output_dir,
                file_name=f"{request.job_id}.transcript.json",
            )
            destination.write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")
            result = {"transcriptPath": str(destination), **result}
        elif request.kind == "normalize_generated_media":
            assert source is not None
            requested_kind = str(request.options.get("requestedKind", ""))
            suffix = {
                "video": ".mp4",
                "image": ".png",
                "voice": ".wav",
                "music": ".wav",
                "sfx": ".wav",
            }.get(requested_kind)
            if suffix is None:
                raise WorkerError("INVALID_GENERATED_KIND", "Unsupported generated media kind")
            destination = safe_output_path(
                output_dir=output_dir,
                file_name=f"{request.job_id}{suffix}",
            )
            self._progress(request, 0.1, "Normalizing provider media")
            result = normalize_generated_media(
                source=source,
                destination=destination,
                requested_kind=requested_kind,
            )
            self._progress(request, 0.95, "Verifying normalized provider media")
        elif request.kind == "capture_web_page":
            assert source is not None
            asset_paths = request.options.get("assetPaths", [])
            if not isinstance(asset_paths, list) or len(asset_paths) > 8:
                raise WorkerError(
                    "INVALID_WEB_CAPTURE_REQUEST",
                    "Website capture accepts at most eight staged public assets",
                )
            resolved_assets = [
                resolve_under_root(value=value, root=self.data_root)
                for value in asset_paths
                if isinstance(value, str)
            ]
            if len(resolved_assets) != len(asset_paths):
                raise WorkerError(
                    "INVALID_WEB_CAPTURE_REQUEST",
                    "Every staged public asset path must be a string",
                )
            destination = safe_output_path(
                output_dir=output_dir,
                file_name=f"{request.job_id}.png",
            )
            self._progress(request, 0.1, "Opening isolated offline Chromium")
            result = capture_web_page(
                source=source,
                destination=destination,
                source_url=request.options.get("sourceUrl"),
                asset_paths=resolved_assets,
            )
            self._progress(request, 0.95, "Verifying isolated website capture")
        elif request.kind in (
            "denoise",
            "normalize_loudness",
            "compress_dialogue",
            "duck_music",
            "loop_audio",
            "crossfade_audio",
        ):
            assert source is not None
            destination = safe_output_path(
                output_dir=output_dir,
                file_name=f"{request.job_id}.wav",
            )
            if request.kind == "denoise":
                filter_graph = str(request.options.get("filter", "highpass=f=80,afftdn=nf=-25"))
            elif request.kind == "normalize_loudness":
                target = float(request.options.get("targetLufs", -16.0))
                filter_graph = f"loudnorm=I={target}:TP=-1.5:LRA=11"
            elif request.kind == "compress_dialogue":
                threshold = float(request.options.get("thresholdDb", -18.0))
                ratio = float(request.options.get("ratio", 3.0))
                attack = float(request.options.get("attackMs", 15.0))
                release = float(request.options.get("releaseMs", 180.0))
                filter_graph = (
                    f"highpass=f=70,acompressor=threshold={threshold}dB:ratio={ratio}:"
                    f"attack={attack}:release={release}:makeup=3"
                )
            else:
                filter_graph = "anull"
            self._progress(request, 0.1, "Processing derived audio")
            if request.kind == "denoise":
                engine = str(request.options.get("engine", "auto"))
                if engine not in ("auto", "deepfilternet", "rnnoise", "ffmpeg"):
                    raise WorkerError("INVALID_DENOISE_ENGINE", "Unsupported denoise engine")
                if engine in ("auto", "deepfilternet"):
                    try:
                        deepfilter_denoise(source=source, destination=destination)
                        result = {
                            "derivedAssetPath": str(destination),
                            "sourcePath": str(source),
                            "reversible": True,
                            "engine": "deepfilternet",
                        }
                    except CapabilityUnavailable:
                        if engine == "deepfilternet":
                            raise
                        process_audio(
                            source=source,
                            destination=destination,
                            filter_graph=filter_graph,
                        )
                        result = {
                            "derivedAssetPath": str(destination),
                            "sourcePath": str(source),
                            "reversible": True,
                            "engine": "ffmpeg-afftdn",
                        }
                elif engine == "rnnoise":
                    model_value = request.options.get("rnnoiseModelPath")
                    if not isinstance(model_value, str):
                        raise CapabilityUnavailable(
                            "RNNoise model",
                            "Place model.rnnn under the daemon data models/rnnoise directory",
                        )
                    model = resolve_under_root(value=model_value, root=self.data_root)
                    escaped = _escape_filter_path(model)
                    process_audio(
                        source=source,
                        destination=destination,
                        filter_graph=f"highpass=f=80,arnndn=m='{escaped}'",
                    )
                    result = {
                        "derivedAssetPath": str(destination),
                        "sourcePath": str(source),
                        "reversible": True,
                        "engine": "rnnoise",
                    }
                else:
                    process_audio(
                        source=source,
                        destination=destination,
                        filter_graph=filter_graph,
                    )
                    result = {
                        "derivedAssetPath": str(destination),
                        "sourcePath": str(source),
                        "reversible": True,
                        "engine": "ffmpeg-afftdn",
                    }
            elif request.kind == "loop_audio":
                loop_audio(
                    source=source,
                    destination=destination,
                    duration_seconds=float(request.options["targetDurationSeconds"]),
                    fade_seconds=float(request.options.get("fadeSeconds", 0.05)),
                )
            elif request.kind in ("duck_music", "crossfade_audio"):
                secondary_path = request.options.get("secondaryInputPath")
                if not isinstance(secondary_path, str):
                    raise WorkerError("SECONDARY_AUDIO_REQUIRED", "A secondary managed audio path is required")
                secondary = resolve_under_root(value=secondary_path, root=self.data_root)
                if request.kind == "duck_music":
                    threshold = float(request.options.get("threshold", 0.05))
                    ratio = float(request.options.get("ratio", 8.0))
                    attack = float(request.options.get("attackMs", 20.0))
                    release = float(request.options.get("releaseMs", 300.0))
                    main_duration = audio_duration_seconds(source)
                    filter_graph = (
                        f"[0:a][1:a]sidechaincompress=threshold={threshold}:ratio={ratio}:"
                        f"attack={attack}:release={release}[ducked];"
                        f"[ducked]apad=whole_dur={main_duration:.9f}[out]"
                    )
                else:
                    duration = float(request.options.get("durationSeconds", 0.5))
                    curve = str(request.options.get("curve", "tri"))
                    if curve not in ("tri", "qsin", "hsin", "exp", "log"):
                        raise WorkerError("INVALID_CROSSFADE_CURVE", "Unsupported crossfade curve")
                    filter_graph = f"[0:a][1:a]acrossfade=d={duration}:c1={curve}:c2={curve}[out]"
                process_audio_pair(
                    source=source,
                    secondary=secondary,
                    destination=destination,
                    filter_graph=filter_graph,
                )
            else:
                process_audio(source=source, destination=destination, filter_graph=filter_graph)
            if request.kind != "denoise":
                result = {
                    "derivedAssetPath": str(destination),
                    "sourcePath": str(source),
                    "reversible": True,
                }
        elif request.kind == "synthesize_voice":
            destination = safe_output_path(
                output_dir=output_dir,
                file_name=f"{request.job_id}.wav",
            )
            result = synthesize_voice(
                data_root=self.data_root,
                destination=destination,
                options=request.options,
                progress=lambda value, message: self._progress(request, value, message),
            )
        elif request.kind == "synthesize_sfx":
            destination = safe_output_path(
                output_dir=output_dir,
                file_name=f"{request.job_id}.wav",
            )
            result = synthesize_sfx(
                destination=destination,
                options=request.options,
                progress=lambda value, message: self._progress(request, value, message),
            )
        elif request.kind == "export":
            assert source is not None
            output_name = request.options.get("outputFileName")
            if not isinstance(output_name, str):
                raise WorkerError("INVALID_OUTPUT_NAME", "Export outputFileName is required")
            destination = safe_output_path(output_dir=output_dir, file_name=output_name)
            self._progress(request, 0.05, "Encoding pinned revision")
            result = render_export(
                source=source,
                destination=destination,
                options=request.options,
                progress=lambda value, message: self._progress(request, value, message),
            )
            self._progress(request, 0.98, "Finalizing export")
        elif request.kind == "render_preview_frames":
            result = render_preview_frames(
                project_id=request.project_id,
                job_id=request.job_id,
                output_dir=output_dir,
                options=request.options,
                progress=lambda value, message: self._progress(request, value, message),
            )
        elif request.kind == "headless_export":
            result = render_headless_export(
                data_root=self.data_root,
                project_id=request.project_id,
                output_dir=output_dir,
                options=request.options,
                progress=lambda value, message: self._progress(request, value, message),
            )
        elif request.kind == "timeline_audio_export":
            result = render_timeline_audio_export(
                data_root=self.data_root,
                output_dir=output_dir,
                options=request.options,
                progress=lambda value, message: self._progress(request, value, message),
            )
        else:
            raise WorkerError("UNSUPPORTED_JOB_KIND", f"Unsupported job kind: {request.kind}")

        self._progress(request, 1.0, "Complete")
        self.emit(WorkerEvent(request.job_id, "result", {"result": result}))
        return result

    def _progress(self, request: JobRequest, value: float, message: str) -> None:
        self.emit(
            WorkerEvent(
                request.job_id,
                "progress",
                {"progress": max(0.0, min(1.0, value)), "message": message},
            )
        )


def _escape_filter_path(path: Path) -> str:
    value = str(path).replace("\\", "\\\\")
    for character in ("'", ":", ",", "[", "]", ";"):
        value = value.replace(character, f"\\{character}")
    return value
