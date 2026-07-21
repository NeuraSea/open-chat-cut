from __future__ import annotations

import subprocess

from openchatcut_worker import hardware


def test_auto_prefers_verified_apple_adapter(monkeypatch) -> None:
    monkeypatch.setattr(hardware.shutil, "which", lambda name: f"/usr/bin/{name}")
    monkeypatch.setattr(hardware.platform, "system", lambda: "Darwin")
    monkeypatch.setattr(hardware.platform, "machine", lambda: "arm64")

    def fake_run(command, **_kwargs):
        if "-encoders" in command:
            return subprocess.CompletedProcess(
                command,
                0,
                stdout="libx264 h264_videotoolbox h264_nvenc",
                stderr="",
            )
        return subprocess.CompletedProcess(command, 0, stdout="", stderr="")

    monkeypatch.setattr(hardware.subprocess, "run", fake_run)
    hardware.probe_hardware_capabilities.cache_clear()
    result = hardware.probe_hardware_capabilities("auto")
    assert result["videoEncoding"]["selected"] == "apple"
    descriptor, arguments = hardware.h264_encoder_arguments("auto")
    assert descriptor["accelerated"] is True
    assert "h264_videotoolbox" in arguments


def test_explicit_nvidia_falls_back_honestly_to_cpu(monkeypatch) -> None:
    monkeypatch.setattr(hardware.shutil, "which", lambda name: f"/usr/bin/{name}")
    monkeypatch.setattr(hardware.platform, "system", lambda: "Linux")
    monkeypatch.setattr(hardware.platform, "machine", lambda: "x86_64")

    def fake_run(command, **_kwargs):
        if "-encoders" in command:
            return subprocess.CompletedProcess(
                command, 0, stdout="libx264 h264_nvenc", stderr=""
            )
        encoder = command[command.index("-c:v") + 1]
        if encoder == "h264_nvenc":
            return subprocess.CompletedProcess(
                command, 1, stdout="", stderr="No capable devices found"
            )
        return subprocess.CompletedProcess(command, 0, stdout="", stderr="")

    monkeypatch.setattr(hardware.subprocess, "run", fake_run)
    hardware.probe_hardware_capabilities.cache_clear()
    result = hardware.probe_hardware_capabilities("nvidia")
    assert result["videoEncoding"]["selected"] == "cpu"
    assert "No capable devices" in result["videoEncoding"]["fallbackReason"]
    descriptor, arguments = hardware.h264_encoder_arguments("nvidia")
    assert descriptor["accelerated"] is False
    assert descriptor["requested"] == "nvidia"
    assert "libx264" in arguments


def test_invalid_preference_is_treated_as_auto(monkeypatch) -> None:
    monkeypatch.setenv("OPENCHATCUT_VIDEO_ACCELERATION", "not-an-adapter")
    assert hardware.acceleration_preference() == "auto"


def test_runtime_features_report_installed_modules_without_importing_them(
    monkeypatch,
) -> None:
    available = {"faster_whisper", "playwright.sync_api"}
    monkeypatch.setattr(hardware, "_module_available", lambda name: name in available)

    assert hardware.probe_runtime_features() == {
        "fasterWhisper": True,
        "speakerDiarization": False,
        "deepFilterNet": False,
        "playwright": True,
        "kokoro": False,
        "audioGen": False,
    }


def test_daemon_verified_adapter_skips_redundant_probe(monkeypatch) -> None:
    monkeypatch.setenv(hardware.VERIFIED_ADAPTER_ENV, "apple")

    def unexpected_probe(_preference):
        raise AssertionError("the daemon-verified adapter must not be probed again")

    monkeypatch.setattr(hardware, "probe_hardware_capabilities", unexpected_probe)
    descriptor, arguments = hardware.h264_encoder_arguments("auto")

    assert descriptor == {
        "requested": "auto",
        "selected": "apple",
        "encoder": "h264_videotoolbox",
        "accelerated": True,
        "fallbackReason": None,
    }
    assert "h264_videotoolbox" in arguments


def test_untrusted_verified_adapter_value_is_ignored(monkeypatch) -> None:
    monkeypatch.setenv(hardware.VERIFIED_ADAPTER_ENV, "arbitrary-command")
    monkeypatch.setattr(
        hardware,
        "probe_hardware_capabilities",
        lambda _preference: {
            "videoEncoding": {"selected": "cpu", "fallbackReason": None}
        },
    )

    descriptor, arguments = hardware.h264_encoder_arguments("auto")

    assert descriptor["selected"] == "cpu"
    assert "libx264" in arguments
