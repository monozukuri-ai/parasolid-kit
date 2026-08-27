from __future__ import annotations

from pathlib import Path

import pytest

from parasolid_kit import ByteRange, ParseError, ParseLimits, XbBinaryFormat, inspect_xb
from tests.support.parasolid_binary import SyntheticXbBuilder


def test_inspect_xb_returns_typed_header_without_parsing_nodes() -> None:
    builder = SyntheticXbBuilder().add_raw_node(12, b"arbitrary-node-bytes")
    payload = builder.build()

    header = inspect_xb(payload)

    assert header.signature == b"PS"
    assert header.binary_format is XbBinaryFormat.NEUTRAL
    assert header.modeller_version == builder.modeller_version
    assert header.schema_key == builder.schema_name
    assert header.user_field_size == 0
    assert header.schema_max_type == 205
    assert header.file_size == len(payload)
    assert header.text_header_range is None
    assert header.binary_header_range == ByteRange(0, len(builder.header()))
    assert header.header_range == header.binary_header_range


def test_inspect_xb_accepts_path_bytearray_and_memoryview(tmp_path: Path) -> None:
    payload = SyntheticXbBuilder().build()
    path = tmp_path / "minimal.x_b"
    path.write_bytes(payload)

    assert inspect_xb(path).schema_key == SyntheticXbBuilder().schema_name
    assert inspect_xb(bytearray(payload)).file_size == len(payload)
    assert inspect_xb(memoryview(payload)).file_size == len(payload)


@pytest.mark.parametrize("newline", [b"\n", b"\r\n"])
@pytest.mark.parametrize("padding", [b"", b"*" * 61])
def test_inspect_xb_records_optional_text_preamble(padding: bytes, newline: bytes) -> None:
    preamble = b"**ABCDEFGHIJKLMNOPQRSTUVWXYZ\n**END_OF_HEADER**" + padding + newline
    payload = preamble + SyntheticXbBuilder().build()

    header = inspect_xb(payload)

    assert header.text_header_range == ByteRange(0, len(preamble))
    assert header.binary_header_range.start == len(preamble)
    assert header.header_range == ByteRange(0, len(preamble) + len(SyntheticXbBuilder().header()))


def test_inspect_xb_maps_native_truncation_to_structured_parse_error() -> None:
    truncated = SyntheticXbBuilder().header()[:-1]

    with pytest.raises(ParseError) as captured:
        inspect_xb(truncated)

    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "binary.truncated_field"
    assert diagnostic.location is not None
    assert diagnostic.location.byte_offset == len(SyntheticXbBuilder().header()) - 4
    assert diagnostic.details == {"needed": 4, "remaining": 3}


def test_inspect_xb_rejects_invalid_signature_at_offset_zero() -> None:
    with pytest.raises(ParseError) as captured:
        inspect_xb(b"NO")

    assert captured.value.diagnostic.code == "binary.invalid_signature"
    assert captured.value.diagnostic.location is not None
    assert captured.value.diagnostic.location.byte_offset == 0


def test_inspect_xb_applies_rust_string_limit() -> None:
    limits = ParseLimits(max_string_bytes=8)

    with pytest.raises(ParseError) as captured:
        inspect_xb(SyntheticXbBuilder().build(), limits=limits)

    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "limits.exceeded"
    assert diagnostic.details["resource"] == "string_bytes"
    assert diagnostic.location is not None
    assert diagnostic.location.byte_offset == 4


def test_inspection_report_is_json_compatible() -> None:
    report = inspect_xb(SyntheticXbBuilder().build()).to_dict()

    assert report["signature"] == "PS"
    assert report["binary_format"] == "neutral"
    assert report["schema_key"] == "SCH_3000000_30000_13006"
    assert report["binary_header_range"] == {
        "start": 0,
        "end": len(SyntheticXbBuilder().header()),
        "length": len(SyntheticXbBuilder().header()),
    }


def test_inspect_xb_rejects_unsupported_source_type() -> None:
    with pytest.raises(TypeError, match="path or bytes-like"):
        inspect_xb(123)  # type: ignore[arg-type]
