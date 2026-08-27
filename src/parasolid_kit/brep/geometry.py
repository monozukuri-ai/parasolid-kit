"""Typed analytic, NURBS, trim, and intersection geometry."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import TypeAlias

from .topology import Sense, SourceNodeRef, Vector3


class CurveKind(str, Enum):
    """Supported curve definition categories."""

    LINE = "line"
    CIRCLE = "circle"
    ELLIPSE = "ellipse"
    PARABOLA = "parabola"
    HYPERBOLA = "hyperbola"
    TRIMMED = "trimmed"
    NURBS = "nurbs"
    SURFACE_PARAMETRIC = "surface_parametric"
    INTERSECTION = "intersection"
    UNSUPPORTED = "unsupported"


class SurfaceKind(str, Enum):
    """Supported surface definition categories."""

    PLANE = "plane"
    CYLINDER = "cylinder"
    CONE = "cone"
    SPHERE = "sphere"
    TORUS = "torus"
    BLENDED_EDGE = "blended_edge"
    BLEND_BOUNDARY = "blend_boundary"
    OFFSET = "offset"
    NURBS = "nurbs"
    UNSUPPORTED = "unsupported"


class BlendType(str, Enum):
    """Parasolid exact blend construction."""

    ROLLING_BALL = "rolling_ball"
    CLIFF_EDGE = "cliff_edge"


@dataclass(frozen=True, slots=True)
class PointGeometry:
    """One exact point in source transmit units."""

    id: int
    position: Vector3
    owner: SourceNodeRef | None
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class LineCurve:
    point: Vector3
    direction: Vector3


@dataclass(frozen=True, slots=True)
class CircleCurve:
    center: Vector3
    normal: Vector3
    x_axis: Vector3
    radius: float


@dataclass(frozen=True, slots=True)
class EllipseCurve:
    center: Vector3
    normal: Vector3
    x_axis: Vector3
    major_radius: float
    minor_radius: float


@dataclass(frozen=True, slots=True)
class ParabolaCurve:
    origin: Vector3
    normal: Vector3
    x_axis: Vector3
    focal_length: float


@dataclass(frozen=True, slots=True)
class HyperbolaCurve:
    origin: Vector3
    normal: Vector3
    x_axis: Vector3
    transverse_radius: float
    conjugate_radius: float


@dataclass(frozen=True, slots=True)
class TrimmedCurve:
    basis_curve: int
    start_point: Vector3
    end_point: Vector3
    start_parameter: float
    end_parameter: float


@dataclass(frozen=True, slots=True)
class NurbsCurve:
    degree: int
    control_vertex_count: int
    vertex_dimension: int
    knot_type: int
    periodic: bool
    closed: bool
    rational: bool
    curve_form: int
    control_vertices: tuple[tuple[float, ...], ...]
    knots: tuple[float, ...]
    knot_multiplicities: tuple[int, ...]
    sources: tuple[SourceNodeRef, ...]


@dataclass(frozen=True, slots=True)
class SurfaceParametricCurve:
    surface: int
    parameter_curve: int
    original_curve: int | None
    tolerance_to_original: float | None


@dataclass(frozen=True, slots=True)
class IntersectionCurve:
    surfaces: tuple[int, int]
    chart: SourceNodeRef
    start: SourceNodeRef
    end: SourceNodeRef
    intersection_data: SourceNodeRef | None


@dataclass(frozen=True, slots=True)
class UnsupportedGeometry:
    """A retained source geometry node whose semantics are not implemented."""

    type_name: str


CurveDefinition: TypeAlias = (
    LineCurve
    | CircleCurve
    | EllipseCurve
    | ParabolaCurve
    | HyperbolaCurve
    | TrimmedCurve
    | NurbsCurve
    | SurfaceParametricCurve
    | IntersectionCurve
    | UnsupportedGeometry
)


@dataclass(frozen=True, slots=True)
class CurveGeometry:
    """One curve with orientation, typed definition, and source provenance."""

    id: int
    sense: Sense
    owner: SourceNodeRef | None
    kind: CurveKind
    definition: CurveDefinition
    source: SourceNodeRef


@dataclass(frozen=True, slots=True)
class PlaneSurface:
    point: Vector3
    normal: Vector3
    x_axis: Vector3


@dataclass(frozen=True, slots=True)
class CylinderSurface:
    point: Vector3
    axis: Vector3
    radius: float
    x_axis: Vector3


@dataclass(frozen=True, slots=True)
class ConeSurface:
    point: Vector3
    axis: Vector3
    radius: float
    sin_half_angle: float
    cos_half_angle: float
    x_axis: Vector3


@dataclass(frozen=True, slots=True)
class SphereSurface:
    center: Vector3
    radius: float
    axis: Vector3
    x_axis: Vector3


@dataclass(frozen=True, slots=True)
class TorusSurface:
    center: Vector3
    axis: Vector3
    major_radius: float
    minor_radius: float
    x_axis: Vector3


@dataclass(frozen=True, slots=True)
class BlendedEdgeSurface:
    """Exact rolling-ball or cliff-edge blend surface."""

    blend_type: BlendType
    supporting_surfaces: tuple[int, int]
    spine_curve: int
    ranges: tuple[float, float]
    thumb_weights: tuple[float, float]
    boundary_surfaces: tuple[int | None, int | None]
    start: SourceNodeRef | None
    end: SourceNodeRef | None


@dataclass(frozen=True, slots=True)
class BlendBoundarySurface:
    """Construction surface selecting one support side of a blend."""

    boundary_index: int
    blend_surface: int


@dataclass(frozen=True, slots=True)
class OffsetSurface:
    basis_surface: int
    offset: float


@dataclass(frozen=True, slots=True)
class NurbsSurface:
    u_degree: int
    v_degree: int
    u_control_vertex_count: int
    v_control_vertex_count: int
    vertex_dimension: int
    u_knot_type: int
    v_knot_type: int
    u_periodic: bool
    v_periodic: bool
    u_closed: bool
    v_closed: bool
    rational: bool
    surface_form: int
    control_vertices: tuple[tuple[float, ...], ...]
    u_knots: tuple[float, ...]
    v_knots: tuple[float, ...]
    u_knot_multiplicities: tuple[int, ...]
    v_knot_multiplicities: tuple[int, ...]
    sources: tuple[SourceNodeRef, ...]


SurfaceDefinition: TypeAlias = (
    PlaneSurface
    | CylinderSurface
    | ConeSurface
    | SphereSurface
    | TorusSurface
    | BlendedEdgeSurface
    | BlendBoundarySurface
    | OffsetSurface
    | NurbsSurface
    | UnsupportedGeometry
)


@dataclass(frozen=True, slots=True)
class SurfaceGeometry:
    """One surface with orientation, typed definition, and source provenance."""

    id: int
    sense: Sense
    owner: SourceNodeRef | None
    kind: SurfaceKind
    definition: SurfaceDefinition
    source: SourceNodeRef
