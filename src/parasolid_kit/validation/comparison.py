"""Typed reports for encoding-independent raw document comparison."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

ComparisonCategory = Literal["schema", "node_count", "topology", "field_value"]


@dataclass(frozen=True, slots=True)
class ComparisonDifference:
    """One semantic difference with source indices retained only as provenance."""

    code: str
    category: ComparisonCategory
    message: str
    node_type: int | None = None
    left_node_index: int | None = None
    right_node_index: int | None = None
    field_name: str | None = None
    value_index: int | None = None
    left_value: str | None = None
    right_value: str | None = None

    def __post_init__(self) -> None:
        if not self.code.startswith("comparison."):
            raise ValueError("comparison difference code must start with 'comparison.'")
        if self.category not in {"schema", "node_count", "topology", "field_value"}:
            raise ValueError("unsupported comparison category")
        if not self.message:
            raise ValueError("comparison difference message must not be empty")
        for name in ("node_type", "left_node_index", "right_node_index", "value_index"):
            value = getattr(self, name)
            if value is not None and value < 0:
                raise ValueError(f"{name} must be non-negative when present")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible difference record."""

        result: dict[str, object] = {
            "code": self.code,
            "category": self.category,
            "message": self.message,
        }
        for name in (
            "node_type",
            "left_node_index",
            "right_node_index",
            "field_name",
            "value_index",
            "left_value",
            "right_value",
        ):
            value = getattr(self, name)
            if value is not None:
                result[name] = value
        return result


@dataclass(frozen=True, slots=True)
class DocumentComparison:
    """Staged L3 comparison of two parsed raw documents."""

    equivalent: bool
    schema_key_equal: bool
    schema_coverage_equal: bool
    node_type_counts_equal: bool
    node_index_layout_equal: bool
    topology_equal: bool
    field_values_equal: bool
    left_node_count: int
    right_node_count: int
    compared_node_count: int
    difference_count: int
    differences_truncated: bool
    differences: tuple[ComparisonDifference, ...]

    def __post_init__(self) -> None:
        for name in (
            "left_node_count",
            "right_node_count",
            "compared_node_count",
            "difference_count",
        ):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ValueError(f"{name} must be a non-negative integer")
        if not isinstance(self.differences, tuple):
            object.__setattr__(self, "differences", tuple(self.differences))
        if not all(isinstance(item, ComparisonDifference) for item in self.differences):
            raise ValueError("differences must contain ComparisonDifference values")
        if len(self.differences) > self.difference_count:
            raise ValueError("retained differences cannot exceed the total difference count")
        if self.differences_truncated != (len(self.differences) < self.difference_count):
            raise ValueError("differences_truncated is inconsistent with retained differences")
        expected_equivalent = all(
            (
                self.schema_coverage_equal,
                self.node_type_counts_equal,
                self.topology_equal,
                self.field_values_equal,
            )
        )
        if self.equivalent != expected_equivalent:
            raise ValueError("equivalent is inconsistent with staged comparison results")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible L3 report."""

        return {
            "level": "L3",
            "equivalent": self.equivalent,
            "schema_key_equal": self.schema_key_equal,
            "schema_coverage_equal": self.schema_coverage_equal,
            "node_type_counts_equal": self.node_type_counts_equal,
            "node_index_layout_equal": self.node_index_layout_equal,
            "topology_equal": self.topology_equal,
            "field_values_equal": self.field_values_equal,
            "left_node_count": self.left_node_count,
            "right_node_count": self.right_node_count,
            "compared_node_count": self.compared_node_count,
            "difference_count": self.difference_count,
            "differences_truncated": self.differences_truncated,
            "differences": [item.to_dict() for item in self.differences],
        }
