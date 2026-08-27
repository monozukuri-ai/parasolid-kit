"""Resource limits applied consistently by every parser entry point."""

from __future__ import annotations

from dataclasses import dataclass, fields

from .diagnostics import SourceLocation
from .errors import LimitExceededError


@dataclass(frozen=True, slots=True)
class ParseLimits:
    """Bound memory and CPU-amplifying counts before allocation or traversal."""

    max_file_size: int = 256 * 1024 * 1024
    max_nodes: int = 10_000_000
    max_schema_types: int = 65_536
    max_fields_per_type: int = 4_096
    max_string_bytes: int = 16 * 1024 * 1024
    max_variable_elements: int = 10_000_000
    max_diagnostics: int = 10_000

    def __post_init__(self) -> None:
        for item in fields(self):
            value = getattr(self, item.name)
            if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
                raise ValueError(f"{item.name} must be a positive integer")

    def ensure_file_size(
        self,
        size: int,
        *,
        location: SourceLocation | None = None,
    ) -> None:
        """Reject a source before retaining or indexing an oversized payload."""

        self.ensure_within(
            resource="file_size",
            actual=size,
            limit=self.max_file_size,
            location=location,
        )

    @staticmethod
    def ensure_within(
        *,
        resource: str,
        actual: int,
        limit: int,
        location: SourceLocation | None = None,
    ) -> None:
        """Apply one named limit while retaining structured failure details."""

        if isinstance(actual, bool) or not isinstance(actual, int) or actual < 0:
            raise ValueError("actual must be a non-negative integer")
        if isinstance(limit, bool) or not isinstance(limit, int) or limit <= 0:
            raise ValueError("limit must be a positive integer")
        if actual > limit:
            raise LimitExceededError(
                resource=resource,
                actual=actual,
                limit=limit,
                location=location,
            )


DEFAULT_PARSE_LIMITS = ParseLimits()
