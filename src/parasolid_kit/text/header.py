"""Typed Python representation of Rust X_T header inspection."""

from __future__ import annotations

from dataclasses import dataclass

from ..binary.header import ByteRange


@dataclass(frozen=True, slots=True)
class XtHeader:
    """Fields confirmed before parsing an X_T node stream."""

    flag: str
    modeller_version: str
    schema_key: str
    user_field_size: int
    schema_max_type: int | None
    file_size: int
    common_header_range: ByteRange | None
    text_stream_header_range: ByteRange
    header_range: ByteRange

    def __post_init__(self) -> None:
        if self.flag != "T":
            raise ValueError("flag must be 'T'")
        if not self.modeller_version:
            raise ValueError("modeller_version must not be empty")
        if not self.schema_key.startswith("SCH_"):
            raise ValueError("schema_key must start with 'SCH_'")
        if not 0 <= self.user_field_size <= 16:
            raise ValueError("user_field_size must be between 0 and 16")
        if self.schema_max_type is not None and self.schema_max_type < 0:
            raise ValueError("schema_max_type must be non-negative when present")
        if self.file_size < self.header_range.end:
            raise ValueError("file_size must include the complete header range")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible inspection report."""

        return {
            "flag": self.flag,
            "modeller_version": self.modeller_version,
            "schema_key": self.schema_key,
            "user_field_size": self.user_field_size,
            "schema_max_type": self.schema_max_type,
            "file_size": self.file_size,
            "common_header_range": (
                None if self.common_header_range is None else self.common_header_range.to_dict()
            ),
            "text_stream_header_range": self.text_stream_header_range.to_dict(),
            "header_range": self.header_range.to_dict(),
        }


@dataclass(frozen=True, slots=True)
class XtTermination:
    """Validated text node-type-1 marker followed by index zero."""

    index: int
    byte_range: ByteRange

    def __post_init__(self) -> None:
        if self.index != 0:
            raise ValueError("termination index must be zero")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        return {"index": self.index, "byte_range": self.byte_range.to_dict()}
