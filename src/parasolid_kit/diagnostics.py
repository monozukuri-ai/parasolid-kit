"""Stable diagnostics shared by binary, text, schema, and B-Rep layers."""

from __future__ import annotations

import re
from collections.abc import Mapping
from dataclasses import dataclass, field
from enum import Enum
from typing import TypeAlias

JsonScalar: TypeAlias = str | int | float | bool | None

_DIAGNOSTIC_CODE = re.compile(r"^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+$")


class DiagnosticSeverity(str, Enum):
    """Severity presented to callers and command-line consumers."""

    INFO = "info"
    WARNING = "warning"
    ERROR = "error"


class DiagnosticKind(str, Enum):
    """Outcome category; intentionally separate from severity."""

    UNSUPPORTED = "unsupported"
    INCOMPLETE = "incomplete"
    INVALID = "invalid"
    LIMIT = "limit"
    INTERNAL = "internal"


@dataclass(frozen=True, slots=True)
class SourceLocation:
    """A byte or token location in the original source representation."""

    byte_offset: int | None = None
    token_index: int | None = None
    line: int | None = None
    column: int | None = None

    def __post_init__(self) -> None:
        if all(
            value is None for value in (self.byte_offset, self.token_index, self.line, self.column)
        ):
            raise ValueError("a source location must contain at least one coordinate")
        for name in ("byte_offset", "token_index"):
            value = getattr(self, name)
            if value is not None and value < 0:
                raise ValueError(f"{name} must be non-negative")
        for name in ("line", "column"):
            value = getattr(self, name)
            if value is not None and value < 1:
                raise ValueError(f"{name} must be at least 1")

    def to_dict(self) -> dict[str, int]:
        """Return only coordinates known for this source."""

        result: dict[str, int] = {}
        for name in ("byte_offset", "token_index", "line", "column"):
            value = getattr(self, name)
            if value is not None:
                result[name] = value
        return result


@dataclass(frozen=True, slots=True)
class Diagnostic:
    """One explicit parser or validation finding."""

    code: str
    severity: DiagnosticSeverity
    kind: DiagnosticKind
    message: str
    location: SourceLocation | None = None
    node_type: int | None = None
    node_id: int | None = None
    schema_key: str | None = None
    fatal: bool = False
    details: Mapping[str, JsonScalar] = field(default_factory=dict)

    def __post_init__(self) -> None:
        if not _DIAGNOSTIC_CODE.fullmatch(self.code):
            raise ValueError("diagnostic code must contain lowercase dotted components")
        if not self.message.strip():
            raise ValueError("diagnostic message must not be empty")
        if not isinstance(self.severity, DiagnosticSeverity):
            object.__setattr__(self, "severity", DiagnosticSeverity(self.severity))
        if not isinstance(self.kind, DiagnosticKind):
            object.__setattr__(self, "kind", DiagnosticKind(self.kind))
        if self.node_type is not None and self.node_type < 0:
            raise ValueError("node_type must be non-negative")
        if self.node_id is not None and self.node_id < 0:
            raise ValueError("node_id must be non-negative")
        if self.schema_key is not None and not self.schema_key:
            raise ValueError("schema_key must not be empty")

    @property
    def recoverable(self) -> bool:
        """Whether a caller can continue without claiming full success."""

        return not self.fatal

    def to_dict(self) -> dict[str, object]:
        """Serialize the finding for JSON-compatible reports."""

        result: dict[str, object] = {
            "code": self.code,
            "severity": self.severity.value,
            "kind": self.kind.value,
            "message": self.message,
            "fatal": self.fatal,
            "recoverable": self.recoverable,
        }
        if self.location is not None:
            result["location"] = self.location.to_dict()
        if self.node_type is not None:
            result["node_type"] = self.node_type
        if self.node_id is not None:
            result["node_id"] = self.node_id
        if self.schema_key is not None:
            result["schema_key"] = self.schema_key
        if self.details:
            result["details"] = dict(self.details)
        return result
