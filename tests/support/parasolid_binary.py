"""Small deterministic X_B-like payloads for framing and limit tests.

This module intentionally lives under tests. It is not a production writer and
must not become a source of format truth without validation against real files.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field


def _i16(value: int) -> bytes:
    return struct.pack(">h", value)


def _u16(value: int) -> bytes:
    return struct.pack(">H", value)


def _i32(value: int) -> bytes:
    return struct.pack(">i", value)


def _ascii_i32(text: str) -> bytes:
    encoded = text.encode("ascii")
    return _i32(len(encoded)) + encoded


def _ascii_u16(text: str) -> bytes:
    encoded = text.encode("ascii")
    return _u16(len(encoded)) + encoded


@dataclass(slots=True)
class SyntheticXbBuilder:
    """Build the known binary header plus caller-supplied raw node records."""

    modeller_version: str = ": TRANSMIT FILE created by modeller version 3000000"
    schema_name: str = "SCH_3000000_30000_13006"
    user_field_size: int = 0
    schema_max_type: int | None = 205
    _records: list[bytes] = field(default_factory=list)

    def __post_init__(self) -> None:
        self.modeller_version.encode("ascii")
        self.schema_name.encode("ascii")
        if not 0 <= self.user_field_size <= 16:
            raise ValueError("user_field_size must be between 0 and 16")
        components = self.schema_name.removeprefix("SCH_").split("_")
        if (
            not self.schema_name.startswith("SCH_")
            or len(components) not in (2, 3)
            or any(not component.isdigit() for component in components)
        ):
            raise ValueError("schema_name must contain two or three numeric components")
        if len(components) == 3 and self.schema_max_type is None:
            raise ValueError("embedded schema fixture requires schema_max_type")
        if len(components) == 2 and self.schema_max_type is not None:
            raise ValueError("standard schema fixture must omit schema_max_type")
        if self.schema_max_type is not None and not 0 <= self.schema_max_type <= 65_535:
            raise ValueError("schema_max_type must fit an unsigned 16-bit field")

    def add_raw_node(self, node_type: int, payload: bytes = b"") -> SyntheticXbBuilder:
        """Append a raw non-termination node for a future parser fixture."""

        if node_type == 1:
            raise ValueError("use build() to add the termination node")
        if self.schema_max_type is not None and node_type > self.schema_max_type:
            raise ValueError("node_type lies outside the fixture schema range")
        self._records.append(_i16(node_type) + bytes(payload))
        return self

    def header(self) -> bytes:
        """Return the observed binary transmit header framing."""

        fields = [
            b"PS\x00\x00",
            _ascii_u16(self.modeller_version),
            _ascii_i32(self.schema_name),
        ]
        if self.schema_max_type is not None:
            fields.append(_u16(self.schema_max_type))
        fields.append(_i32(self.user_field_size))
        return b"".join(fields)

    def build(self, *, termination_index: int = 0) -> bytes:
        """Return a payload terminated by node type 1 and compact index zero."""

        if termination_index != 0:
            raise ValueError("a valid termination index must be zero")

        return b"".join(
            (
                self.header(),
                *self._records,
                _i16(1),
                _i16(termination_index + 1),
            )
        )

    def build_without_termination(self) -> bytes:
        """Return a deliberately truncated payload for negative tests."""

        return b"".join((self.header(), *self._records))
