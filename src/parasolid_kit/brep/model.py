"""Aggregate B-Rep source model and L4/L5 reports."""

from __future__ import annotations

from dataclasses import dataclass, fields, is_dataclass
from enum import Enum
from typing import Any, Literal, cast

from ..diagnostics import Diagnostic
from .geometry import CurveGeometry, PointGeometry, SurfaceGeometry
from .topology import (
    Body,
    BoundingBox,
    Edge,
    Face,
    HalfEdge,
    Loop,
    Region,
    Shell,
    Vertex,
)

BrepSourceFormat = Literal["binary", "text"]


@dataclass(frozen=True, slots=True)
class TopologyValidation:
    """L4 linked-graph validation results."""

    valid: bool
    closed_loop_count: int
    closed_edge_ring_count: int
    euler_characteristic: int


@dataclass(frozen=True, slots=True)
class BrepMetrics:
    """Kernel-free metrics; unavailable values remain ``None``."""

    bounding_box: BoundingBox | None
    surface_area: float | None
    volume: float | None


@dataclass(frozen=True, slots=True)
class BrepModel:
    """Complete Parasolid-native semantic view of one parsed raw document."""

    source_format: BrepSourceFormat
    schema_key: str
    complete: bool
    bodies: tuple[Body, ...]
    regions: tuple[Region, ...]
    shells: tuple[Shell, ...]
    faces: tuple[Face, ...]
    loops: tuple[Loop, ...]
    half_edges: tuple[HalfEdge, ...]
    edges: tuple[Edge, ...]
    vertices: tuple[Vertex, ...]
    points: tuple[PointGeometry, ...]
    curves: tuple[CurveGeometry, ...]
    surfaces: tuple[SurfaceGeometry, ...]
    topology: TopologyValidation
    metrics: BrepMetrics
    diagnostics: tuple[Diagnostic, ...]

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible semantic report."""

        return cast(dict[str, object], _json_value(self))


def _json_value(value: Any) -> Any:
    if isinstance(value, Enum):
        return value.value
    if is_dataclass(value) and not isinstance(value, type):
        return {item.name: _json_value(getattr(value, item.name)) for item in fields(value)}
    if isinstance(value, tuple):
        return [_json_value(item) for item in value]
    return value
