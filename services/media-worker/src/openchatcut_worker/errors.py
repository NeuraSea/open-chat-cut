class WorkerError(RuntimeError):
    """Structured error safe to return to the local daemon."""

    def __init__(self, code: str, message: str, *, details: dict | None = None):
        super().__init__(message)
        self.code = code
        self.details = details or {}


class CapabilityUnavailable(WorkerError):
    def __init__(self, capability: str, install_hint: str):
        super().__init__(
            "CAPABILITY_UNAVAILABLE",
            f"{capability} is not installed",
            details={"capability": capability, "installHint": install_hint},
        )
