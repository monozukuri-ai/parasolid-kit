"""Stable Python facade over the private Rust parser binding."""

from __future__ import annotations

import os
import sys
from collections.abc import Mapping
from math import isfinite
from pathlib import Path
from typing import Any, Literal, TypeAlias, cast

from . import _core
from ._native import mapping_value as _mapping_value
from ._native import parse_error_from_native as _parse_error_from_native
from .binary.document import FieldValue, ParasolidDocument, RawField, RawNode, XbTermination
from .binary.header import ByteRange, XbBinaryFormat, XbHeader
from .brep import BrepModel
from .brep._native import brep_from_native as _brep_from_native
from .errors import ParseError, SchemaError
from .limits import DEFAULT_PARSE_LIMITS, ParseLimits
from .schema.api import (
    _coverage_from_native,
    _field_from_native,
    _resolution_from_native,
    _type_from_native,
)
from .schema.model import FieldType, SchemaKey
from .schema.provider import DirectorySchemaProvider, SchemaCatalog, SchemaProvider
from .summary import BrepSummary, ParsedBrep
from .text import XtHeader, XtTermination
from .validation import ComparisonDifference, DocumentComparison

ParasolidSource: TypeAlias = str | os.PathLike[str] | bytes | bytearray | memoryview
ReadSourceFormat: TypeAlias = Literal["auto", "x-b", "x-t"]


def inspect_xb(
    source: ParasolidSource,
    *,
    limits: ParseLimits = DEFAULT_PARSE_LIMITS,
) -> XbHeader:
    """Inspect an X_B header without claiming that its node stream parsed.

    ``source`` may be a filesystem path or an in-memory bytes-like object. File
    size is checked before and during reading, and the Rust core independently
    enforces the same bound.
    """

    data = _read_source(source, limits)
    response = _core._inspect_xb(
        data,
        min(limits.max_file_size, sys.maxsize),
        min(limits.max_string_bytes, sys.maxsize),
    )
    if not isinstance(response, Mapping):
        raise RuntimeError("native inspect response is not a mapping")
    if response.get("ok") is True:
        return _header_from_native(_mapping_value(response, "value"))
    if response.get("ok") is False:
        raise _parse_error_from_native(_mapping_value(response, "error"))
    raise RuntimeError("native inspect response has no boolean 'ok' field")


def inspect_xt(
    source: ParasolidSource,
    *,
    limits: ParseLimits = DEFAULT_PARSE_LIMITS,
) -> XtHeader:
    """Inspect an X_T header without claiming that its node stream parsed."""

    data = _read_source(source, limits)
    response = _core._inspect_xt(
        data,
        min(limits.max_file_size, sys.maxsize),
        min(limits.max_string_bytes, sys.maxsize),
    )
    if not isinstance(response, Mapping):
        raise RuntimeError("native inspect response is not a mapping")
    if response.get("ok") is True:
        return _xt_header_from_native(_mapping_value(response, "value"))
    if response.get("ok") is False:
        raise _parse_error_from_native(_mapping_value(response, "error"))
    raise RuntimeError("native inspect response has no boolean 'ok' field")


def parse_xb(
    source: ParasolidSource,
    *,
    schema_provider: SchemaProvider | None = None,
    limits: ParseLimits = DEFAULT_PARSE_LIMITS,
) -> ParasolidDocument:
    """Parse a complete neutral X_B node stream in the Rust core.

    The provider must return the exact numeric catalog named by the schema key.
    No compatible-version fallback or inferred field layout is attempted.
    """

    data = _read_source(source, limits)
    header = inspect_xb(data, limits=limits)
    schema_key = SchemaKey.parse(header.schema_key)
    if schema_provider is not None and not isinstance(schema_provider, SchemaProvider):
        raise TypeError("schema_provider must implement SchemaProvider")
    catalog = (
        None if schema_provider is None else schema_provider.get_schema(schema_key.provider_schema)
    )
    if catalog is not None and not isinstance(catalog, SchemaCatalog):
        raise TypeError("schema_provider.get_schema() must return SchemaCatalog or None")
    if catalog is not None and catalog.schema_id != schema_key.provider_schema:
        raise ValueError("schema provider returned a catalog with the wrong schema_id")

    schema_id, definitions = _catalog_to_native(catalog)
    response = _core._parse_xb(
        data,
        schema_id,
        definitions,
        min(limits.max_file_size, sys.maxsize),
        min(limits.max_nodes, sys.maxsize),
        min(limits.max_schema_types, sys.maxsize),
        min(limits.max_fields_per_type, sys.maxsize),
        min(limits.max_string_bytes, sys.maxsize),
        min(limits.max_variable_elements, sys.maxsize),
    )
    if not isinstance(response, Mapping):
        raise RuntimeError("native parse response is not a mapping")
    if response.get("ok") is True:
        return _document_from_native(_mapping_value(response, "value"))
    if response.get("ok") is False:
        error = _mapping_value(response, "error")
        error_type = SchemaError if _str_value(error, "code").startswith("schema.") else ParseError
        raise _parse_error_from_native(error, error_type=error_type)
    raise RuntimeError("native parse response has no boolean 'ok' field")


def parse_xt(
    source: ParasolidSource,
    *,
    schema_provider: SchemaProvider | None = None,
    limits: ParseLimits = DEFAULT_PARSE_LIMITS,
) -> ParasolidDocument:
    """Parse a complete X_T node stream in the Rust core.

    The provider must return the exact catalog named by the internal text stream;
    the human-oriented common-header ``SCH`` value is not used for selection.
    """

    data = _read_source(source, limits)
    header = inspect_xt(data, limits=limits)
    schema_key = SchemaKey.parse(header.schema_key)
    if schema_provider is not None and not isinstance(schema_provider, SchemaProvider):
        raise TypeError("schema_provider must implement SchemaProvider")
    catalog = (
        None if schema_provider is None else schema_provider.get_schema(schema_key.provider_schema)
    )
    if catalog is not None and not isinstance(catalog, SchemaCatalog):
        raise TypeError("schema_provider.get_schema() must return SchemaCatalog or None")
    if catalog is not None and catalog.schema_id != schema_key.provider_schema:
        raise ValueError("schema provider returned a catalog with the wrong schema_id")

    schema_id, definitions = _catalog_to_native(catalog)
    response = _core._parse_xt(
        data,
        schema_id,
        definitions,
        min(limits.max_file_size, sys.maxsize),
        min(limits.max_nodes, sys.maxsize),
        min(limits.max_schema_types, sys.maxsize),
        min(limits.max_fields_per_type, sys.maxsize),
        min(limits.max_string_bytes, sys.maxsize),
        min(limits.max_variable_elements, sys.maxsize),
    )
    if not isinstance(response, Mapping):
        raise RuntimeError("native parse response is not a mapping")
    if response.get("ok") is True:
        return _document_from_native(_mapping_value(response, "value"))
    if response.get("ok") is False:
        error = _mapping_value(response, "error")
        error_type = SchemaError if _str_value(error, "code").startswith("schema.") else ParseError
        raise _parse_error_from_native(error, error_type=error_type)
    raise RuntimeError("native parse response has no boolean 'ok' field")


def write_xb(document: ParasolidDocument) -> bytes:
    """Return a byte-exact reconstruction of an unmodified parsed X_B document."""

    if not isinstance(document, ParasolidDocument):
        raise TypeError("document must be a ParasolidDocument")
    if document.format != "binary":
        raise ValueError("write_xb requires a document parsed from X_B")
    output = _core._write_xb(document._native_document)
    if not isinstance(output, bytes):
        raise RuntimeError("native writer response is not bytes")
    return output


def map_brep(
    document: ParasolidDocument,
    *,
    limits: ParseLimits = DEFAULT_PARSE_LIMITS,
) -> BrepModel:
    """Map a parsed X_T/X_B document to the Parasolid-native B-Rep source model.

    Required topology and geometry are validated strictly in Rust. Geometry
    types whose raw records are valid but not yet decoded remain available as
    explicit unsupported definitions and make ``model.complete`` false.
    ``limits.max_diagnostics`` bounds retained unsupported-geometry reports.
    """

    if not isinstance(document, ParasolidDocument):
        raise TypeError("document must be a ParasolidDocument")
    if not isinstance(limits, ParseLimits):
        raise TypeError("limits must be a ParseLimits value")
    native_function = _core._map_xb_brep if document.format == "binary" else _core._map_xt_brep
    response = native_function(
        document._native_document,
        min(limits.max_diagnostics, sys.maxsize),
    )
    if not isinstance(response, Mapping):
        raise RuntimeError("native B-Rep response is not a mapping")
    if response.get("ok") is True:
        return _brep_from_native(_mapping_value(response, "value"))
    if response.get("ok") is False:
        raise _parse_error_from_native(_mapping_value(response, "error"))
    raise RuntimeError("native B-Rep response has no boolean 'ok' field")


def read_brep(
    source: ParasolidSource,
    *,
    schema_provider: SchemaProvider | None = None,
    schema_dir: str | os.PathLike[str] | None = None,
    source_format: ReadSourceFormat = "auto",
    limits: ParseLimits = DEFAULT_PARSE_LIMITS,
) -> ParsedBrep:
    """Parse and map one X_T/X_B source through the complete B-Rep pipeline.

    Pass either an explicit ``schema_provider`` or a caller-owned
    ``schema_dir``. The directory provider loads only the exact
    ``sch_<provider-schema>.sch_txt`` filename selected by the internal stream
    key. Neither argument is required for a self-contained stream whose
    embedded definitions are sufficient, but they are mutually exclusive.
    """

    if not isinstance(limits, ParseLimits):
        raise TypeError("limits must be a ParseLimits value")
    if schema_provider is not None and schema_dir is not None:
        raise ValueError("schema_provider and schema_dir are mutually exclusive")
    if schema_provider is not None and not isinstance(schema_provider, SchemaProvider):
        raise TypeError("schema_provider must implement SchemaProvider")

    data = _read_source(source, limits)
    resolved_format = _read_source_format(source, data, source_format)
    parser = parse_xb if resolved_format == "binary" else parse_xt
    inspector = inspect_xb if resolved_format == "binary" else inspect_xt

    provider = schema_provider
    if schema_dir is not None:
        directory_provider = DirectorySchemaProvider(schema_dir, limits=limits)
        header = inspector(data, limits=limits)
        required_schema = SchemaKey.parse(header.schema_key).provider_schema
        if directory_provider.get_schema(required_schema) is None:
            raise FileNotFoundError(
                "required exact schema catalog does not exist: "
                f"{directory_provider.catalog_path(required_schema)}"
            )
        provider = directory_provider

    document = parser(data, schema_provider=provider, limits=limits)
    model = map_brep(document, limits=limits)
    summary = BrepSummary.from_parsed(document, model)
    return ParsedBrep(document=document, brep=model, summary=summary)


def compare_documents(
    left: ParasolidDocument,
    right: ParasolidDocument,
    *,
    absolute_tolerance: float = 1.0e-12,
    relative_tolerance: float = 1.0e-12,
    max_differences: int = 1_000,
) -> DocumentComparison:
    """Compare parsed documents without requiring equal bytes, order, or node indices."""

    if not isinstance(left, ParasolidDocument) or not isinstance(right, ParasolidDocument):
        raise TypeError("left and right must be ParasolidDocument values")
    absolute_tolerance = _comparison_tolerance(absolute_tolerance, "absolute_tolerance")
    relative_tolerance = _comparison_tolerance(relative_tolerance, "relative_tolerance")
    if isinstance(max_differences, bool) or not isinstance(max_differences, int):
        raise TypeError("max_differences must be an integer")
    if max_differences <= 0:
        raise ValueError("max_differences must be positive")

    native_function = {
        ("binary", "binary"): _core._compare_xb_xb,
        ("binary", "text"): _core._compare_xb_xt,
        ("text", "binary"): _core._compare_xt_xb,
        ("text", "text"): _core._compare_xt_xt,
    }[(left.format, right.format)]
    response = native_function(
        left._native_document,
        right._native_document,
        absolute_tolerance,
        relative_tolerance,
        min(max_differences, sys.maxsize),
    )
    if not isinstance(response, Mapping):
        raise RuntimeError("native comparison response is not a mapping")
    if response.get("ok") is True:
        return _comparison_from_native(_mapping_value(response, "value"))
    if response.get("ok") is False:
        raise _parse_error_from_native(_mapping_value(response, "error"))
    raise RuntimeError("native comparison response has no boolean 'ok' field")


def _catalog_to_native(
    catalog: SchemaCatalog | None,
) -> tuple[str | None, list[tuple[int, str, str, list[tuple[str, str, int, int, bool]]]] | None]:
    if catalog is None:
        return None, None
    definitions = [
        (
            definition.node_type,
            definition.name,
            definition.description,
            [
                (
                    field.name,
                    field.field_type.value,
                    field.pointer_class,
                    field.element_count,
                    field.transmitted,
                )
                for field in definition.fields
            ],
        )
        for definition in catalog.definitions
    ]
    return catalog.schema_id, definitions


def _comparison_tolerance(value: float, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise TypeError(f"{name} must be a float")
    value = float(value)
    if not isfinite(value) or value < 0.0:
        raise ValueError(f"{name} must be finite and non-negative")
    return value


def _comparison_from_native(value: Mapping[str, Any]) -> DocumentComparison:
    differences_value = value.get("differences")
    if not isinstance(differences_value, list):
        raise RuntimeError("native comparison differences are not a list")
    return DocumentComparison(
        equivalent=_bool_value(value, "equivalent"),
        schema_key_equal=_bool_value(value, "schema_key_equal"),
        schema_coverage_equal=_bool_value(value, "schema_coverage_equal"),
        node_type_counts_equal=_bool_value(value, "node_type_counts_equal"),
        node_index_layout_equal=_bool_value(value, "node_index_layout_equal"),
        topology_equal=_bool_value(value, "topology_equal"),
        field_values_equal=_bool_value(value, "field_values_equal"),
        left_node_count=_int_value(value, "left_node_count"),
        right_node_count=_int_value(value, "right_node_count"),
        compared_node_count=_int_value(value, "compared_node_count"),
        difference_count=_int_value(value, "difference_count"),
        differences_truncated=_bool_value(value, "differences_truncated"),
        differences=tuple(_comparison_difference_from_native(item) for item in differences_value),
    )


def _comparison_difference_from_native(value: object) -> ComparisonDifference:
    mapping = _require_mapping(value, "comparison difference")
    return ComparisonDifference(
        code=_str_value(mapping, "code"),
        category=cast(Any, _str_value(mapping, "category")),
        message=_str_value(mapping, "message"),
        node_type=_optional_int_value(mapping, "node_type"),
        left_node_index=_optional_int_value(mapping, "left_node_index"),
        right_node_index=_optional_int_value(mapping, "right_node_index"),
        field_name=_optional_str_value(mapping, "field_name"),
        value_index=_optional_int_value(mapping, "value_index"),
        left_value=_optional_str_value(mapping, "left_value"),
        right_value=_optional_str_value(mapping, "right_value"),
    )


def _document_from_native(value: Mapping[str, Any]) -> ParasolidDocument:
    schemas_value = value.get("schemas")
    nodes_value = value.get("nodes")
    raw_bytes = value.get("raw_bytes")
    if not isinstance(schemas_value, list):
        raise RuntimeError("native document schemas are not a list")
    if not isinstance(nodes_value, list):
        raise RuntimeError("native document nodes are not a list")
    if not isinstance(raw_bytes, bytes):
        raise RuntimeError("native document raw_bytes is not bytes")
    native_document = value.get("native_document")
    if native_document is None:
        raise RuntimeError("native document handle is missing")
    document_format = _str_value(value, "format")
    if document_format == "binary":
        header: XbHeader | XtHeader = _header_from_native(_mapping_value(value, "header"))
        terminator: XbTermination | XtTermination = _termination_from_native(
            _mapping_value(value, "terminator")
        )
    elif document_format == "text":
        header = _xt_header_from_native(_mapping_value(value, "header"))
        terminator = _xt_termination_from_native(_mapping_value(value, "terminator"))
    else:
        raise RuntimeError("native document has an unsupported source format")
    return ParasolidDocument(
        format=cast(Any, document_format),
        header=header,
        schema_key=_schema_key_from_native(_mapping_value(value, "schema_key")),
        schemas=tuple(
            _resolution_from_native(_require_mapping(item, "schema resolution"))
            for item in schemas_value
        ),
        nodes=tuple(_node_from_native(item) for item in nodes_value),
        terminator=terminator,
        schema_coverage=_coverage_from_native(_mapping_value(value, "schema_coverage")),
        raw_bytes=raw_bytes,
        _native_document=native_document,
    )


def _schema_key_from_native(value: Mapping[str, Any]) -> SchemaKey:
    base = value.get("base")
    if base is not None and not isinstance(base, str):
        raise RuntimeError("native schema key base is not a string or None")
    schema_key = SchemaKey(
        raw=_str_value(value, "raw"),
        modeller=_str_value(value, "modeller"),
        effective=_str_value(value, "effective"),
        base=base,
    )
    if _str_value(value, "provider_schema") != schema_key.provider_schema:
        raise RuntimeError("native schema provider identifier is inconsistent")
    return schema_key


def _node_from_native(value: object) -> RawNode:
    mapping = _require_mapping(value, "node")
    fields_value = mapping.get("fields")
    if not isinstance(fields_value, list):
        raise RuntimeError("native node fields are not a list")
    first_schema = mapping.get("first_schema")
    return RawNode(
        node_type=_int_value(mapping, "node_type"),
        index=_int_value(mapping, "index"),
        variable_length=_optional_int_value(mapping, "variable_length"),
        definition=_type_from_native(_mapping_value(mapping, "definition")),
        first_schema=(
            None
            if first_schema is None
            else _resolution_from_native(_require_mapping(first_schema, "first schema"))
        ),
        fields=tuple(_field_record_from_native(item) for item in fields_value),
        byte_range=_range_value(mapping.get("byte_range")),
    )


def _field_record_from_native(value: object) -> RawField:
    mapping = _require_mapping(value, "field record")
    values = mapping.get("values")
    if not isinstance(values, list):
        raise RuntimeError("native field values are not a list")
    return RawField(
        definition=_field_from_native(_mapping_value(mapping, "definition")),
        values=tuple(_field_value_from_native(item) for item in values),
        byte_range=_range_value(mapping.get("byte_range")),
    )


def _field_value_from_native(value: object) -> FieldValue:
    mapping = _require_mapping(value, "field value")
    field_type = FieldType(_str_value(mapping, "field_type"))
    item = mapping.get("value")
    if isinstance(item, list):
        if any(
            component is not None
            and (isinstance(component, bool) or not isinstance(component, float))
            for component in item
        ):
            raise RuntimeError("native composite field contains an invalid component")
        item = tuple(item)
    elif item is not None and not isinstance(item, (bool, int, float)):
        raise RuntimeError("native scalar field contains an invalid value")
    return FieldValue(field_type=field_type, value=cast(Any, item))


def _termination_from_native(value: Mapping[str, Any]) -> XbTermination:
    return XbTermination(
        index=_int_value(value, "index"),
        byte_range=_range_value(value.get("byte_range")),
    )


def _xt_termination_from_native(value: Mapping[str, Any]) -> XtTermination:
    return XtTermination(
        index=_int_value(value, "index"),
        byte_range=_range_value(value.get("byte_range")),
    )


def _require_mapping(value: object, name: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise RuntimeError(f"native {name} is not a mapping")
    return value


def _read_source(source: ParasolidSource, limits: ParseLimits) -> bytes:
    if isinstance(source, bytes):
        limits.ensure_file_size(len(source))
        return source
    if isinstance(source, (bytearray, memoryview)):
        data = bytes(source)
        limits.ensure_file_size(len(data))
        return data
    if not isinstance(source, (str, os.PathLike)):
        raise TypeError("source must be a path or bytes-like object")

    path = Path(source)
    limits.ensure_file_size(path.stat().st_size)
    with path.open("rb") as stream:
        data = stream.read(limits.max_file_size + 1)
    limits.ensure_file_size(len(data))
    return data


def _read_source_format(
    source: ParasolidSource,
    data: bytes,
    requested: ReadSourceFormat,
) -> Literal["binary", "text"]:
    if requested == "x-b":
        return "binary"
    if requested == "x-t":
        return "text"
    if requested != "auto":
        raise ValueError("source_format must be 'auto', 'x-b', or 'x-t'")

    if isinstance(source, (str, os.PathLike)):
        suffix = Path(source).suffix.lower()
        if suffix in {".x_b", ".xb"}:
            return "binary"
        if suffix in {".x_t", ".xt"}:
            return "text"

    prefix = data[:4096]
    if prefix.startswith(b"PS\0\0") or b"\nPS\0\0" in prefix:
        return "binary"
    if prefix.startswith(b"T") or prefix.startswith(b"**PART"):
        return "text"
    raise ValueError("cannot identify source encoding; pass source_format='x-b' or 'x-t'")


def _header_from_native(value: Mapping[str, Any]) -> XbHeader:
    text_range = value.get("text_header_range")
    return XbHeader(
        signature=_bytes_value(value, "signature"),
        binary_format=XbBinaryFormat(_str_value(value, "binary_format")),
        modeller_version=_str_value(value, "modeller_version"),
        schema_key=_str_value(value, "schema_key"),
        user_field_size=_int_value(value, "user_field_size"),
        schema_max_type=_optional_int_value(value, "schema_max_type"),
        file_size=_int_value(value, "file_size"),
        text_header_range=None if text_range is None else _range_value(text_range),
        binary_header_range=_range_value(value.get("binary_header_range")),
        header_range=_range_value(value.get("header_range")),
    )


def _xt_header_from_native(value: Mapping[str, Any]) -> XtHeader:
    common_range = value.get("common_header_range")
    return XtHeader(
        flag=_str_value(value, "flag"),
        modeller_version=_str_value(value, "modeller_version"),
        schema_key=_str_value(value, "schema_key"),
        user_field_size=_int_value(value, "user_field_size"),
        schema_max_type=_optional_int_value(value, "schema_max_type"),
        file_size=_int_value(value, "file_size"),
        common_header_range=None if common_range is None else _range_value(common_range),
        text_stream_header_range=_range_value(value.get("text_stream_header_range")),
        header_range=_range_value(value.get("header_range")),
    )


def _str_value(value: Mapping[str, Any], key: str) -> str:
    item = value.get(key)
    if not isinstance(item, str):
        raise RuntimeError(f"native inspect response field {key!r} is not a string")
    return item


def _optional_str_value(value: Mapping[str, Any], key: str) -> str | None:
    item = value.get(key)
    if item is None:
        return None
    if not isinstance(item, str):
        raise RuntimeError(f"native response field {key!r} is not a string or None")
    return item


def _bool_value(value: Mapping[str, Any], key: str) -> bool:
    item = value.get(key)
    if not isinstance(item, bool):
        raise RuntimeError(f"native response field {key!r} is not a boolean")
    return item


def _int_value(value: Mapping[str, Any], key: str) -> int:
    item = value.get(key)
    if isinstance(item, bool) or not isinstance(item, int):
        raise RuntimeError(f"native inspect response field {key!r} is not an integer")
    return item


def _optional_int_value(value: Mapping[str, Any], key: str) -> int | None:
    item = value.get(key)
    if item is None:
        return None
    if isinstance(item, bool) or not isinstance(item, int):
        raise RuntimeError(f"native inspect response field {key!r} is not an integer or None")
    return item


def _bytes_value(value: Mapping[str, Any], key: str) -> bytes:
    item = value.get(key)
    if not isinstance(item, bytes):
        raise RuntimeError(f"native inspect response field {key!r} is not bytes")
    return item


def _range_value(value: object) -> ByteRange:
    if (
        not isinstance(value, tuple)
        or len(value) != 2
        or any(isinstance(item, bool) or not isinstance(item, int) for item in value)
    ):
        raise RuntimeError("native inspect response contains an invalid byte range")
    return ByteRange(start=value[0], end=value[1])
