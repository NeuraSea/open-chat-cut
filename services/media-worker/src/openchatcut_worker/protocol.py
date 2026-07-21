from __future__ import annotations

from dataclasses import asdict, dataclass, field
from typing import Any, Literal


JobKind = Literal[
    "inspect_media",
    "prepare_media",
    "normalize_generated_media",
    "capture_web_page",
    "transcribe",
    "denoise",
    "normalize_loudness",
    "synthesize_voice",
    "synthesize_sfx",
    "export",
    "render_preview_frames",
    "headless_export",
    "timeline_audio_export",
]


@dataclass(frozen=True)
class JobRequest:
    job_id: str
    kind: JobKind
    project_id: str
    input_path: str
    output_dir: str
    options: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_dict(cls, value: dict[str, Any]) -> "JobRequest":
        required = ("jobId", "kind", "projectId", "inputPath", "outputDir")
        missing = [key for key in required if not isinstance(value.get(key), str)]
        if missing:
            raise ValueError(f"Missing string fields: {', '.join(missing)}")
        return cls(
            job_id=value["jobId"],
            kind=value["kind"],
            project_id=value["projectId"],
            input_path=value["inputPath"],
            output_dir=value["outputDir"],
            options=value.get("options") if isinstance(value.get("options"), dict) else {},
        )


@dataclass(frozen=True)
class WorkerEvent:
    job_id: str
    type: Literal["progress", "result", "error"]
    payload: dict[str, Any]

    def to_dict(self) -> dict[str, Any]:
        return {"jobId": self.job_id, "type": self.type, **self.payload}


@dataclass(frozen=True)
class TranscriptWord:
    id: str
    spoken_text: str
    display_text: str
    start_ms: int
    end_ms: int
    confidence: float | None = None
    speaker_id: str | None = None

    def to_dict(self) -> dict[str, Any]:
        value = asdict(self)
        return {
            "id": value["id"],
            "spokenText": value["spoken_text"],
            "displayText": value["display_text"],
            "startMs": value["start_ms"],
            "endMs": value["end_ms"],
            "confidence": value["confidence"],
            "speakerId": value["speaker_id"],
        }
