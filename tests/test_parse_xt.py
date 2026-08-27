from __future__ import annotations

import struct
from pathlib import Path

import pytest

from parasolid_kit import (
    ByteRange,
    InMemorySchemaProvider,
    ParseError,
    SchemaCatalog,
    SchemaError,
    compare_documents,
    inspect_xt,
    parse_xb,
    parse_xt,
    write_xb,
)
from parasolid_kit.schema import FieldDefinition, FieldType, SchemaSource, TypeDefinition
from parasolid_kit.text import XtHeader, XtTermination
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header


def _provider() -> InMemorySchemaProvider:
    fields = (
        FieldDefinition(
            name="next",
            field_type=FieldType.POINTER_INDEX,
            pointer_class=12,
            element_count=0,
            transmitted=True,
        ),
        FieldDefinition(
            name="value",
            field_type=FieldType.DOUBLE,
            pointer_class=0,
            element_count=0,
            transmitted=True,
        ),
    )
    definition = TypeDefinition(
        node_type=12,
        name="BODY",
        description="Synthetic body",
        variable=False,
        fields=fields,
        source=SchemaSource.BASE,
    )
    return InMemorySchemaProvider((SchemaCatalog("30000", (definition,)),))


def _xt_document() -> bytes:
    # X_T uses indices 1 and 9; number tokens end in a space, while newlines are layout.
    return text_header(common_header=True) + b"12 1 9 100\n0 12 9 0 1 1 0 "


def _xb_document() -> bytes:
    first = positive_integer(1) + positive_integer(2) + struct.pack(">d", 1000.0000000000001)
    second = positive_integer(2) + positive_integer(0) + struct.pack(">d", 1.0)
    return (
        SyntheticXbBuilder(schema_name="SCH_3000000_30000", schema_max_type=None)
        .add_raw_node(12, first)
        .add_raw_node(12, second)
        .build()
    )


def test_inspect_xt_uses_internal_schema_key_and_preserves_ranges(tmp_path: Path) -> None:
    data = _xt_document()
    path = tmp_path / "model.x_t"
    path.write_bytes(data)

    header = inspect_xt(path)

    assert isinstance(header, XtHeader)
    assert header.flag == "T"
    assert header.schema_key == "SCH_3000000_30000"
    assert header.common_header_range == ByteRange(0, data.index(b"T51"))
    assert header.text_stream_header_range.start == header.common_header_range.end
    assert header.header_range.end < len(data)
    assert header.to_dict()["flag"] == "T"


def test_parse_xt_returns_shared_raw_model_and_reports_missing_catalog() -> None:
    document = parse_xt(_xt_document(), schema_provider=_provider())

    assert document.format == "text"
    assert isinstance(document.header, XtHeader)
    assert isinstance(document.terminator, XtTermination)
    assert len(document.nodes) == 2
    assert document.nodes[0].index == 1
    assert document.nodes[0].fields[0].values[0].value == 9
    assert document.nodes[0].fields[1].values[0].value == 1000.0
    assert document.schema_coverage.node_types == (12,)
    assert document.raw_bytes == _xt_document()

    with pytest.raises(SchemaError) as captured:
        parse_xt(_xt_document())
    assert captured.value.diagnostic.code == "schema.missing_base_schema"
    assert captured.value.diagnostic.details["schema"] == "30000"

    with pytest.raises(ValueError, match="parsed from X_B"):
        write_xb(document)


def test_compare_documents_remaps_indices_and_applies_documented_tolerance() -> None:
    text = parse_xt(_xt_document(), schema_provider=_provider())
    binary = parse_xb(_xb_document(), schema_provider=_provider())

    comparison = compare_documents(text, binary)

    assert comparison.equivalent
    assert comparison.schema_key_equal
    assert comparison.schema_coverage_equal
    assert comparison.node_type_counts_equal
    assert not comparison.node_index_layout_equal
    assert comparison.topology_equal
    assert comparison.field_values_equal
    assert comparison.compared_node_count == 2
    assert comparison.difference_count == 0
    assert comparison.to_dict()["level"] == "L3"

    exact = compare_documents(text, binary, absolute_tolerance=0.0, relative_tolerance=0.0)
    assert not exact.equivalent
    assert exact.topology_equal
    assert not exact.field_values_equal
    assert exact.differences[0].code == "comparison.field_value_mismatch"


def test_parse_xt_reports_text_delimiter_and_trailing_content() -> None:
    malformed = text_header() + b"12 1 0 1.0"
    with pytest.raises(ParseError) as captured:
        parse_xt(malformed, schema_provider=_provider())
    assert captured.value.diagnostic.code == "text.invalid_delimiter"

    trailing = _xt_document() + b"X"
    with pytest.raises(ParseError) as captured_trailing:
        parse_xt(trailing, schema_provider=_provider())
    assert captured_trailing.value.diagnostic.code == "text.trailing_content"
