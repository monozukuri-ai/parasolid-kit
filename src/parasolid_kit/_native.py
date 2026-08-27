"""Shared validation for private native response mappings."""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any, cast

from .diagnostics import (
    Diagnostic,
    DiagnosticKind,
    DiagnosticSeverity,
    JsonScalar,
    SourceLocation,
)
from .errors import ParseError


def mapping_value(value: Mapping[str, Any], key: str) -> Mapping[str, Any]:
    """Return one required nested mapping from a native response."""

    item = value.get(key)
    if not isinstance(item, Mapping):
        raise RuntimeError(f"native response field {key!r} is not a mapping")
    return item


def parse_error_from_native(
    value: Mapping[str, Any],
    *,
    error_type: type[ParseError] = ParseError,
    node_type: int | None = None,
) -> ParseError:
    """Convert one validated native error mapping into the public hierarchy."""

    code = _str_value(value, "code")
    details_value = value.get("details", {})
    details: dict[str, JsonScalar] = {}
    if isinstance(details_value, Mapping):
        for key, item in details_value.items():
            if isinstance(key, str) and (item is None or isinstance(item, (str, int, float, bool))):
                details[key] = cast(JsonScalar, item)
    if code.startswith("limits."):
        kind = DiagnosticKind.LIMIT
    elif code in ("schema.missing_base_schema", "schema.missing_type_definition"):
        kind = DiagnosticKind.INCOMPLETE
    elif code in ("schema.unsupported_field_type", "node.unsupported_user_fields"):
        kind = DiagnosticKind.UNSUPPORTED
    else:
        kind = DiagnosticKind.INVALID
    resolved_node_type = node_type
    if resolved_node_type is None:
        detail_node_type = details.get("node_type")
        if isinstance(detail_node_type, int) and not isinstance(detail_node_type, bool):
            resolved_node_type = detail_node_type
    return error_type(
        Diagnostic(
            code=code,
            severity=DiagnosticSeverity.ERROR,
            kind=kind,
            message=_str_value(value, "message"),
            location=SourceLocation(byte_offset=_int_value(value, "offset")),
            node_type=resolved_node_type,
            fatal=True,
            details=details,
        )
    )


def _str_value(value: Mapping[str, Any], key: str) -> str:
    item = value.get(key)
    if not isinstance(item, str):
        raise RuntimeError(f"native error field {key!r} is not a string")
    return item


def _int_value(value: Mapping[str, Any], key: str) -> int:
    item = value.get(key)
    if isinstance(item, bool) or not isinstance(item, int):
        raise RuntimeError(f"native error field {key!r} is not an integer")
    return item
