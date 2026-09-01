from __future__ import annotations

import struct
from collections import Counter
from dataclasses import replace

import pytest

from parasolid_kit import (
    DEFAULT_PARSE_LIMITS,
    InMemorySchemaProvider,
    ParseError,
    ParseLimits,
    SchemaCatalog,
    _core,
    compare_documents,
    inspect_xt,
    parse_xb,
    parse_xt,
    write_xb,
)
from parasolid_kit.api import _catalog_to_native
from parasolid_kit.schema import FieldDefinition, FieldType, SchemaSource, TypeDefinition
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header


def _catalog() -> SchemaCatalog:
    # Public numeric visibility facts; all field layouts below are synthetic.
    return SchemaCatalog(
        "30000",
        tuple(
            TypeDefinition(
                kind,
                "SYNTHETIC",
                "Not an actual Parasolid field layout",
                kind in (70, 83, 98),
                (FieldDefinition("value", FieldType.INTEGER, 0, int(kind in (70, 83, 98)), True),),
                SchemaSource.BASE,
            )
            for kind in (12, 17, 70, 83, 98, 110, 111, 112)
        ),
    )


def _provider() -> InMemorySchemaProvider:
    return InMemorySchemaProvider((_catalog(),))


def _scan(data: bytes, limits: ParseLimits = DEFAULT_PARSE_LIMITS) -> dict:
    compiled = _core._compile_schema_provider(*_catalog_to_native(_catalog()))
    return _core._scan_xt_node_types(
        data,
        compiled,
        [110, 111, 112],
        limits.max_file_size,
        limits.max_nodes,
        limits.max_schema_types,
        limits.max_fields_per_type,
        limits.max_string_bytes,
        limits.max_variable_elements,
    )


def _pair(words: tuple[int | None, ...]) -> tuple[bytes, bytes]:
    text = bytearray(text_header(user_field_size=len(words)))
    binary = SyntheticXbBuilder(
        schema_name="SCH_3000000_30000", schema_max_type=None, user_field_size=len(words)
    )
    for kind, index, values in [
        (12, 1, (23,)),
        (83, 2, (110, 111, 112)),
        (17, 3, (29,)),
        (70, 4, ()),
        (12, 9, (-1,)),
        (98, 10, (42,)),
    ]:
        text.extend(f"{kind} ".encode())
        payload = bytearray()
        if kind in (70, 83, 98):
            text.extend(f"{len(values)} ".encode())
            payload.extend(struct.pack(">i", len(values)))
        text.extend(f"{index} ".encode())
        payload.extend(positive_integer(index))
        for value in (*values, *(words if kind in (12, 17, 70) else ())):
            text.extend(b"?" if value is None else f"{value} ".encode())
            payload.extend(struct.pack(">i", -32764 if value is None else value))
        binary.add_raw_node(kind, bytes(payload))
    text.extend(b"1 0 ")
    return bytes(text), binary.build()


@pytest.mark.parametrize(
    "words",
    [(), (110,), (None,), (-(2**31),), (2**31 - 1,), (110, 111, 112, -1, None, 0, *range(10))],
)
def test_user_fields_are_retained_separately_and_match_binary(words) -> None:
    text_data, binary_data = _pair(words)
    text = parse_xt(text_data, schema_provider=_provider())
    binary = parse_xb(binary_data, schema_provider=_provider())
    for document in (text, binary):
        assert document.header.user_field_size == len(words)
        assert len(document.nodes) == 6
        assert document.terminator.byte_range.end == len(document.raw_bytes)
        for node in document.nodes:
            assert len(node.fields) == 1
            assert node.user_fields == (words if node.node_type in (12, 17, 70) else ())
            assert node.to_dict()["user_fields"] == list(node.user_fields)
            assert node.byte_range.end >= node.fields[-1].byte_range.end
    assert text.raw_bytes == text_data
    assert write_xb(binary) == binary_data
    assert compare_documents(text, binary).equivalent
    assert compare_documents(binary, text).equivalent
    assert _scan(text_data) == {
        "ok": True,
        "value": {
            "record_count": 6,
            "target_type_counts": {110: 0, 111: 0, 112: 0},
        },
    }


def test_text_comparison_reports_user_words_as_values_not_pointers() -> None:
    a = parse_xt(text_header(user_field_size=1) + b"12 1 7 1 1 0 ", schema_provider=_provider())
    b = parse_xt(text_header(user_field_size=1) + b"12 1 7 9 1 0 ", schema_provider=_provider())
    compared = compare_documents(a, b)
    assert not compared.equivalent
    assert compared.topology_equal and compared.schema_coverage_equal
    assert not compared.field_values_equal
    assert compared.differences[0].code == "comparison.user_fields_mismatch"


@pytest.mark.parametrize(
    "body",
    [
        b"12 1 7 ",
        b"12 1 7 1 0 ",
        b"12 1 7 2147483648 1 0 ",
        b"12 1 7 -2147483649 1 0 ",
        b"12 1 7 1.2 1 0 ",
        b"12 1 7 A1 0 ",
        b"12 1 7 1\t1 0 ",
        b"12 1 7 3 1 0 X",
        b"12 1 7 3 12 1 8 4 1 0 ",
        b"12 1 7 3 110 2 0 1 0 ",
    ],
)
def test_bad_user_field_streams_fail_without_partial_counts(body: bytes) -> None:
    data = text_header(user_field_size=1) + body
    catalog_id, definitions = _catalog_to_native(_catalog())
    limits = ParseLimits()
    full = _core._parse_xt(
        data,
        catalog_id,
        definitions,
        limits.max_file_size,
        limits.max_nodes,
        limits.max_schema_types,
        limits.max_fields_per_type,
        limits.max_string_bytes,
        limits.max_variable_elements,
    )
    assert full["ok"] is False
    assert _scan(data) == full
    assert "value" not in full


@pytest.mark.parametrize("kind", [110, 111, 112])
def test_unknown_visibility_is_rejected_in_both_encodings(kind: int) -> None:
    text = text_header(user_field_size=1) + f"{kind} 1 0 23 1 0 ".encode()
    binary = (
        SyntheticXbBuilder(schema_name="SCH_3000000_30000", schema_max_type=None, user_field_size=1)
        .add_raw_node(kind, positive_integer(1) + struct.pack(">ii", 0, 23))
        .build()
    )
    for parse, data in ((parse_xt, text), (parse_xb, binary)):
        with pytest.raises(ParseError) as captured:
            parse(data, schema_provider=_provider())
        assert captured.value.diagnostic.code == "node.unsupported_user_fields"
        assert captured.value.diagnostic.details["node_type"] == kind
    assert _scan(text)["ok"] is False
    # A synthetic node with no user fields remains a valid parser control, not real evidence.
    normal = text_header() + f"{kind} 1 110 1 0 ".encode()
    counts = Counter(n.node_type for n in parse_xt(normal, schema_provider=_provider()).nodes)
    assert _scan(normal)["value"]["target_type_counts"][kind] == counts[kind] == 1


def test_user_field_limit_is_enforced_for_full_parse_and_scan() -> None:
    data = text_header(user_field_size=2) + b"12 1 7 3 4 1 0 "
    limits = ParseLimits(max_variable_elements=1)
    with pytest.raises(ParseError) as captured:
        parse_xt(data, schema_provider=_provider(), limits=limits)
    assert captured.value.diagnostic.code == "limits.exceeded"
    assert _scan(data, limits)["ok"] is False


def test_truncated_binary_user_word_is_rejected() -> None:
    data = (
        SyntheticXbBuilder(schema_name="SCH_3000000_30000", schema_max_type=None, user_field_size=1)
        .add_raw_node(12, positive_integer(1) + struct.pack(">i", 7) + b"\x00\x01")
        .build_without_termination()
    )
    with pytest.raises(ParseError):
        parse_xb(data, schema_provider=_provider())


@pytest.mark.parametrize("size", [-1, 17])
def test_invalid_user_field_header_sizes_remain_rejected(size: int) -> None:
    with pytest.raises(ParseError):
        inspect_xt(text_header(user_field_size=size) + b"1 0 ")


@pytest.mark.parametrize("words", [(True,), (1.5,), (2**31,), (-(2**31) - 1,), tuple(range(17))])
def test_public_model_rejects_invalid_user_field_words(words) -> None:
    node = parse_xt(text_header() + b"12 1 7 1 0 ", schema_provider=_provider()).nodes[0]
    assert node.user_fields == ()
    with pytest.raises(ValueError, match="user_fields"):
        replace(node, user_fields=words)
