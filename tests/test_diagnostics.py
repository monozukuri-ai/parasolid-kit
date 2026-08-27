from __future__ import annotations

import pytest

from parasolid_kit import (
    Diagnostic,
    DiagnosticKind,
    DiagnosticSeverity,
    ParseError,
    SourceLocation,
)


def test_diagnostic_serializes_only_known_source_coordinates() -> None:
    diagnostic = Diagnostic(
        code="binary.truncated_field",
        severity=DiagnosticSeverity.ERROR,
        kind=DiagnosticKind.INVALID,
        message="field extends beyond the source",
        location=SourceLocation(byte_offset=42),
        node_type=204,
        node_id=7,
        schema_key="SCH_3000310_30000_13006",
        fatal=True,
        details={"field": "values"},
    )

    assert diagnostic.to_dict() == {
        "code": "binary.truncated_field",
        "severity": "error",
        "kind": "invalid",
        "message": "field extends beyond the source",
        "fatal": True,
        "recoverable": False,
        "location": {"byte_offset": 42},
        "node_type": 204,
        "node_id": 7,
        "schema_key": "SCH_3000310_30000_13006",
        "details": {"field": "values"},
    }


def test_parse_error_includes_code_and_location() -> None:
    error = ParseError(
        Diagnostic(
            code="binary.trailing_bytes",
            severity="error",
            kind="invalid",
            message="bytes remain after the terminator",
            location=SourceLocation(byte_offset=128),
            fatal=True,
        )
    )

    assert str(error) == (
        "binary.trailing_bytes (byte_offset=128): bytes remain after the terminator"
    )


@pytest.mark.parametrize("code", ["INVALID", "missing_dot", "binary.Bad"])
def test_diagnostic_rejects_unstable_codes(code: str) -> None:
    with pytest.raises(ValueError, match="lowercase dotted"):
        Diagnostic(
            code=code,
            severity=DiagnosticSeverity.ERROR,
            kind=DiagnosticKind.INVALID,
            message="invalid",
        )


def test_source_location_rejects_empty_or_negative_coordinates() -> None:
    with pytest.raises(ValueError, match="at least one"):
        SourceLocation()
    with pytest.raises(ValueError, match="non-negative"):
        SourceLocation(byte_offset=-1)
