"""Aggregate B-Rep source model and L4/L5 reports."""

from __future__ import annotations

from dataclasses import dataclass, fields, is_dataclass
from enum import Enum
from typing import Any, Literal, cast

from ..diagnostics import Diagnostic
from .geometry import CurveGeometry, PointGeometry, SurfaceGeometry
from .topology import (
    Assembly,
    Attribute,
    AttributeOwnerKind,
    Body,
    BoundingBox,
    Edge,
    Face,
    HalfEdge,
    Instance,
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
    """Vertex bounding box and planar polygon metrics; unavailable values are ``None``."""

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
    # Assembly structure beside the topology; instance transforms are not applied.
    assemblies: tuple[Assembly, ...] = ()
    instances: tuple[Instance, ...] = ()
    # ATTRIBUTE nodes with their definition identifiers; bodies and faces also
    # list the ids of their own attributes.
    attributes: tuple[Attribute, ...] = ()

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible semantic report."""

        return cast(dict[str, object], _json_value(self))

    def attributes_of(
        self, kind: AttributeOwnerKind | str, entity_id: int
    ) -> tuple[Attribute, ...]:
        """Return the attributes attached to one mapped entity, in source order."""

        owner_kind = AttributeOwnerKind(kind)
        return tuple(
            item
            for item in self.attributes
            if item.owner.kind is owner_kind and item.owner.id == entity_id
        )

    def colour_of(
        self, kind: AttributeOwnerKind | str, entity_id: int
    ) -> tuple[float, float, float] | None:
        """The ``SDL/TYSA_COLOUR`` of one entity as (red, green, blue) in 0..1, if any.

        The first colour attribute whose first field holds three set reals
        counts; the reference assigns the attribute to faces and edges, and
        producers also write it on bodies.
        """

        for item in self.attributes_of(kind, entity_id):
            if item.identifier != "SDL/TYSA_COLOUR" or not item.fields:
                continue
            field = item.fields[0]
            if field.kind == "reals" and len(field.values) == 3:
                values = field.values
                if all(isinstance(v, float) for v in values):
                    return cast(tuple[float, float, float], tuple(values))
        return None

    def name_of(self, kind: AttributeOwnerKind | str, entity_id: int) -> str | None:
        """The ``SDL/TYSA_NAME`` of one entity, if any."""

        for item in self.attributes_of(kind, entity_id):
            if item.identifier != "SDL/TYSA_NAME" or not item.fields:
                continue
            field = item.fields[0]
            if field.kind in {"text", "unicode"} and field.value is not None:
                return field.value
        return None


def _json_value(value: Any) -> Any:
    if isinstance(value, Enum):
        return value.value
    if is_dataclass(value) and not isinstance(value, type):
        return {item.name: _json_value(getattr(value, item.name)) for item in fields(value)}
    if isinstance(value, tuple):
        return [_json_value(item) for item in value]
    return value
