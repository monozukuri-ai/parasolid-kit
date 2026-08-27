from __future__ import annotations

import pytest

from parasolid_kit import ParseLimits, SchemaError
from parasolid_kit.schema import (
    FieldDefinition,
    FieldType,
    SchemaEditKind,
    SchemaSource,
    TypeDefinition,
    resolve_schema_blob,
    schema_coverage,
)
from tests.support.parasolid_schema import embedded_field, full_schema


def _base_field(name: str) -> FieldDefinition:
    return FieldDefinition(
        name=name,
        field_type=FieldType.INTEGER,
        pointer_class=0,
        element_count=0,
        transmitted=True,
    )


def _base_type() -> TypeDefinition:
    return TypeDefinition(
        node_type=29,
        name="POINT",
        description="Point",
        variable=False,
        fields=(_base_field("a"), _base_field("b"), _base_field("c")),
        source=SchemaSource.BASE,
    )


def test_resolves_complete_type_204_definition_in_native_core() -> None:
    blob = full_schema(
        "INTERSECTION_DATA",
        "Intersection data",
        (
            embedded_field("uv_type", field_type="u"),
            embedded_field("values", field_type="f", element_count=1),
        ),
    )

    resolution = resolve_schema_blob(blob + b"node-data", node_type=204)

    assert resolution.consumed == 65
    assert resolution.raw_schema == blob
    assert resolution.definition.node_type == 204
    assert resolution.definition.name == "INTERSECTION_DATA"
    assert resolution.definition.source is SchemaSource.EMBEDDED_FULL
    assert resolution.definition.variable is True
    assert [field.field_type for field in resolution.definition.fields] == [
        FieldType.UNSIGNED_BYTE,
        FieldType.DOUBLE,
    ]
    assert resolution.definition.fields[1].element_count == 1
    assert resolution.edits == ()


def test_applies_all_delta_opcodes_and_preserves_edit_offsets() -> None:
    inserted = embedded_field("inserted", field_type="u")
    appended = embedded_field("appended", field_type="f")
    blob = b"".join((bytes((4,)), b"CDI", inserted, b"CA", appended, b"Z"))

    resolution = resolve_schema_blob(blob, node_type=29, base_type=_base_type())

    assert [field.name for field in resolution.definition.fields] == [
        "a",
        "inserted",
        "c",
        "appended",
    ]
    assert [edit.kind for edit in resolution.edits] == [
        SchemaEditKind.COPY,
        SchemaEditKind.DELETE,
        SchemaEditKind.INSERT,
        SchemaEditKind.COPY,
        SchemaEditKind.APPEND,
        SchemaEditKind.END,
    ]
    assert resolution.edits[0].byte_offset == 1
    assert resolution.definition.source is SchemaSource.EMBEDDED_DELTA


def test_reuses_base_definition_for_ff_marker() -> None:
    resolution = resolve_schema_blob(b"\xffnode-data", node_type=29, base_type=_base_type())

    assert resolution.consumed == 1
    assert resolution.definition.fields == _base_type().fields
    assert resolution.definition.source is SchemaSource.EMBEDDED_UNCHANGED


def test_unknown_delta_opcode_is_a_schema_error_at_exact_offset() -> None:
    with pytest.raises(SchemaError) as captured:
        resolve_schema_blob(b"\x01X", node_type=29, base_type=_base_type())

    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "schema.unknown_delta_opcode"
    assert diagnostic.location is not None
    assert diagnostic.location.byte_offset == 1
    assert diagnostic.node_type == 29
    assert diagnostic.details == {"field": "schema_opcode", "value": ord("X")}


def test_schema_limits_are_enforced_before_field_allocation() -> None:
    with pytest.raises(SchemaError) as captured:
        resolve_schema_blob(
            b"\x02",
            node_type=204,
            limits=ParseLimits(max_fields_per_type=1),
        )

    assert captured.value.diagnostic.code == "limits.exceeded"
    assert captured.value.diagnostic.details["resource"] == "schema_fields_per_type"


def test_schema_coverage_counts_sources_without_claiming_node_coverage() -> None:
    full = resolve_schema_blob(full_schema("EMPTY", "Empty", ()), node_type=204)
    unchanged = resolve_schema_blob(b"\xff", node_type=29, base_type=_base_type())

    report = schema_coverage((full, unchanged))

    assert report.node_types == (29, 204)
    assert report.resolved_type_count == 2
    assert report.field_count == 3
    assert report.unchanged_count == 1
    assert report.full_count == 1
    assert report.delta_count == 0

    with pytest.raises(ValueError, match="at most once"):
        schema_coverage((full, full))


def test_python_models_reject_pointer_and_variable_inconsistency() -> None:
    with pytest.raises(ValueError, match="POINTER_INDEX"):
        FieldDefinition(
            name="owner",
            field_type=FieldType.INTEGER,
            pointer_class=1011,
            element_count=0,
            transmitted=True,
        )

    with pytest.raises(ValueError, match="final field"):
        TypeDefinition(
            node_type=12,
            name="BODY",
            description="Body",
            variable=True,
            fields=(_base_field("value"),),
            source=SchemaSource.BASE,
        )
