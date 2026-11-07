"""Typed analytic, NURBS, trim, and intersection geometry."""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum
from typing import Any, TypeAlias

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
    POLYLINE = "polyline"
    SURFACE_PARAMETRIC = "surface_parametric"
    INTERSECTION = "intersection"
    UNSUPPORTED = "unsupported"


class SurfaceKind(str, Enum):
    """Supported surface definition categories."""

    PLANE = "plane"
    CYLINDER = "cylinder"
    CONE = "cone"
    XT_CONE = "xt_cone"
    MESH = "mesh"
    SPHERE = "sphere"
    TORUS = "torus"
    SPUN = "spun"
    SWEPT = "swept"
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
class PolylineCurve:
    """Polyline curve retained from the native Parasolid record."""

    points: tuple[Vector3, ...]
    closed: bool
    sense: Sense
    base_parameter: float
    source: SourceNodeRef


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
    chart_points: tuple[Vector3, ...] = ()
    start_points: tuple[Vector3, ...] = ()
    end_points: tuple[Vector3, ...] = ()


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
    | PolylineCurve
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
class XtConeSurface:
    """Cone using the XT axis and parameter convention."""

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
class SpunSurface:
    """Surface of revolution of a profile curve about an axis (XT SPUN_SURF)."""

    profile: int
    base: Vector3
    axis: Vector3
    start: Vector3 | None
    end: Vector3 | None
    start_parameter: float | None
    end_parameter: float | None
    x_axis: Vector3 | None


@dataclass(frozen=True, slots=True)
class SweptSurface:
    """Section curve swept along a unit direction, ``R(u, v) = C(u) + vD`` (XT SWEPT_SURF)."""

    section: int
    direction: Vector3


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


@dataclass(frozen=True, slots=True)
class RawVectorPool:
    """Raw transmitted vector pool; values include unused capacity."""

    encoding: int
    vector_count: int
    capacity: int
    shift: int
    values: tuple[float, ...]


@dataclass(frozen=True, slots=True)
class RawIntegerPool:
    """Raw transmitted integer pool; indices are not origin-adjusted."""

    integer_count: int
    capacity: int
    shift: int
    values: tuple[int, ...]


@dataclass(frozen=True, slots=True)
class MeshTransformPrecision:
    """Optional higher-precision corrections retained on a mesh transform."""

    index: int
    rotation_correction: tuple[float, ...]
    translation_correction: tuple[float, ...]


@dataclass(frozen=True, slots=True)
class MeshTransformNode:
    """Uncomposed transform values linked by a source MESH record."""

    index: int
    rotation: tuple[float, ...]
    translation: Vector3
    scale: float
    flags: int
    perspective: tuple[float | None, float | None, float | None]
    precision: MeshTransformPrecision | None
    byte_range: tuple[int, int]


@dataclass(frozen=True, slots=True)
class RawMeshData:
    """Embedded raw PSM mesh pools without choosing an index origin."""

    mesh_index: int
    transform_index: int | None
    surface_sense: Sense
    psm_mesh_index: int
    precision: int
    normal_type: int
    positions: RawVectorPool
    normals: RawVectorPool | None
    position_indices: RawIntegerPool
    normal_indices: RawIntegerPool | None


@dataclass(frozen=True, slots=True)
class MeshSurface:
    """Faceted MESH definition retained in its raw source representation."""

    source_mesh_index: int
    embedded: RawMeshData | None
    transform_index: int | None
    transform: MeshTransformNode | None
    unembedded_source_fields: tuple[Mapping[str, Any], ...] | None


SurfaceDefinition: TypeAlias = (
    PlaneSurface
    | CylinderSurface
    | ConeSurface
    | XtConeSurface
    | MeshSurface
    | SphereSurface
    | TorusSurface
    | SpunSurface
    | SweptSurface
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
