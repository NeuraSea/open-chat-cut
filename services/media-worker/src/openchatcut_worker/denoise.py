from __future__ import annotations

from pathlib import Path

from .errors import CapabilityUnavailable, WorkerError


def deepfilter_denoise(*, source: Path, destination: Path) -> None:
    """Run the optional in-process DeepFilterNet model without replacing source."""

    try:
        from df.enhance import enhance, init_df, load_audio, save_audio
    except ImportError as error:
        raise CapabilityUnavailable(
            "DeepFilterNet",
            "Install services/media-worker[denoise] and restart openchatcutd",
        ) from error
    try:
        model, state, _ = init_df()
        audio, _ = load_audio(str(source), sr=state.sr())
        enhanced = enhance(model, state, audio)
        save_audio(str(destination), enhanced, state.sr())
    except Exception as error:  # The optional model stack has several error types.
        destination.unlink(missing_ok=True)
        raise WorkerError(
            "DEEPFILTER_DENOISE_FAILED",
            f"DeepFilterNet could not process the managed source: {error}",
        ) from error
    if not destination.is_file() or destination.stat().st_size < 12:
        destination.unlink(missing_ok=True)
        raise WorkerError(
            "DEEPFILTER_DENOISE_FAILED",
            "DeepFilterNet did not create a valid output file",
        )
