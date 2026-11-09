"""Structured diagnostics for dependency-free operations; no optional imports."""

from __future__ import annotations

from dataclasses import replace
from math import isfinite

from ..diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity, SourceLocation
from ..errors import ParasolidError


def _detail(value):
    """Keep invalid arguments serializable, including NaN and oversized integers."""
    if isinstance(value, float) and not isfinite(value):
        return str(value)
    if isinstance(value, int) and value.bit_length() > 1024:
        return f"<integer with {value.bit_length()} bits>"
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    return f"<{type(value).__name__}>"


class _OperationError(ValueError, ParasolidError):
    """Retain ValueError/string compatibility while exposing a Diagnostic."""

    default_code = "brep.operation_failed"

    def __init__(
        self,
        message,
        *,
        code=None,
        kind=DiagnosticKind.INVALID,
        source=None,
        field=None,
        details=None,
    ):
        self.diagnostic = Diagnostic(
            code=code or self.default_code,
            severity=DiagnosticSeverity.ERROR,
            kind=kind,
            message=message,
            fatal=True,
            details={key: _detail(value) for key, value in (details or {}).items()},
        )
        self.add_context(source=source, field=field)
        super().__init__(message)

    def add_context(self, *, entity=None, source=None, schema_key=None, field=None, **details):
        """Add known outer context without replacing the more specific failing entity."""
        diagnostic = self.diagnostic
        if "node_index" in diagnostic.details:
            entity = source = None
        if entity is not None:
            source = getattr(entity, "source", source)
            if hasattr(entity, "id"):
                details.update(
                    entity_id=entity.id,
                    entity_kind=type(entity).__name__.removesuffix("Geometry").lower(),
                )
        if source is not None:
            details.update(node_index=source.node_index, type_name=source.type_name)
            if source.node_id is not None and source.node_id < 0:
                # Source tags are signed; Diagnostic.node_id has a nonnegative contract.
                details["source_node_id"] = source.node_id
        if field is not None:
            details["field"] = field
        self.diagnostic = replace(
            diagnostic,
            location=diagnostic.location
            or (
                SourceLocation(byte_offset=source.byte_range.start) if source is not None else None
            ),
            node_type=diagnostic.node_type
            if diagnostic.node_type is not None
            else (source.node_type if source is not None else None),
            node_id=diagnostic.node_id
            if diagnostic.node_id is not None
            else (
                source.node_id
                if source is not None and source.node_id is not None and source.node_id >= 0
                else None
            ),
            schema_key=diagnostic.schema_key or schema_key,
            details={
                **{key: _detail(value) for key, value in details.items()},
                **diagnostic.details,
            },
        )
        return self


def _with_operation_cause(diagnostic, error):
    """Keep an interop-owned outer code and preserve the operation's source/cause."""
    if not isinstance(error, _OperationError):
        return diagnostic
    cause = error.diagnostic
    return replace(
        diagnostic,
        location=cause.location or diagnostic.location,
        node_type=cause.node_type if cause.node_type is not None else diagnostic.node_type,
        node_id=cause.node_id if "node_index" in cause.details else diagnostic.node_id,
        schema_key=cause.schema_key or diagnostic.schema_key,
        details={**cause.details, "cause_code": cause.code, **diagnostic.details},
    )
