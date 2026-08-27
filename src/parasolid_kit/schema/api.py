"""Stable Python facade over the native schema resolver."""

from __future__ import annotations

import hashlib
import os
import sys
from collections.abc import Iterable, Mapping
from pathlib import Path
from typing import Any, TypeAlias

from .. import _core
from .._native import mapping_value as _mapping_value
from .._native import parse_error_from_native as _parse_error_from_native
from ..binary.header import ByteRange
from ..diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity, SourceLocation
from ..errors import SchemaError
from ..limits import DEFAULT_PARSE_LIMITS, ParseLimits
from .model import (
    FieldDefinition,
    FieldType,
    SchemaCoverageReport,
    SchemaEdit,
    SchemaEditKind,
    SchemaResolution,
    SchemaSource,
    TypeDefinition,
)
from .provider import SchemaCatalog

SchemaBlob: TypeAlias = bytes | bytearray | memoryview
SchemaCatalogSource: TypeAlias = str | os.PathLike[str] | bytes | bytearray | memoryview


def load_schema_catalog(
    source: SchemaCatalogSource,
    *,
    expected_schema_id: str | None = None,
    limits: ParseLimits = DEFAULT_PARSE_LIMITS,
) -> SchemaCatalog:
    """Load and validate one Parasolid ``sch_*.sch_txt`` base catalog.

    The Rust core parses the source grammar and declared counts. The Python
    facade retains the local source path and SHA-256 digest as provenance, but
    does not copy or redistribute the source catalog.
    """

    if expected_schema_id is not None and (
        not expected_schema_id
        or not expected_schema_id.isascii()
        or not expected_schema_id.isdigit()
    ):
        raise ValueError("expected_schema_id must contain only ASCII digits")
    data, source_path = _read_catalog_source(source, limits)
    response = _core._parse_schema_catalog(
        data,
        min(limits.max_file_size, sys.maxsize),
        min(limits.max_schema_types, sys.maxsize),
        min(limits.max_fields_per_type, sys.maxsize),
        min(limits.max_string_bytes, sys.maxsize),
    )
    if not isinstance(response, Mapping):
        raise RuntimeError("native schema catalog response is not a mapping")
    if response.get("ok") is False:
        raise _parse_error_from_native(
            _mapping_value(response, "error"),
            error_type=SchemaError,
        )
    if response.get("ok") is not True:
        raise RuntimeError("native schema catalog response has no boolean 'ok' field")
    value = _mapping_value(response, "value")
    definitions_value = value.get("definitions")
    if not isinstance(definitions_value, list):
        raise RuntimeError("native schema catalog definitions are not a list")
    schema_id = _str_value(value, "schema_id")
    if expected_schema_id is not None and schema_id != expected_schema_id:
        raise SchemaError(
            Diagnostic(
                code="schema.catalog_id_mismatch",
                severity=DiagnosticSeverity.ERROR,
                kind=DiagnosticKind.INVALID,
                message=(
                    f"schema catalog identifier {schema_id!r} does not match "
                    f"expected identifier {expected_schema_id!r}"
                ),
                location=SourceLocation(byte_offset=0),
                fatal=True,
                details={"expected": expected_schema_id, "actual": schema_id},
            )
        )
    return SchemaCatalog(
        schema_id=schema_id,
        definitions=tuple(
            _type_from_native(_require_mapping(item, "schema catalog definition"))
            for item in definitions_value
        ),
        modeller_version=_str_value(value, "modeller_version"),
        declared_max_node_type=_int_value(value, "declared_max_node_type"),
        declared_node_count=_int_value(value, "declared_node_count"),
        declared_field_count=_int_value(value, "declared_field_count"),
        declared_auxiliary_count=_int_value(value, "declared_auxiliary_count"),
        source_path=source_path,
        source_sha256=hashlib.sha256(data).hexdigest(),
    )


def _read_catalog_source(
    source: SchemaCatalogSource,
    limits: ParseLimits,
) -> tuple[bytes, str | None]:
    if isinstance(source, bytes):
        limits.ensure_file_size(len(source))
        return source, None
    if isinstance(source, (bytearray, memoryview)):
        data = bytes(source)
        limits.ensure_file_size(len(data))
        return data, None
    if not isinstance(source, (str, os.PathLike)):
        raise TypeError("source must be a path or bytes-like object")
    path = Path(source).expanduser().resolve()
    limits.ensure_file_size(path.stat().st_size)
    with path.open("rb") as stream:
        data = stream.read(limits.max_file_size + 1)
    limits.ensure_file_size(len(data))
    return data, str(path)


def resolve_schema_blob(
    source: SchemaBlob,
    *,
    node_type: int,
    base_type: TypeDefinition | None = None,
    limits: ParseLimits = DEFAULT_PARSE_LIMITS,
) -> SchemaResolution:
    """Resolve one first-occurrence full, delta, or unchanged schema blob.

    The blob starts immediately after its node type. Pass ``base_type`` exactly
    when the node type exists in the selected base schema.
    """

    if not isinstance(source, (bytes, bytearray, memoryview)):
        raise TypeError("source must be bytes-like")
    if (
        isinstance(node_type, bool)
        or not isinstance(node_type, int)
        or not 0 <= node_type <= 65_535
    ):
        raise ValueError("node_type must fit an unsigned 16-bit field")
    if base_type is not None and base_type.node_type != node_type:
        raise ValueError("base_type.node_type must match node_type")
    data = bytes(source)
    limits.ensure_file_size(len(data))

    if base_type is None:
        base_name: str | None = None
        base_description: str | None = None
        base_fields: list[tuple[str, str, int, int, bool]] | None = None
    else:
        base_name = base_type.name
        base_description = base_type.description
        base_fields = [
            (
                field.name,
                field.field_type.value,
                field.pointer_class,
                field.element_count,
                field.transmitted,
            )
            for field in base_type.fields
        ]

    response = _core._resolve_schema_blob(
        data,
        node_type,
        base_name,
        base_description,
        base_fields,
        limits.max_fields_per_type,
        limits.max_string_bytes,
        limits.max_schema_types,
    )
    if not isinstance(response, Mapping):
        raise RuntimeError("native schema response is not a mapping")
    if response.get("ok") is True:
        return _resolution_from_native(_mapping_value(response, "value"))
    if response.get("ok") is False:
        raise _parse_error_from_native(
            _mapping_value(response, "error"),
            error_type=SchemaError,
            node_type=node_type,
        )
    raise RuntimeError("native schema response has no boolean 'ok' field")


def schema_coverage(resolutions: Iterable[SchemaResolution]) -> SchemaCoverageReport:
    """Summarize effective definitions in Rust without parsing node bodies."""

    items = tuple(resolutions)
    if not all(isinstance(item, SchemaResolution) for item in items):
        raise TypeError("resolutions must contain SchemaResolution values")
    node_types = [item.definition.node_type for item in items]
    if len(set(node_types)) != len(node_types):
        raise ValueError("resolutions must contain each node type at most once")
    entries = [
        (
            item.definition.node_type,
            item.definition.source.value,
            len(item.definition.fields),
        )
        for item in items
    ]
    value = _core._schema_coverage(entries)
    if not isinstance(value, Mapping):
        raise RuntimeError("native schema coverage is not a mapping")
    return _coverage_from_native(value)


def _coverage_from_native(value: Mapping[str, Any]) -> SchemaCoverageReport:
    return SchemaCoverageReport(
        node_types=tuple(_int_list(value, "node_types")),
        field_count=_int_value(value, "field_count"),
        base_count=_int_value(value, "base_count"),
        unchanged_count=_int_value(value, "unchanged_count"),
        delta_count=_int_value(value, "delta_count"),
        full_count=_int_value(value, "full_count"),
    )


def _resolution_from_native(value: Mapping[str, Any]) -> SchemaResolution:
    edits_value = value.get("edits")
    if not isinstance(edits_value, list):
        raise RuntimeError("native schema edits are not a list")
    raw_schema = value.get("raw_schema")
    if not isinstance(raw_schema, bytes):
        raise RuntimeError("native raw schema is not bytes")
    return SchemaResolution(
        definition=_type_from_native(_mapping_value(value, "definition")),
        raw_schema=raw_schema,
        byte_range=_range_from_native(value.get("byte_range")),
        edits=tuple(_edit_from_native(item) for item in edits_value),
    )


def _type_from_native(value: Mapping[str, Any]) -> TypeDefinition:
    fields_value = value.get("fields")
    if not isinstance(fields_value, list):
        raise RuntimeError("native schema fields are not a list")
    return TypeDefinition(
        node_type=_int_value(value, "node_type"),
        name=_str_value(value, "name"),
        description=_str_value(value, "description"),
        variable=_bool_value(value, "variable"),
        fields=tuple(_field_from_native(item) for item in fields_value),
        source=SchemaSource(_str_value(value, "source")),
    )


def _field_from_native(value: object) -> FieldDefinition:
    if not isinstance(value, Mapping):
        raise RuntimeError("native schema field is not a mapping")
    return FieldDefinition(
        name=_str_value(value, "name"),
        field_type=FieldType(_str_value(value, "field_type")),
        pointer_class=_int_value(value, "pointer_class"),
        element_count=_int_value(value, "element_count"),
        transmitted=_bool_value(value, "transmitted"),
    )


def _edit_from_native(value: object) -> SchemaEdit:
    if not isinstance(value, Mapping):
        raise RuntimeError("native schema edit is not a mapping")
    field = value.get("field")
    return SchemaEdit(
        kind=SchemaEditKind(_str_value(value, "opcode")),
        byte_offset=_int_value(value, "offset"),
        field=None if field is None else _field_from_native(field),
    )


def _range_from_native(value: object) -> ByteRange:
    if (
        not isinstance(value, tuple)
        or len(value) != 2
        or any(isinstance(item, bool) or not isinstance(item, int) for item in value)
    ):
        raise RuntimeError("native schema response contains an invalid byte range")
    return ByteRange(value[0], value[1])


def _require_mapping(value: object, name: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise RuntimeError(f"native {name} is not a mapping")
    return value


def _str_value(value: Mapping[str, Any], key: str) -> str:
    item = value.get(key)
    if not isinstance(item, str):
        raise RuntimeError(f"native schema field {key!r} is not a string")
    return item


def _int_value(value: Mapping[str, Any], key: str) -> int:
    item = value.get(key)
    if isinstance(item, bool) or not isinstance(item, int):
        raise RuntimeError(f"native schema field {key!r} is not an integer")
    return item


def _bool_value(value: Mapping[str, Any], key: str) -> bool:
    item = value.get(key)
    if not isinstance(item, bool):
        raise RuntimeError(f"native schema field {key!r} is not a boolean")
    return item


def _int_list(value: Mapping[str, Any], key: str) -> list[int]:
    item = value.get(key)
    if not isinstance(item, list) or any(
        isinstance(element, bool) or not isinstance(element, int) for element in item
    ):
        raise RuntimeError(f"native schema field {key!r} is not an integer list")
    return item
