from __future__ import annotations

import struct
from dataclasses import FrozenInstanceError
from pathlib import Path

import pytest

from parasolid_kit import (
    FieldValue,
    InMemorySchemaProvider,
    ParseError,
    ParseLimits,
    SchemaCatalog,
    SchemaError,
    parse_xb,
    write_xb,
)
from parasolid_kit.schema import FieldDefinition, FieldType, SchemaSource, TypeDefinition
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import (
    embedded_field,
    full_schema,
    positive_integer,
)


def _intersection_payload(*, value_count: int, index: int, include_schema: bool) -> bytes:
    schema = (
        full_schema(
            "INTERSECTION_DATA",
            "Intersection data",
            (
                embedded_field("uv_type", field_type="u"),
                embedded_field("values", field_type="f", element_count=1),
            ),
        )
        if include_schema
        else b""
    )
    values = b"".join(struct.pack(">d", float(value)) for value in range(value_count))
    return b"".join(
        (
            schema,
            struct.pack(">i", value_count),
            positive_integer(index),
            b"\x03",
            values,
        )
    )


def _embedded_document_bytes() -> tuple[bytes, bytes]:
    first_schema = full_schema(
        "INTERSECTION_DATA",
        "Intersection data",
        (
            embedded_field("uv_type", field_type="u"),
            embedded_field("values", field_type="f", element_count=1),
        ),
    )
    payload = (
        SyntheticXbBuilder()
        .add_raw_node(204, _intersection_payload(value_count=2, index=1, include_schema=True))
        .add_raw_node(204, _intersection_payload(value_count=0, index=2, include_schema=False))
        .build()
    )
    return payload, first_schema


def _provider() -> InMemorySchemaProvider:
    return InMemorySchemaProvider((SchemaCatalog("13006", ()),))


def _fixed_codec_provider() -> InMemorySchemaProvider:
    definitions = tuple(
        FieldDefinition(
            name=field_type.name.lower(),
            field_type=field_type,
            pointer_class=1 if field_type is FieldType.POINTER_INDEX else 0,
            element_count=0,
            transmitted=True,
        )
        for field_type in FieldType
        if field_type is not FieldType.OPAQUE_POINTER
    )
    node = TypeDefinition(
        node_type=42,
        name="EVERY_FIXED_CODEC",
        description="Every fixed codec",
        variable=False,
        fields=definitions,
        source=SchemaSource.BASE,
    )
    return InMemorySchemaProvider((SchemaCatalog("30000", (node,)),))


def test_parse_xb_returns_typed_raw_document_and_native_round_trip() -> None:
    payload, embedded_schema = _embedded_document_bytes()

    document = parse_xb(payload, schema_provider=_provider())

    assert document.format == "binary"
    assert document.schema_key.provider_schema == "13006"
    assert len(document.schemas) == 1
    assert document.schemas[0].raw_schema == embedded_schema
    assert document.schemas[0].definition.source is SchemaSource.EMBEDDED_FULL
    assert document.schema_coverage.node_types == (204,)
    assert document.schema_coverage.full_count == 1
    assert len(document.nodes) == 2
    assert document.nodes[0].type_name == "INTERSECTION_DATA"
    assert document.nodes[0].variable_length == 2
    assert document.nodes[0].first_schema == document.schemas[0]
    assert document.nodes[1].first_schema is None
    assert document.nodes[1].variable_length == 0
    assert document.nodes[0].fields[0].values == (FieldValue(FieldType.UNSIGNED_BYTE, 3),)
    assert [value.value for value in document.nodes[0].fields[1].values] == [0.0, 1.0]
    assert document.terminator.index == 0
    assert document.terminator.byte_range.end == len(payload)
    assert document.raw_bytes == payload
    assert write_xb(document) == payload
    assert document.to_dict()["raw_byte_count"] == len(payload)

    with pytest.raises(FrozenInstanceError):
        document.raw_bytes = b"changed"  # type: ignore[misc]


def test_python_binding_preserves_every_fixed_field_codec() -> None:
    fields = bytearray((7, ord("A"), 1))
    fields.extend(struct.pack(">hHi", -32_764, 0x3042, -32_764))
    fields.extend(positive_integer(0))
    fields.extend(struct.pack(">i", -32_764))
    fields.extend(struct.pack(">d", -3.14158e13))
    for value in range(1, 15):
        fields.extend(struct.pack(">d", float(value)))
    payload = (
        SyntheticXbBuilder(schema_name="SCH_3000000_30000", schema_max_type=None)
        .add_raw_node(42, positive_integer(1) + fields)
        .build()
    )

    node = parse_xb(payload, schema_provider=_fixed_codec_provider()).nodes[0]

    transmitted_types = [
        field_type for field_type in FieldType if field_type is not FieldType.OPAQUE_POINTER
    ]
    assert [field.values[0].field_type for field in node.fields] == transmitted_types
    assert node.fields[3].values[0].value is None
    assert node.fields[5].values[0].value is None
    assert node.fields[7].values[0].value is None
    assert node.fields[8].values[0].value is None
    assert node.fields[9].values[0].value == (1.0, 2.0)
    assert node.fields[10].values[0].value == (3.0, 4.0, 5.0)
    assert node.fields[11].values[0].value == (6.0, 7.0, 8.0, 9.0, 10.0, 11.0)
    assert node.fields[12].values[0].value == (12.0, 13.0, 14.0)
    assert write_xb(parse_xb(payload, schema_provider=_fixed_codec_provider())) == payload

    with pytest.raises(ValueError, match="no public neutral value"):
        FieldValue(FieldType.OPAQUE_POINTER, 0)


def test_parse_xb_accepts_a_path_and_rejects_missing_catalog(tmp_path: Path) -> None:
    payload, _ = _embedded_document_bytes()
    path = tmp_path / "embedded.x_b"
    path.write_bytes(payload)

    assert parse_xb(path, schema_provider=_provider()).nodes[0].index == 1

    with pytest.raises(SchemaError) as captured:
        parse_xb(payload)

    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "schema.missing_base_schema"
    assert diagnostic.kind.value == "incomplete"
    assert diagnostic.node_type == 204
    assert diagnostic.details["schema"] == "13006"


def test_parse_xb_enforces_variable_limit_and_termination_boundary() -> None:
    payload, _ = _embedded_document_bytes()

    with pytest.raises(ParseError) as captured:
        parse_xb(
            payload,
            schema_provider=_provider(),
            limits=ParseLimits(max_variable_elements=1),
        )
    assert captured.value.diagnostic.code == "limits.exceeded"
    assert captured.value.diagnostic.details["resource"] == "field_elements"

    with pytest.raises(ParseError) as captured_trailing:
        parse_xb(payload + b"\x00", schema_provider=_provider())
    assert captured_trailing.value.diagnostic.code == "binary.trailing_bytes"


def test_write_xb_requires_a_parsed_document() -> None:
    with pytest.raises(TypeError, match="ParasolidDocument"):
        write_xb(b"not-a-document")  # type: ignore[arg-type]
