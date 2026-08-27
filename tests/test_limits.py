from __future__ import annotations

import pytest

from parasolid_kit import LimitExceededError, ParseLimits, SourceLocation


def test_parse_limits_accept_values_at_the_boundary() -> None:
    limits = ParseLimits(max_file_size=4)

    limits.ensure_file_size(4)


def test_parse_limits_raise_structured_error_above_the_boundary() -> None:
    limits = ParseLimits(max_file_size=4)

    with pytest.raises(LimitExceededError) as captured:
        limits.ensure_file_size(5, location=SourceLocation(byte_offset=0))

    error = captured.value
    assert error.resource == "file_size"
    assert error.actual == 5
    assert error.limit == 4
    assert error.diagnostic.to_dict()["kind"] == "limit"
    assert error.diagnostic.to_dict()["location"] == {"byte_offset": 0}


@pytest.mark.parametrize("value", [0, -1, True, 1.5])
def test_parse_limits_require_positive_integers(value: object) -> None:
    with pytest.raises(ValueError, match="positive integer"):
        ParseLimits(max_nodes=value)  # type: ignore[arg-type]


@pytest.mark.parametrize("value", [-1, True, 1.5])
def test_ensure_within_rejects_invalid_actual_values(value: object) -> None:
    with pytest.raises(ValueError, match="non-negative integer"):
        ParseLimits.ensure_within(
            resource="nodes",
            actual=value,  # type: ignore[arg-type]
            limit=10,
        )
