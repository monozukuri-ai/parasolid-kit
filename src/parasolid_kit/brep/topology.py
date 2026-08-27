"""Typed Parasolid topology values with raw-node provenance."""

from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass
from enum import Enum
from math import isfinite

from ..binary.header import ByteRange


class Sense(str, Enum):
    """Orientation relative to a natural curve or surface parameterization."""

    POSITIVE = "positive"
    NEGATIVE = "negative"
    UNKNOWN = "unknown"


class BodyKind(str, Enum):
    """Parasolid ``body_type`` classification."""

    SOLID = "solid"
    WIRE = "wire"
    SHEET = "sheet"
    GENERAL = "general"


class RegionKind(str, Enum):
    """Material or void region classification."""

    SOLID = "solid"
    VOID = "void"


@dataclass(frozen=True, slots=True)
class Vector3:
    """Cartesian vector in the source transmit units."""

    x: float
    y: float
    z: float

    def __post_init__(self) -> None:
        if any(isinstance(value, bool) or not isinstance(value, float) for value in self):
            raise ValueError("vector components must be floats")
        if not all(isfinite(value) for value in self):
            raise ValueError("vector components must be finite")

    def __iter__(self) -> Iterator[float]:
        return iter((self.x, self.y, self.z))

    def to_tuple(self) -> tuple[float, float, float]:
        """Return the three components in source order."""

        return (self.x, self.y, self.z)


@dataclass(frozen=True, slots=True)
class BoundingBox:
    """Axis-aligned bounds derived from topological vertex points."""

    minimum: Vector3
    maximum: Vector3

    @property
    def extents(self) -> Vector3:
        """Return maximum minus minimum on each axis."""

        return Vector3(
            self.maximum.x - self.minimum.x,
            self.maximum.y - self.minimum.y,
            self.maximum.z - self.minimum.z,
        )


@dataclass(frozen=True, slots=True)
class SourceNodeRef:
    """Provenance linking one semantic value to its raw transmit record."""

    node_index: int
    node_type: int
    type_name: str
    node_id: int | None
    byte_range: ByteRange

    def __post_init__(self) -> None:
        if isinstance(self.node_index, bool) or not isinstance(self.node_index, int):
            raise ValueError("node_index must be an integer")
        if self.node_index <= 0:
            raise ValueError("node_index must be positive")
        if isinstance(self.node_type, bool) or not isinstance(self.node_type, int):
            raise ValueError("node_type must be an integer")
        if self.node_type < 2:
            raise ValueError("node_type must identify a non-termination node")
        if not self.type_name:
            raise ValueError("type_name must not be empty")
        if self.node_id is not None and (
            isinstance(self.node_id, bool) or not isinstance(self.node_id, int)
        ):
            raise ValueError("node_id must be an integer or None")


@dataclass(frozen=True, slots=True)
class Body:
    """One Parasolid body root."""

    id: int
    kind: BodyKind
    size_resolution: float
    linear_resolution: float
    regions: tuple[int, ...]
    edges: tuple[int, ...]
    vertices: tuple[int, ...]
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class Region:
    """One material or void region owned by a body."""

    id: int
    kind: RegionKind
    body: int
    shells: tuple[int, ...]
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class Shell:
    """One connected boundary component of a region."""

    id: int
    region: int
    back_faces: tuple[int, ...]
    front_faces: tuple[int, ...]
    wire_edges: tuple[int, ...]
    isolated_vertex: int | None
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class Face:
    """One oriented subset of a surface."""

    id: int
    back_shell: int
    front_shell: int
    loops: tuple[int, ...]
    surface: int | None
    sense: Sense
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class Loop:
    """One ordered ring of half-edges bounding a face."""

    id: int
    face: int
    half_edges: tuple[int, ...]
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class HalfEdge:
    """One oriented use of an edge by a loop, including dummy fins."""

    id: int
    loop: int | None
    forward: int | None
    backward: int | None
    vertex: int | None
    other: int | None
    edge: int | None
    curve: int | None
    sense: Sense
    dummy: bool
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class Edge:
    """One bounded curve subset and its ring of oriented uses."""

    id: int
    owner: SourceNodeRef
    half_edges: tuple[int, ...]
    start_vertex: int | None
    end_vertex: int | None
    curve: int | None
    tolerance: float | None
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class Vertex:
    """One topological point linked to point geometry."""

    id: int
    point: int
    tolerance: float | None
    owner: SourceNodeRef
    source: SourceNodeRef
