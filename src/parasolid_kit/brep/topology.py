"""Typed Parasolid topology values with raw-node provenance."""

from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass
from enum import Enum
from math import isfinite
from typing import ClassVar

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


class AssemblyKind(str, Enum):
    """Parasolid assembly ``type`` classification."""

    COLLECTIVE = "collective"
    CONJUNCTIVE = "conjunctive"
    DISJUNCTIVE = "disjunctive"


class InstancePartKind(str, Enum):
    """Which model collection holds the part positioned by an instance."""

    BODY = "body"
    ASSEMBLY = "assembly"


@dataclass(frozen=True, slots=True)
class Transform:
    """Placement stored on a ``TRANSFORM`` node.

    A part point ``x`` maps into the assembly as
    ``(rotation · x + translation) · scale``; ``rotation`` holds the stored rows
    and ``flags`` the stored component bits named by the class constants.
    """

    TRANSLATION: ClassVar[int] = 0b00001
    ROTATION: ClassVar[int] = 0b00010
    SCALING: ClassVar[int] = 0b00100
    REFLECTION: ClassVar[int] = 0b01000
    GENERAL_AFFINE: ClassVar[int] = 0b10000

    rotation: tuple[
        tuple[float, float, float], tuple[float, float, float], tuple[float, float, float]
    ]
    translation: Vector3
    scale: float
    flags: int
    perspective: Vector3
    source: SourceNodeRef

    def has(self, flag: int) -> bool:
        """Return whether a component flag is set."""

        return bool(self.flags & flag)

    def apply(self, point: Vector3) -> Vector3:
        """Map a point of the part into the assembly."""

        rotated = [row[0] * point.x + row[1] * point.y + row[2] * point.z for row in self.rotation]
        return Vector3(
            (rotated[0] + self.translation.x) * self.scale,
            (rotated[1] + self.translation.y) * self.scale,
            (rotated[2] + self.translation.z) * self.scale,
        )


@dataclass(frozen=True, slots=True)
class Assembly:
    """One assembly root: a part which positions instances of bodies and assemblies.

    The structure sits beside the B-Rep topology; instance transforms are not
    applied to the bodies' geometry.
    """

    id: int
    kind: AssemblyKind
    size_resolution: float
    linear_resolution: float
    instances: tuple[int, ...]
    referencing_instances: tuple[int, ...]
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class Instance:
    """One placement of a body or sub-assembly within an assembly."""

    id: int
    negative: bool
    part_kind: InstancePartKind
    part: int
    transform: Transform | None
    assembly: int
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
