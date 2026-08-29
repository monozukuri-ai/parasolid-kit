"""Exact OCCT geometry builders with no import-time OCCT dependency."""

from __future__ import annotations

from collections.abc import Callable
from itertools import pairwise
from math import atan2, isfinite, pi, sqrt

from ...brep.geometry import (
    CircleCurve,
    ConeSurface,
    CurveDefinition,
    CylinderSurface,
    EllipseCurve,
    HyperbolaCurve,
    LineCurve,
    NurbsCurve,
    NurbsSurface,
    OffsetSurface,
    ParabolaCurve,
    PlaneSurface,
    SphereSurface,
    SurfaceDefinition,
    TorusSurface,
    TrimmedCurve,
)
from ...brep.topology import Vector3
from .options import OcctConversionOptions


class GeometryFactory:
    """Scale source lengths and construct the supported exact OCCT geometry."""

    def __init__(self, options: OcctConversionOptions) -> None:
        self.options = options

    @property
    def scale(self) -> float:
        return self.options.applied_scale

    def coordinates(self, value: Vector3) -> tuple[float, float, float]:
        return tuple(component * self.scale for component in value)

    def point(self, value: Vector3) -> object:
        from OCP.gp import gp_Pnt

        return gp_Pnt(*self.coordinates(value))

    def direction(self, value: Vector3, *, role: str) -> object:
        from OCP.gp import gp_Dir

        components = value.to_tuple()
        magnitude = _magnitude(components)
        if magnitude <= 1.0e-15:
            raise ValueError(f"{role} must be non-zero")
        return gp_Dir(*components)

    def axis3(
        self,
        point: Vector3,
        normal: Vector3,
        x_axis: Vector3,
        *,
        role: str,
    ) -> object:
        from OCP.gp import gp_Ax3

        self._validate_frame(normal, x_axis, role=role)
        return gp_Ax3(
            self.point(point),
            self.direction(normal, role=f"{role} normal"),
            self.direction(x_axis, role=f"{role} x_axis"),
        )

    def axis2(
        self,
        point: Vector3,
        normal: Vector3,
        x_axis: Vector3,
        *,
        role: str,
    ) -> object:
        from OCP.gp import gp_Ax2

        self._validate_frame(normal, x_axis, role=role)
        return gp_Ax2(
            self.point(point),
            self.direction(normal, role=f"{role} normal"),
            self.direction(x_axis, role=f"{role} x_axis"),
        )

    def make_vertex(self, position: Vector3) -> object:
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeVertex

        builder = BRepBuilderAPI_MakeVertex(self.point(position))
        if not builder.IsDone():
            raise ValueError("OCCT point vertex construction did not complete")
        return builder.Vertex()

    def make_line_edge(
        self,
        curve: LineCurve,
        start_vertex: object,
        end_vertex: object,
        start_position: Vector3,
        end_position: Vector3,
    ) -> object:
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge

        start = self.coordinates(start_position)
        end = self.coordinates(end_position)
        origin = self.coordinates(curve.point)
        direction = curve.direction.to_tuple()
        direction_magnitude = _magnitude(direction)
        if direction_magnitude <= 1.0e-15:
            raise ValueError("line direction must be non-zero")
        segment_length = _magnitude(
            tuple(right - left for left, right in zip(start, end, strict=True))
        )
        tolerance = self.options.validation.linear_threshold(segment_length)
        if segment_length <= tolerance:
            raise ValueError("line edge endpoints must be distinct")
        for name, point in (("start", start), ("end", end)):
            offset = tuple(value - base for value, base in zip(point, origin, strict=True))
            distance = _magnitude(_cross(offset, direction)) / direction_magnitude
            if distance > tolerance:
                raise ValueError(
                    f"line edge {name} vertex is {distance:g} target units off its curve"
                )
        builder = BRepBuilderAPI_MakeEdge(start_vertex, end_vertex)
        if not builder.IsDone():
            raise ValueError("OCCT line edge construction did not complete")
        return builder.Edge()

    def make_curve_edge(
        self,
        definition: CurveDefinition,
        start_vertex: object | None,
        end_vertex: object | None,
        start_position: Vector3 | None,
        end_position: Vector3 | None,
        *,
        resolve_basis: Callable[[int], object],
    ) -> object:
        """Build one exact finite or periodic edge from a mapped curve definition."""

        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge

        curve = self.curve3d(definition, resolve_basis=resolve_basis)
        if isinstance(definition, TrimmedCurve):
            if (
                start_vertex is None
                or end_vertex is None
                or start_position is None
                or end_position is None
            ):
                raise ValueError("trimmed curve edge must have two endpoint vertices")
            self._validate_trimmed_endpoints(definition, start_position, end_position)
            self._validate_parameter_points(
                curve,
                (float(curve.FirstParameter()), float(curve.LastParameter())),
                start_position,
                end_position,
                role="trimmed curve",
            )
            builder = BRepBuilderAPI_MakeEdge(
                curve,
                start_vertex,
                end_vertex,
                curve.FirstParameter(),
                curve.LastParameter(),
            )
        elif start_vertex is not None or end_vertex is not None:
            if (
                start_vertex is None
                or end_vertex is None
                or start_position is None
                or end_position is None
            ):
                raise ValueError("bounded curve edge must have two endpoint vertices")
            self._validate_distinct_endpoints(start_position, end_position, role="curve edge")
            parameters = self._bounded_curve_parameters(
                definition,
                curve,
                start_position,
                end_position,
            )
            if parameters is None:
                builder = BRepBuilderAPI_MakeEdge(curve, start_vertex, end_vertex)
            else:
                self._validate_parameter_points(
                    curve,
                    parameters,
                    start_position,
                    end_position,
                    role="curve edge",
                )
                builder = BRepBuilderAPI_MakeEdge(
                    curve,
                    start_vertex,
                    end_vertex,
                    *parameters,
                )
        else:
            if not _is_closed_curve(definition):
                raise ValueError("unbounded curve edge must be explicitly trimmed by vertices")
            builder = BRepBuilderAPI_MakeEdge(curve)
        if not builder.IsDone():
            raise ValueError("OCCT exact curve edge construction did not complete")
        return builder.Edge()

    def curve3d(
        self,
        definition: CurveDefinition,
        *,
        resolve_basis: Callable[[int], object],
    ) -> object:
        """Return an exact ``Geom_Curve`` for one supported source definition."""

        from OCP.Geom import (
            Geom_Circle,
            Geom_Ellipse,
            Geom_Hyperbola,
            Geom_Line,
            Geom_Parabola,
            Geom_TrimmedCurve,
        )

        if isinstance(definition, LineCurve):
            return Geom_Line(self.line(definition))
        if isinstance(definition, CircleCurve):
            return Geom_Circle(self.circle(definition))
        if isinstance(definition, EllipseCurve):
            return Geom_Ellipse(self.ellipse(definition))
        if isinstance(definition, ParabolaCurve):
            return Geom_Parabola(self.parabola(definition))
        if isinstance(definition, HyperbolaCurve):
            return Geom_Hyperbola(self.hyperbola(definition))
        if isinstance(definition, TrimmedCurve):
            if not all(
                isfinite(value) for value in (definition.start_parameter, definition.end_parameter)
            ):
                raise ValueError("trimmed curve parameters must be finite")
            if definition.start_parameter == definition.end_parameter:
                raise ValueError("trimmed curve parameters must be distinct")
            return Geom_TrimmedCurve(
                resolve_basis(definition.basis_curve),
                definition.start_parameter,
                definition.end_parameter,
                True,
                True,
            )
        if isinstance(definition, NurbsCurve):
            return self.nurbs_curve(definition)
        raise ValueError(f"unsupported exact curve definition: {type(definition).__name__}")

    def line(self, curve: LineCurve) -> object:
        from OCP.gp import gp_Lin

        return gp_Lin(
            self.point(curve.point),
            self.direction(curve.direction, role="line direction"),
        )

    def circle(self, curve: CircleCurve) -> object:
        from OCP.gp import gp_Circ

        radius = curve.radius * self.scale
        if not isfinite(radius) or radius <= 0.0:
            raise ValueError("circle radius must be finite and positive")
        return gp_Circ(
            self.axis2(curve.center, curve.normal, curve.x_axis, role="circle"),
            radius,
        )

    def ellipse(self, curve: EllipseCurve) -> object:
        from OCP.gp import gp_Elips

        major = curve.major_radius * self.scale
        minor = curve.minor_radius * self.scale
        if not all(isfinite(value) for value in (major, minor)) or minor <= 0.0:
            raise ValueError("ellipse radii must be finite and positive")
        if major < minor:
            raise ValueError("ellipse major radius must not be smaller than its minor radius")
        return gp_Elips(
            self.axis2(curve.center, curve.normal, curve.x_axis, role="ellipse"),
            major,
            minor,
        )

    def parabola(self, curve: ParabolaCurve) -> object:
        from OCP.gp import gp_Parab

        focal = curve.focal_length * self.scale
        if not isfinite(focal) or focal <= 0.0:
            raise ValueError("parabola focal length must be finite and positive")
        return gp_Parab(
            self.axis2(curve.origin, curve.normal, curve.x_axis, role="parabola"),
            focal,
        )

    def hyperbola(self, curve: HyperbolaCurve) -> object:
        from OCP.gp import gp_Hypr

        major = curve.transverse_radius * self.scale
        minor = curve.conjugate_radius * self.scale
        if not all(isfinite(value) for value in (major, minor)) or min(major, minor) <= 0.0:
            raise ValueError("hyperbola radii must be finite and positive")
        return gp_Hypr(
            self.axis2(curve.origin, curve.normal, curve.x_axis, role="hyperbola"),
            major,
            minor,
        )

    def make_closed_circle_edge(self, curve: CircleCurve) -> object:
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge

        builder = BRepBuilderAPI_MakeEdge(self.circle(curve))
        if not builder.IsDone():
            raise ValueError("OCCT periodic circle edge construction did not complete")
        return builder.Edge()

    def plane(self, surface: PlaneSurface) -> object:
        from OCP.gp import gp_Pln

        return gp_Pln(
            self.axis3(
                surface.point,
                surface.normal,
                surface.x_axis,
                role="plane",
            )
        )

    def cylinder(self, surface: CylinderSurface) -> object:
        from OCP.gp import gp_Cylinder

        radius = surface.radius * self.scale
        if not isfinite(radius) or radius <= 0.0:
            raise ValueError("cylinder radius must be finite and positive")
        return gp_Cylinder(
            self.axis3(
                surface.point,
                surface.axis,
                surface.x_axis,
                role="cylinder",
            ),
            radius,
        )

    def cone(self, surface: ConeSurface) -> object:
        from OCP.gp import gp_Cone

        radius = surface.radius * self.scale
        angle = atan2(surface.sin_half_angle, surface.cos_half_angle)
        if not isfinite(radius) or radius < 0.0:
            raise ValueError("cone reference radius must be finite and non-negative")
        if not all(isfinite(value) for value in (surface.sin_half_angle, surface.cos_half_angle)):
            raise ValueError("cone half-angle components must be finite")
        magnitude = sqrt(surface.sin_half_angle**2 + surface.cos_half_angle**2)
        if abs(magnitude - 1.0) > 1.0e-9:
            raise ValueError("cone half-angle sine and cosine must form a unit pair")
        if abs(angle) <= 1.0e-15 or abs(angle) >= pi / 2.0:
            raise ValueError("cone half-angle must be non-zero and strictly within pi/2")
        return gp_Cone(
            self.axis3(surface.point, surface.axis, surface.x_axis, role="cone"),
            angle,
            radius,
        )

    def sphere(self, surface: SphereSurface) -> object:
        from OCP.gp import gp_Sphere

        radius = surface.radius * self.scale
        if not isfinite(radius) or radius <= 0.0:
            raise ValueError("sphere radius must be finite and positive")
        return gp_Sphere(
            self.axis3(surface.center, surface.axis, surface.x_axis, role="sphere"),
            radius,
        )

    def torus(self, surface: TorusSurface) -> object:
        from OCP.gp import gp_Torus

        major = surface.major_radius * self.scale
        minor = surface.minor_radius * self.scale
        if not all(isfinite(value) for value in (major, minor)) or min(major, minor) <= 0.0:
            raise ValueError("torus radii must be finite and positive")
        if major <= minor:
            raise ValueError("I7 supports ring tori with major radius greater than minor radius")
        return gp_Torus(
            self.axis3(surface.center, surface.axis, surface.x_axis, role="torus"),
            major,
            minor,
        )

    def nurbs_curve(self, curve: NurbsCurve) -> object:
        from OCP.Geom import Geom_BSplineCurve
        from OCP.TColgp import TColgp_Array1OfPnt

        self._validate_nurbs_curve(curve)
        poles = TColgp_Array1OfPnt(1, curve.control_vertex_count)
        for index, values in enumerate(curve.control_vertices, 1):
            poles.SetValue(index, self.point(Vector3(*values)))
        knots = _real_array(curve.knots)
        multiplicities = _integer_array(curve.knot_multiplicities)
        return Geom_BSplineCurve(
            poles,
            knots,
            multiplicities,
            curve.degree,
            curve.periodic,
        )

    def surface3d(
        self,
        definition: SurfaceDefinition,
        *,
        resolve_basis: Callable[[int], object],
    ) -> object:
        """Return an exact ``Geom_Surface`` for one supported source definition."""

        from OCP.Geom import (
            Geom_ConicalSurface,
            Geom_CylindricalSurface,
            Geom_OffsetSurface,
            Geom_Plane,
            Geom_SphericalSurface,
            Geom_ToroidalSurface,
        )

        if isinstance(definition, PlaneSurface):
            return Geom_Plane(self.plane(definition))
        if isinstance(definition, CylinderSurface):
            return Geom_CylindricalSurface(self.cylinder(definition))
        if isinstance(definition, ConeSurface):
            return Geom_ConicalSurface(self.cone(definition))
        if isinstance(definition, SphereSurface):
            return Geom_SphericalSurface(self.sphere(definition))
        if isinstance(definition, TorusSurface):
            return Geom_ToroidalSurface(self.torus(definition))
        if isinstance(definition, NurbsSurface):
            return self.nurbs_surface(definition)
        if isinstance(definition, OffsetSurface):
            if not isfinite(definition.offset):
                raise ValueError("offset surface distance must be finite")
            return Geom_OffsetSurface(
                resolve_basis(definition.basis_surface),
                definition.offset * self.scale,
            )
        raise ValueError(f"unsupported exact surface definition: {type(definition).__name__}")

    def nurbs_surface(self, surface: NurbsSurface) -> object:
        from OCP.Geom import Geom_BSplineSurface
        from OCP.TColgp import TColgp_Array2OfPnt

        self._validate_nurbs_surface(surface)
        poles = TColgp_Array2OfPnt(
            1,
            surface.u_control_vertex_count,
            1,
            surface.v_control_vertex_count,
        )
        for u_index in range(surface.u_control_vertex_count):
            for v_index in range(surface.v_control_vertex_count):
                values = surface.control_vertices[
                    u_index * surface.v_control_vertex_count + v_index
                ]
                poles.SetValue(u_index + 1, v_index + 1, self.point(Vector3(*values)))
        return Geom_BSplineSurface(
            poles,
            _real_array(surface.u_knots),
            _real_array(surface.v_knots),
            _integer_array(surface.u_knot_multiplicities),
            _integer_array(surface.v_knot_multiplicities),
            surface.u_degree,
            surface.v_degree,
            surface.u_periodic,
            surface.v_periodic,
        )

    def _validate_nurbs_curve(self, curve: NurbsCurve) -> None:
        if curve.rational or curve.vertex_dimension != 3:
            raise ValueError("I7 supports only non-rational three-dimensional NURBS curves")
        if curve.periodic or curve.closed:
            raise ValueError("I7 supports only open non-periodic NURBS curves")
        if curve.control_vertex_count != len(curve.control_vertices):
            raise ValueError("NURBS curve control vertex count does not match its payload")
        if curve.degree < 1 or curve.degree >= curve.control_vertex_count:
            raise ValueError("NURBS curve degree is outside its valid control-point range")
        if len(curve.knots) != len(curve.knot_multiplicities) or not curve.knots:
            raise ValueError("NURBS curve knots and multiplicities must have equal non-zero length")
        self._validate_control_vertices(curve.control_vertices, role="NURBS curve")
        _validate_knot_vector(curve.knots, curve.knot_multiplicities, role="NURBS curve")

    def _validate_nurbs_surface(self, surface: NurbsSurface) -> None:
        if surface.rational or surface.vertex_dimension != 3:
            raise ValueError("I7 supports only non-rational three-dimensional NURBS surfaces")
        if surface.u_periodic or surface.v_periodic or surface.u_closed or surface.v_closed:
            raise ValueError("I7 supports only open non-periodic NURBS surfaces")
        expected = surface.u_control_vertex_count * surface.v_control_vertex_count
        if expected != len(surface.control_vertices):
            raise ValueError("NURBS surface control grid dimensions do not match its payload")
        if (
            surface.u_degree < 1
            or surface.u_degree >= surface.u_control_vertex_count
            or surface.v_degree < 1
            or surface.v_degree >= surface.v_control_vertex_count
        ):
            raise ValueError("NURBS surface degree is outside its valid control-point range")
        if (
            len(surface.u_knots) != len(surface.u_knot_multiplicities)
            or len(surface.v_knots) != len(surface.v_knot_multiplicities)
            or not surface.u_knots
            or not surface.v_knots
        ):
            raise ValueError("NURBS surface knot and multiplicity arrays must be non-empty pairs")
        self._validate_control_vertices(surface.control_vertices, role="NURBS surface")
        _validate_knot_vector(
            surface.u_knots,
            surface.u_knot_multiplicities,
            role="NURBS surface U",
        )
        _validate_knot_vector(
            surface.v_knots,
            surface.v_knot_multiplicities,
            role="NURBS surface V",
        )

    def _validate_control_vertices(
        self,
        vertices: tuple[tuple[float, ...], ...],
        *,
        role: str,
    ) -> None:
        if any(len(values) != 3 for values in vertices):
            raise ValueError(f"{role} control vertices must contain exactly three coordinates")
        if any(not isfinite(value) for values in vertices for value in values):
            raise ValueError(f"{role} control vertices must be finite")

    def _validate_distinct_endpoints(
        self,
        start: Vector3,
        end: Vector3,
        *,
        role: str,
    ) -> None:
        distance = _magnitude(
            tuple((right - left) * self.scale for left, right in zip(start, end, strict=True))
        )
        if distance <= self.options.validation.linear_threshold(distance):
            raise ValueError(f"{role} endpoints must be distinct")

    def _validate_trimmed_endpoints(
        self,
        curve: TrimmedCurve,
        start: Vector3,
        end: Vector3,
    ) -> None:
        tolerance = self.options.validation.linear_threshold(
            _magnitude(tuple(value * self.scale for value in start))
        )
        for name, actual, expected in (
            ("start", start, curve.start_point),
            ("end", end, curve.end_point),
        ):
            distance = _magnitude(
                tuple(
                    (right - left) * self.scale
                    for left, right in zip(actual, expected, strict=True)
                )
            )
            if distance > tolerance:
                raise ValueError(f"trimmed curve {name} point differs from its topological vertex")
        self._validate_distinct_endpoints(start, end, role="trimmed curve")

    def _bounded_curve_parameters(
        self,
        definition: CurveDefinition,
        curve: object,
        start: Vector3,
        end: Vector3,
    ) -> tuple[float, float] | None:
        if isinstance(definition, (ParabolaCurve, HyperbolaCurve)):
            from OCP.ElCLib import ElCLib

            primitive = (
                self.parabola(definition)
                if isinstance(definition, ParabolaCurve)
                else self.hyperbola(definition)
            )
            return (
                float(ElCLib.Parameter_s(primitive, self.point(start))),
                float(ElCLib.Parameter_s(primitive, self.point(end))),
            )
        if isinstance(definition, NurbsCurve):
            first = float(curve.FirstParameter())
            last = float(curve.LastParameter())
            direct = _point_distance(curve.Value(first), self.point(start)) + _point_distance(
                curve.Value(last), self.point(end)
            )
            reverse = _point_distance(curve.Value(first), self.point(end)) + _point_distance(
                curve.Value(last), self.point(start)
            )
            tolerance = 2.0 * self.options.validation.linear_threshold(
                _magnitude(tuple(value * self.scale for value in start))
            )
            if min(direct, reverse) > tolerance:
                raise ValueError("NURBS edge vertices do not match its natural parameter bounds")
            return (first, last) if direct <= reverse else (last, first)
        return None

    def _validate_parameter_points(
        self,
        curve: object,
        parameters: tuple[float, float],
        start: Vector3,
        end: Vector3,
        *,
        role: str,
    ) -> None:
        scale = max(
            _magnitude(tuple(value * self.scale for value in start)),
            _magnitude(tuple(value * self.scale for value in end)),
        )
        tolerance = self.options.validation.linear_threshold(scale)
        for name, parameter, expected in (
            ("start", parameters[0], start),
            ("end", parameters[1], end),
        ):
            distance = _point_distance(curve.Value(parameter), self.point(expected))
            if distance > tolerance:
                raise ValueError(
                    f"{role} {name} parameter differs from its source point by "
                    f"{distance:g} target units"
                )

    def _validate_frame(
        self,
        normal: Vector3,
        x_axis: Vector3,
        *,
        role: str,
    ) -> None:
        normal_values = normal.to_tuple()
        x_values = x_axis.to_tuple()
        denominator = _magnitude(normal_values) * _magnitude(x_values)
        if denominator <= 1.0e-15:
            raise ValueError(f"{role} axis directions must be non-zero")
        cosine = abs(_dot(normal_values, x_values) / denominator)
        if cosine > 1.0e-9:
            raise ValueError(f"{role} normal and x_axis must be perpendicular")


def _dot(left: tuple[float, ...], right: tuple[float, ...]) -> float:
    return sum(a * b for a, b in zip(left, right, strict=True))


def _cross(
    left: tuple[float, float, float],
    right: tuple[float, float, float],
) -> tuple[float, float, float]:
    return (
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    )


def _magnitude(values: tuple[float, ...]) -> float:
    return sqrt(sum(value * value for value in values))


def _point_distance(left: object, right: object) -> float:
    return sqrt(
        (left.X() - right.X()) ** 2 + (left.Y() - right.Y()) ** 2 + (left.Z() - right.Z()) ** 2
    )


def _is_closed_curve(definition: CurveDefinition) -> bool:
    return isinstance(definition, (CircleCurve, EllipseCurve)) or (
        isinstance(definition, NurbsCurve) and (definition.closed or definition.periodic)
    )


def _real_array(values: tuple[float, ...]) -> object:
    from OCP.TColStd import TColStd_Array1OfReal

    result = TColStd_Array1OfReal(1, len(values))
    for index, value in enumerate(values, 1):
        result.SetValue(index, value)
    return result


def _integer_array(values: tuple[int, ...]) -> object:
    from OCP.TColStd import TColStd_Array1OfInteger

    result = TColStd_Array1OfInteger(1, len(values))
    for index, value in enumerate(values, 1):
        result.SetValue(index, value)
    return result


def _validate_knot_vector(
    knots: tuple[float, ...],
    multiplicities: tuple[int, ...],
    *,
    role: str,
) -> None:
    if any(not isfinite(value) for value in knots):
        raise ValueError(f"{role} knots must be finite")
    if any(left >= right for left, right in pairwise(knots)):
        raise ValueError(f"{role} distinct knots must be strictly increasing")
    if any(value <= 0 for value in multiplicities):
        raise ValueError(f"{role} knot multiplicities must be positive")


__all__ = ["GeometryFactory"]
