from __future__ import annotations

import struct

import pytest

from tests.support.parasolid_binary import SyntheticXbBuilder


def _read_i32_ascii(data: bytes, offset: int) -> tuple[str, int]:
    length = struct.unpack_from(">i", data, offset)[0]
    start = offset + 4
    end = start + length
    return data[start:end].decode("ascii"), end


def _read_u16_ascii(data: bytes, offset: int) -> tuple[str, int]:
    length = struct.unpack_from(">H", data, offset)[0]
    start = offset + 2
    end = start + length
    return data[start:end].decode("ascii"), end


def test_synthetic_binary_builder_emits_deterministic_header_and_terminator() -> None:
    builder = SyntheticXbBuilder()
    payload = builder.build()

    assert payload.startswith(b"PS\x00\x00")
    modeller, offset = _read_u16_ascii(payload, 4)
    schema, offset = _read_i32_ascii(payload, offset)
    maximum = struct.unpack_from(">H", payload, offset)[0]
    user_field_size = struct.unpack_from(">i", payload, offset + 2)[0]

    assert modeller == ": TRANSMIT FILE created by modeller version 3000000"
    assert schema == "SCH_3000000_30000_13006"
    assert user_field_size == 0
    assert maximum == 205
    assert payload[-4:] == struct.pack(">hh", 1, 1)


def test_synthetic_binary_builder_can_make_a_truncated_fixture() -> None:
    builder = SyntheticXbBuilder().add_raw_node(12, b"\xff\x00\x02")

    assert builder.build_without_termination().endswith(struct.pack(">h", 12) + b"\xff\x00\x02")
    assert not builder.build_without_termination().endswith(struct.pack(">hh", 1, 1))


def test_synthetic_binary_builder_rejects_termination_as_a_raw_node() -> None:
    with pytest.raises(ValueError, match="termination"):
        SyntheticXbBuilder().add_raw_node(1)

    with pytest.raises(ValueError, match="index must be zero"):
        SyntheticXbBuilder().build(termination_index=1)
