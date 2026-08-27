"""Exception hierarchy for strict parsing and validation modes."""

from __future__ import annotations

from .diagnostics import (
    Diagnostic,
    DiagnosticKind,
    DiagnosticSeverity,
    SourceLocation,
)


class ParasolidError(Exception):
    """Base class for package-defined failures."""


class ParseError(ParasolidError):
    """A strict-mode failure carrying a structured diagnostic."""

    def __init__(self, diagnostic: Diagnostic) -> None:
        self.diagnostic = diagnostic
        super().__init__(self._format_message())

    def _format_message(self) -> str:
        location = ""
        if self.diagnostic.location is not None:
            coordinates = ", ".join(
                f"{name}={value}" for name, value in self.diagnostic.location.to_dict().items()
            )
            location = f" ({coordinates})"
        return f"{self.diagnostic.code}{location}: {self.diagnostic.message}"


class SchemaError(ParseError):
    """A schema could not be loaded or resolved safely."""


class LimitExceededError(ParseError):
    """Input exceeded an explicit parser resource limit."""

    def __init__(
        self,
        *,
        resource: str,
        actual: int,
        limit: int,
        location: SourceLocation | None = None,
    ) -> None:
        self.resource = resource
        self.actual = actual
        self.limit = limit
        super().__init__(
            Diagnostic(
                code="limits.exceeded",
                severity=DiagnosticSeverity.ERROR,
                kind=DiagnosticKind.LIMIT,
                message=f"{resource} exceeds the configured limit",
                location=location,
                fatal=True,
                details={
                    "resource": resource,
                    "actual": actual,
                    "limit": limit,
                },
            )
        )
