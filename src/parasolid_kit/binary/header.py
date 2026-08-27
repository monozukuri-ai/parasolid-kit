"""Typed Python representation of Rust X_B header inspection."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum


class XbBinaryFormat(str, Enum):
    """Physical representation selected by the X_B flag sequence."""

    NEUTRAL = "neutral"


@dataclass(frozen=True, slots=True)
class ByteRange:
    """Half-open absolute byte range in the original source."""

    start: int
    end: int

    def __post_init__(self) -> None:
        if isinstance(self.start, bool) or not isinstance(self.start, int) or self.start < 0:
            raise ValueError("range start must be a non-negative integer")
        if isinstance(self.end, bool) or not isinstance(self.end, int) or self.end < self.start:
            raise ValueError("range end must be an integer not preceding start")

    @property
    def length(self) -> int:
        """Return the number of bytes in the range."""

        return self.end - self.start

    def to_dict(self) -> dict[str, int]:
        """Return a JSON-compatible representation."""

        return {"start": self.start, "end": self.end, "length": self.length}


@dataclass(frozen=True, slots=True)
class XbHeader:
    """Fields confirmed by level-zero X_B inspection only."""

    signature: bytes
    binary_format: XbBinaryFormat
    modeller_version: str
    schema_key: str
    user_field_size: int
    schema_max_type: int | None
    file_size: int
    text_header_range: ByteRange | None
    binary_header_range: ByteRange
    header_range: ByteRange

    def __post_init__(self) -> None:
        if self.signature != b"PS":
            raise ValueError("signature must be b'PS'")
        if not isinstance(self.binary_format, XbBinaryFormat):
            object.__setattr__(self, "binary_format", XbBinaryFormat(self.binary_format))
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
            "signature": self.signature.decode("ascii"),
            "binary_format": self.binary_format.value,
            "modeller_version": self.modeller_version,
            "schema_key": self.schema_key,
            "user_field_size": self.user_field_size,
            "schema_max_type": self.schema_max_type,
            "file_size": self.file_size,
            "text_header_range": (
                None if self.text_header_range is None else self.text_header_range.to_dict()
            ),
            "binary_header_range": self.binary_header_range.to_dict(),
            "header_range": self.header_range.to_dict(),
        }
