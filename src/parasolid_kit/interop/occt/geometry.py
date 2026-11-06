"""Exact OCCT geometry builders with no import-time OCCT dependency."""

from __future__ import annotations

from collections.abc import Callable
from itertools import pairwise
from math import atan2, isfinite, pi, sqrt

from ...brep.geometry import (
    BlendBoundarySurface,
    BlendedEdgeSurface,
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
    SpunSurface,
    SurfaceDefinition,
    SweptSurface,
    TorusSurface,
    TrimmedCurve,
)
from ...brep.topology import Sense, Vector3
from .options import OcctConversionOptions

# The part of the validation tolerance a blend pipe's B-spline approximation
# may use. The pipe follows a spine fitted within the full tolerance; a
# hundredth made production pipes of thousands of poles slower to build,
# project onto and mesh without changing any measured edge tolerance.
PIPE_TOLERANCE_FRACTION = 1.0e-1


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

    def torus(self, surface: TorusSurface, *, ring_only: bool = True) -> object:
        """Exact torus; horn and apple tori only where a source trim bounds the face."""
        from OCP.gp import gp_Torus

        major = surface.major_radius * self.scale
        minor = surface.minor_radius * self.scale
        if not all(isfinite(value) for value in (major, minor)) or min(major, minor) <= 0.0:
            raise ValueError("torus radii must be finite and positive")
        if ring_only and major <= minor:
            raise ValueError("an untrimmed closed torus face requires a ring torus")
        return gp_Torus(
            self.axis3(surface.center, surface.axis, surface.x_axis, role="torus"),
            major,
            minor,
        )

    def nurbs_curve(self, curve: NurbsCurve) -> object:
        from OCP.Geom import Geom_BSplineCurve
        from OCP.TColgp import TColgp_Array1OfPnt
        from OCP.TColStd import TColStd_Array1OfReal

        self._validate_nurbs_curve(curve)
        poles = TColgp_Array1OfPnt(1, curve.control_vertex_count)
        weights = TColStd_Array1OfReal(1, curve.control_vertex_count)
        for index, values in enumerate(curve.control_vertices, 1):
            # XT stores rational vertices as (x*w, y*w, z*w, w) (p. 37).
            weight = values[3] if curve.rational else 1.0
            poles.SetValue(index, self.point(Vector3(*(value / weight for value in values[:3]))))
            weights.SetValue(index, weight)
        knots = _real_array(curve.knots)
        multiplicities = _integer_array(curve.knot_multiplicities)
        # A periodic XT curve stores its full knot vector, imaginary knots and
        # all poles included, so it is the same curve as the non-periodic
        # B-spline over the stored range (p. 36); OCCT evaluates it as such.
        if curve.rational:
            result = Geom_BSplineCurve(poles, weights, knots, multiplicities, curve.degree, False)
        else:
            result = Geom_BSplineCurve(poles, knots, multiplicities, curve.degree, False)
        if curve.closed or curve.periodic:
            start, end = result.Value(result.FirstParameter()), result.Value(result.LastParameter())
            if start.Distance(end) > self.options.validation.linear_absolute:
                raise ValueError("NURBS curve is flagged closed but its ends do not meet")
        return result

    def parameter_curve(
        self,
        curve: NurbsCurve,
        surface: SurfaceDefinition,
        *,
        section: CurveDefinition | None = None,
    ) -> object:
        """Embed open 2D poles, rational or not, using the support's parameter units.

        ``section`` is the section curve of a swept support or the profile
        curve of a spun support, whose own parameter is the support's u.
        """
        from OCP.Geom2d import Geom2d_BSplineCurve
        from OCP.gp import gp_Pnt2d
        from OCP.TColgp import TColgp_Array1OfPnt2d

        if curve.vertex_dimension != (3 if curve.rational else 2):
            raise ValueError("parameter curves require 2D NURBS")
        self._validate_nurbs_curve(curve, parameter=True)
        if isinstance(surface, PlaneSurface):
            u_scale, v_scale = self.scale, self.scale
        elif isinstance(surface, CylinderSurface):
            u_scale, v_scale = 1.0, self.scale
        elif isinstance(surface, ConeSurface):
            # The source cone is R(u, v) = P - vA + (X cos u + Y sin u)(r + v tan a)
            # (XT reference, CONE): v is measured along the axis and the radius
            # at v is r + v tan a. OCCT measures v along the generator, where
            # the same point sits at v / cos a.
            if not (isfinite(surface.cos_half_angle) and surface.cos_half_angle > 0.0):
                raise ValueError("cone half-angle must be non-zero and strictly within pi/2")
            u_scale, v_scale = 1.0, self.scale / surface.cos_half_angle
        elif isinstance(surface, (SphereSurface, TorusSurface, NurbsSurface)):
            u_scale, v_scale = 1.0, 1.0
        elif isinstance(surface, SweptSurface):
            # u is the section's own parameter, a length only along a line;
            # v is the distance along the sweep direction.
            u_scale = self.scale if isinstance(section, LineCurve) else 1.0
            v_scale = self.scale
        elif isinstance(surface, SpunSurface):
            # The source parameterises a spun surface by profile parameter u
            # (a length along a line profile) and angle v; OCCT puts the angle
            # first, and `spun_surface` reverses the axis so the normals
            # agree, which also reverses the angle: (u, v) maps to (-v, u).
            u_scale = self.scale if isinstance(section, LineCurve) else 1.0
            v_scale = 1.0
        else:
            raise ValueError("unsupported parameter-curve support surface")
        from OCP.TColStd import TColStd_Array1OfReal

        swap = isinstance(surface, SpunSurface)
        poles = TColgp_Array1OfPnt2d(1, len(curve.control_vertices))
        weights = TColStd_Array1OfReal(1, len(curve.control_vertices))
        for index, values in enumerate(curve.control_vertices, 1):
            weight = values[2] if curve.rational else 1.0
            u, v = values[0] / weight, values[1] / weight
            if swap:
                poles.SetValue(index, gp_Pnt2d(-v * v_scale, u * u_scale))
            else:
                poles.SetValue(index, gp_Pnt2d(u * u_scale, v * v_scale))
            weights.SetValue(index, weight)
        knots = _real_array(curve.knots)
        multiplicities = _integer_array(curve.knot_multiplicities)
        if curve.rational:
            return Geom2d_BSplineCurve(poles, weights, knots, multiplicities, curve.degree, False)
        return Geom2d_BSplineCurve(poles, knots, multiplicities, curve.degree, False)

    def lemon_torus(self, surface: TorusSurface) -> object:
        """Exact XT torus parameterization for a negative major radius (p. 60)."""
        from OCP.Geom import Geom_Circle, Geom_SurfaceOfRevolution
        from OCP.gp import gp_Ax1, gp_Ax2, gp_Dir, gp_Vec

        major, minor = surface.major_radius * self.scale, surface.minor_radius * self.scale
        if not all(isfinite(v) for v in (major, minor)) or not (-minor < major < 0):
            raise ValueError("lemon torus requires -minor_radius < major_radius < 0")
        self._validate_frame(surface.axis, surface.x_axis, role="torus")
        center = self.point(surface.center)
        x, z = gp_Vec(*surface.x_axis), gp_Vec(*surface.axis)
        circle = Geom_Circle(
            gp_Ax2(center.Translated(x.Multiplied(major)), gp_Dir(x.Crossed(z)), gp_Dir(x)),
            minor,
        )
        return Geom_SurfaceOfRevolution(circle, gp_Ax1(center, gp_Dir(z)))

    def surface3d(
        self,
        definition: SurfaceDefinition,
        *,
        resolve_basis: Callable[[int], object],
        resolve_curve: Callable[[int], object] | None = None,
        resolve_basis_sense: Callable[[int], Sense] | None = None,
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
            if definition.major_radius < 0:
                return self.lemon_torus(definition)
            # Source loops trim the face, so the self-intersecting inner part of
            # a horn or apple torus is never part of the result.
            return Geom_ToroidalSurface(self.torus(definition, ring_only=False))
        if isinstance(definition, NurbsSurface):
            return self.nurbs_surface(definition)
        if isinstance(definition, SpunSurface):
            return self.spun_surface(definition, resolve_curve)
        if isinstance(definition, SweptSurface):
            return self.swept_surface(definition, resolve_curve)
        if isinstance(definition, BlendedEdgeSurface):
            return self.blend_surface(definition, resolve_curve)
        if isinstance(definition, BlendBoundarySurface):
            raise ValueError("blend boundary surfaces are construction surfaces without geometry")
        if isinstance(definition, OffsetSurface):
            if not isfinite(definition.offset):
                raise ValueError("offset surface distance must be finite")
            # The source offsets along the basis normal "taking into account
            # the surface sense" (XT reference, OFFSET_SURF): a basis of
            # negative sense is offset against its natural normal.
            sign = 1.0
            if resolve_basis_sense is not None:
                sense = resolve_basis_sense(definition.basis_surface)
                if sense is Sense.NEGATIVE:
                    sign = -1.0
                elif sense is not Sense.POSITIVE:
                    raise ValueError("offset surface basis has an unknown sense")
            # A basis with a C0 knot (a crease) is offset span by span, as the
            # source does; OCCT refuses it unless told not to check.
            return Geom_OffsetSurface(
                resolve_basis(definition.basis_surface),
                sign * definition.offset * self.scale,
                True,
            )
        raise ValueError(f"unsupported exact surface definition: {type(definition).__name__}")

    def spun_surface(
        self, surface: SpunSurface, resolve_curve: Callable[[int], object] | None
    ) -> object:
        """Revolve the mapped profile curve about the spin axis (XT p. 74).

        The source parameterises the surface by profile parameter and then
        angle; OCCT puts the angle first, which reverses the normal of the
        same geometry. The axis is therefore reversed, so the OCCT normal
        agrees with the source normal and the stored senses apply unchanged.
        When both profile parameters are stored, the profile is trimmed to
        them, so a closed profile is not revolved twice; a stored start or end
        point must then lie on the profile at its parameter. A profile that
        meets the spin axis at one end only stores that point and parameter
        alone (see ``_one_sided_spin_range``). No parameter convention is
        assumed by callers.
        """
        from OCP.Geom import Geom_SurfaceOfRevolution, Geom_TrimmedCurve
        from OCP.gp import gp_Ax1

        if resolve_curve is None:
            raise ValueError("spun surface requires a profile curve resolver")
        if surface.x_axis is not None:
            self._validate_frame(surface.axis, surface.x_axis, role="spun")
        profile = resolve_curve(surface.profile)
        first, last = surface.start_parameter, surface.end_parameter
        if (first is None) != (last is None):
            first, last = self._one_sided_spin_range(surface, profile)
        if first is not None and last is not None:
            if not (isfinite(first) and isfinite(last)) or first == last:
                raise ValueError("spun surface profile parameters must be finite and distinct")
            if last < first:
                if not profile.IsPeriodic():
                    raise ValueError("spun surface profile parameters are reversed")
                last += profile.Period()
            profile = Geom_TrimmedCurve(profile, first, last, True, True)
        tolerance = self.options.validation.linear_absolute
        for point, parameter, role in (
            (surface.start, first, "start"),
            (surface.end, last, "end"),
        ):
            if point is None or parameter is None:
                continue
            if profile.Value(parameter).Distance(self.point(point)) > tolerance:
                raise ValueError(
                    f"spun surface {role} point is not on the profile at its parameter"
                )
        axis = gp_Ax1(
            self.point(surface.base),
            self.direction(surface.axis, role="spun axis").Reversed(),
        )
        return Geom_SurfaceOfRevolution(profile, axis)

    def _one_sided_spin_range(self, surface: SpunSurface, profile: object) -> tuple[float, float]:
        """The profile range of a spun surface whose profile meets its axis at one end.

        XT records where the profile meets the spin axis (the surface's
        degenerate point) as ``start`` or ``end`` with its profile parameter
        and leaves the other null when the profile meets the axis only once:
        an iCAD SX revolved cap stores the profile's first point, on the axis,
        and no end. The surface is the revolution of the profile from that
        point to the profile's other end (a full period of a periodic
        profile), which puts the degenerate ring on the boundary of the
        surface. A record whose point is missing or off the axis, or whose
        parameter is outside the profile, still fails closed.
        """
        from OCP.gp import gp_Lin

        refused = ValueError("spun surface with a one-sided profile parameter range is unsupported")
        at_start = surface.start_parameter is not None
        parameter = surface.start_parameter if at_start else surface.end_parameter
        point = surface.start if at_start else surface.end
        if point is None or parameter is None or not isfinite(parameter):
            raise refused
        axis = gp_Lin(self.point(surface.base), self.direction(surface.axis, role="spun axis"))
        if axis.Distance(self.point(point)) > self.options.validation.linear_absolute:
            raise refused
        if profile.IsPeriodic():
            period = profile.Period()
            return (parameter, parameter + period) if at_start else (parameter - period, parameter)
        lower, upper = profile.FirstParameter(), profile.LastParameter()
        slack = 1e-12 * max(1.0, abs(upper - lower))
        if at_start:
            if not lower - slack <= parameter < upper:
                raise refused
            return (max(parameter, lower), upper)
        if not lower < parameter <= upper + slack:
            raise refused
        return (lower, min(parameter, upper))

    def swept_surface(
        self, surface: SweptSurface, resolve_curve: Callable[[int], object] | None
    ) -> object:
        """Sweep the mapped section curve along the unit direction (XT SWEPT_SURF).

        The source is ``R(u, v) = C(u) + vD``. OCCT's surface of linear
        extrusion has the same parameters and the same normal, dC/du x D, so
        the stored senses apply unchanged; v is a length.
        """
        from OCP.Geom import Geom_SurfaceOfLinearExtrusion

        if resolve_curve is None:
            raise ValueError("swept surface requires a section curve resolver")
        return Geom_SurfaceOfLinearExtrusion(
            resolve_curve(surface.section),
            self.direction(surface.direction, role="sweep direction"),
        )

    def blend_surface(
        self, surface: BlendedEdgeSurface, resolve_curve: Callable[[int], object] | None
    ) -> object:
        """Blend as the pipe of the blend radius around its spine (XT p. 62).

        The blend is the envelope of balls of radius |range| centred on the
        spine, i.e. the canal surface around the spine. OCCT approximates that
        surface as a B-spline within a tenth of the validation tolerance;
        the result is a surface only, its parameters are not the source's.

        A cliff-edge blend (type E) is the same envelope: the ball rolls on one
        support and passes through the cliff edge, which the source represents
        as its second support, a blend of range [0, 0] whose spine is the
        cliff edge curve. That zero-range blend is a construction surface only
        and is never a face's surface.
        """
        from OCP.Geom import Geom_BSplineSurface, Geom_RectangularTrimmedSurface

        if resolve_curve is None:
            raise ValueError("blend surface requires a spine curve resolver")
        first, second = surface.ranges
        if not all(isfinite(value) for value in (first, second)) or first == 0.0:
            raise ValueError("blend offsets must be finite and non-zero")
        if abs(abs(first) - abs(second)) > 1.0e-12 * abs(first):
            raise ValueError("blend offsets must have equal magnitude")
        if surface.thumb_weights != (1.0, 1.0):
            raise ValueError("blend thumb weights must be (1, 1)")
        tolerance = max(1.0e-9, self.options.validation.linear_absolute * PIPE_TOLERANCE_FRACTION)
        result = self._canal_surface(
            resolve_curve(surface.spine_curve), abs(first) * self.scale, tolerance
        )
        # An exact pipe (a cylinder around a line, a torus around a circle)
        # comes back trimmed to the spine; the face loops trim it instead, and
        # the seam fixes need the untrimmed periodic basis.
        if isinstance(result, Geom_RectangularTrimmedSurface):
            result = result.BasisSurface()
        # A B-spline pipe's circular section (and a closed spine) come back
        # closed but not periodic; the seam fixes need the periodic form.
        if isinstance(result, Geom_BSplineSurface):
            if result.IsUClosed() and not result.IsUPeriodic():
                result.SetUPeriodic()
            if result.IsVClosed() and not result.IsVPeriodic():
                result.SetVPeriodic()
        return result

    @staticmethod
    def _canal_surface(spine: object, radius: float, tolerance: float) -> object:
        """The surface swept by a circle of ``radius`` along ``spine``.

        GeomFill_Pipe moves the circle with the corrected Frenet trihedron,
        which breaks down where the spine's curvature vanishes or turns
        sharply; on the fitted spines of production blends it left the
        surface 0.6 to 400 mm off while reporting success. The discrete
        trihedron of BRepOffsetAPI_MakePipeShell integrates a
        rotation-minimising frame along the spine instead and held the
        approximation within the tolerance on every blend of that corpus. The
        circle is symmetric, so the frame's rotation about the spine does not
        change the surface.

        The sweep approximates one piece per spine span at the highest degree
        it is allowed. Cubic pieces meet the tolerance on the production
        spines with a third of the poles of degree 11 (the pieces join with
        C0 knots at either degree), which keeps the STEP model and the mesh
        of a blend-heavy part small; degree 11 remains the fallback when a
        cubic sweep misses the tolerance.
        """
        from OCP.Geom import Geom_Circle, Geom_Line, Geom_TrimmedCurve
        from OCP.GeomFill import GeomFill_Pipe
        from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt, gp_Vec

        basis = spine.BasisCurve() if isinstance(spine, Geom_TrimmedCurve) else spine
        if isinstance(basis, (Geom_Line, Geom_Circle)):
            # Exact: the cylinder around a line, the torus around a circle.
            pipe = GeomFill_Pipe(spine, radius)
            pipe.Perform(tolerance, False)
            if not pipe.IsDone():
                raise ValueError("rolling-ball blend pipe construction failed")
            return pipe.Surface()
        binormal = GeometryFactory._closed_spine_binormal(spine, tolerance)
        if binormal is not None:
            # The pipe around a closed spine is closed in both directions.
            # OCCT's seam fix adjusts the wires of such a torus-like face by
            # the period along the missing seam, with one period for both
            # directions (ShapeFix_Face::FixMissingSeam); the spine is
            # reparametrized over the circle's range so the pipe has the
            # periods of a torus.
            spine = GeometryFactory._spine_over_circle_range(spine)
        start, tangent = gp_Pnt(), gp_Vec()
        spine.D1(spine.FirstParameter(), start, tangent)
        if tangent.Magnitude() <= 1e-300:
            raise ValueError("blend spine has no tangent at its start")
        section = Geom_Circle(gp_Ax2(start, gp_Dir(tangent)), radius)
        result = None
        failure = "rolling-ball blend pipe construction failed"
        for degree in (3, 11):
            try:
                candidate = GeometryFactory._sweep_circle(
                    spine, section, binormal, tolerance, degree
                )
            except ValueError as error:
                failure = str(error)
                continue
            if result is None or GeometryFactory._pole_count(
                candidate
            ) < GeometryFactory._pole_count(result):
                result = candidate
        if result is None:
            raise ValueError(failure)
        if binormal is not None:
            # The fixed frame returns exactly, so the end row misses the start
            # row only by the approximation; ten times that is the validation
            # tolerance at the pipe's share of it.
            GeometryFactory._close_sweep(result, 10.0 * tolerance)
        return result

    @staticmethod
    def _pole_count(surface: object) -> int:
        """The number of poles of a B-spline surface (others count as none)."""
        if hasattr(surface, "NbUPoles"):
            return surface.NbUPoles() * surface.NbVPoles()
        return 0

    @staticmethod
    def _sweep_circle(
        spine: object, section: object, binormal: object | None, tolerance: float, degree: int
    ) -> object:
        """The B-spline surface of ``section`` swept along ``spine`` within ``tolerance``."""
        from OCP.BRep import BRep_Tool
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge, BRepBuilderAPI_MakeWire
        from OCP.BRepOffsetAPI import BRepOffsetAPI_MakePipeShell
        from OCP.gp import gp_Dir
        from OCP.TopAbs import TopAbs_FACE
        from OCP.TopExp import TopExp_Explorer
        from OCP.TopoDS import TopoDS

        path = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(spine).Edge()).Wire()
        sweep = BRepOffsetAPI_MakePipeShell(path)
        if binormal is None:
            sweep.SetDiscreteMode()
        else:
            sweep.SetMode(gp_Dir(binormal))
        sweep.SetTolerance(tolerance, tolerance, 1.0e-2)
        sweep.SetMaxDegree(degree)
        sweep.SetMaxSegments(3000)
        profile = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(section).Edge()).Wire()
        sweep.Add(profile, False, False)
        try:
            sweep.Build()
        except Exception as error:  # OCCT raises without a message on degenerate sweeps
            raise ValueError("rolling-ball blend pipe construction failed") from error
        if not sweep.IsDone():
            raise ValueError("rolling-ball blend pipe construction failed")
        if sweep.ErrorOnSurface() > tolerance:
            raise ValueError(
                f"rolling-ball blend pipe approximation error {sweep.ErrorOnSurface():g} "
                f"exceeds {tolerance:g}"
            )
        faces = []
        explorer = TopExp_Explorer(sweep.Shape(), TopAbs_FACE)
        while explorer.More():
            faces.append(TopoDS.Face_s(explorer.Current()))
            explorer.Next()
        if len(faces) != 1:
            raise ValueError(f"rolling-ball blend pipe produced {len(faces)} faces")
        return BRep_Tool.Surface_s(faces[0])

    @staticmethod
    def _closed_spine_binormal(spine: object, tolerance: float) -> object | None:
        """A fixed binormal for sweeping along a closed spine, or None.

        The discrete trihedron integrates a rotation-minimising frame along
        the spine; around a closed spine it returns rotated about the tangent
        (its holonomy), so the pipe's end section is its start section with
        another parameter origin and the surface is not closed in the sweep
        direction. The faces of a ring blend are split at other sections, so
        their loops cross that end, where a pcurve projected onto the
        non-closed surface jumps by the whole sweep range. A fixed binormal
        makes the frame a function of the tangent alone, which returns
        exactly; the section is a circle, so the surface does not depend on
        the frame. The direction is the normal of the spine's mean plane (a
        ring blend's spine is planar or nearly so), usable while every
        tangent keeps clear of it.
        """
        from OCP.gp import gp_Pnt, gp_Vec, gp_XYZ

        first, last = spine.FirstParameter(), spine.LastParameter()
        if not (isfinite(first) and isfinite(last)):
            return None
        if spine.Value(first).Distance(spine.Value(last)) > max(100.0 * tolerance, 1.0e-7):
            return None
        count = 128
        points = []
        tangents = []
        for index in range(count):
            point, tangent = gp_Pnt(), gp_Vec()
            spine.D1(first + (last - first) * index / count, point, tangent)
            points.append(point.XYZ())
            tangents.append(tangent.XYZ())
        # OCP's in-place operators on gp_XYZ return None; the binary forms are used.
        centre = gp_XYZ()
        for point in points:
            centre = centre + point
        centre = centre / count
        area = gp_XYZ()
        for a, b in zip(points, points[1:] + points[:1], strict=True):
            area = area + (a - centre).Crossed(b - centre)
        if area.Modulus() <= 1.0e-300:
            return None
        normal = area.Normalized()
        for tangent in tangents:
            magnitude = tangent.Modulus()
            if magnitude <= 1.0e-300 or abs(tangent.Dot(normal)) > 0.5 * magnitude:
                return None
        return normal

    @staticmethod
    def _spine_over_circle_range(spine: object) -> object:
        """A B-spline copy of ``spine`` with its knots scaled onto [0, 2π]."""
        from OCP.Geom import Geom_BSplineCurve
        from OCP.GeomConvert import GeomConvert
        from OCP.TColStd import TColStd_Array1OfReal

        if isinstance(spine, Geom_BSplineCurve):
            curve = spine.Copy()
        else:
            curve = GeomConvert.CurveToBSplineCurve_s(spine)
        knots = TColStd_Array1OfReal(1, curve.NbKnots())
        curve.Knots(knots)
        first, last = knots.Value(1), knots.Value(knots.Upper())
        if not last > first:
            raise ValueError("blend spine has no parameter range")
        for index in range(1, knots.Upper() + 1):
            knots.SetValue(index, (knots.Value(index) - first) / (last - first) * (2.0 * pi))
        curve.SetKnots(knots)
        return curve

    @staticmethod
    def _close_sweep(surface: object, tolerance: float) -> None:
        """Make the end row of a pipe around a closed spine its start row.

        The sweep returns to its start within its approximation error; the
        periodic form (which ``blend_surface`` sets where the rows are equal)
        drops the end row, so the rows are made equal where they agree within
        the tolerance. A larger gap leaves the surface open.
        """
        from OCP.Geom import Geom_BSplineSurface

        if not isinstance(surface, Geom_BSplineSurface) or surface.IsVClosed():
            return
        last = surface.NbVPoles()
        gap = max(
            surface.Pole(index, 1).Distance(surface.Pole(index, last))
            for index in range(1, surface.NbUPoles() + 1)
        )
        if gap > tolerance:
            return
        for index in range(1, surface.NbUPoles() + 1):
            surface.SetPole(index, last, surface.Pole(index, 1))
            if surface.IsURational() or surface.IsVRational():
                surface.SetWeight(index, last, surface.Weight(index, 1))

    def nurbs_surface(self, surface: NurbsSurface) -> object:
        from OCP.Geom import Geom_BSplineSurface
        from OCP.TColgp import TColgp_Array2OfPnt
        from OCP.TColStd import TColStd_Array2OfReal

        self._validate_nurbs_surface(surface)
        poles = TColgp_Array2OfPnt(
            1,
            surface.u_control_vertex_count,
            1,
            surface.v_control_vertex_count,
        )
        weights = TColStd_Array2OfReal(
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
                weight = values[3] if surface.rational else 1.0
                poles.SetValue(
                    u_index + 1,
                    v_index + 1,
                    self.point(Vector3(*(value / weight for value in values[:3]))),
                )
                weights.SetValue(u_index + 1, v_index + 1, weight)
        arguments = (
            _real_array(surface.u_knots),
            _real_array(surface.v_knots),
            _integer_array(surface.u_knot_multiplicities),
            _integer_array(surface.v_knot_multiplicities),
            surface.u_degree,
            surface.v_degree,
            False,
            False,
        )
        # Periodic and closed XT surfaces store their full knot vectors and
        # poles, so the non-periodic B-spline over the stored ranges is the
        # same surface (p. 38); OCCT evaluates it as such.
        if surface.rational:
            result = Geom_BSplineSurface(poles, weights, *arguments)
        else:
            result = Geom_BSplineSurface(poles, *arguments)
        u1, u2, v1, v2 = result.Bounds()
        tolerance = self.options.validation.linear_absolute
        for closed, samples in (
            (
                surface.u_closed or surface.u_periodic,
                [(u1, v1 + (v2 - v1) * k / 4, u2, v1 + (v2 - v1) * k / 4) for k in range(5)],
            ),
            (
                surface.v_closed or surface.v_periodic,
                [(u1 + (u2 - u1) * k / 4, v1, u1 + (u2 - u1) * k / 4, v2) for k in range(5)],
            ),
        ):
            if closed and any(
                result.Value(a, b).Distance(result.Value(c, d)) > tolerance
                for a, b, c, d in samples
            ):
                raise ValueError("NURBS surface is flagged closed but its boundaries do not meet")
        if surface.u_periodic or surface.v_periodic:
            result = self._periodic_nurbs_surface(result, surface, tolerance)
        return result

    @staticmethod
    def _periodic_nurbs_surface(result: object, surface: NurbsSurface, tolerance: float) -> object:
        """OCCT's periodic form of a source-periodic B-surface.

        A periodic XT surface stores the knots of one period with the degree's
        worth of imaginary knots on each side and repeats its first poles at
        the end (p. 38): the flat form of OCCT's periodic B-spline, whose
        ``SetUPeriodic`` keeps the poles of one period. The non-periodic form
        agrees inside the period but continues the polynomial of its end
        spans beyond it, where the source repeats the surface; a face loop
        that crosses the period there (the projected parameter curve of a
        3D curve on the seam, unwrapped by OCCT's own projection) landed
        millimetres off. Periodic, OCCT evaluates the wrap itself and the
        seam fixes apply as on a cylinder. A surface OCCT cannot make periodic,
        or whose periodic form differs from the stored one, keeps the flat
        form, which is exact inside the period.
        """
        periodic = result.Copy()
        try:
            if surface.u_periodic:
                periodic.SetUPeriodic()
            if surface.v_periodic:
                periodic.SetVPeriodic()
        except Exception:  # OCCT raises Standard_ConstructionError without text
            return result
        u1, u2, v1, v2 = result.Bounds()
        for i in range(5):
            for j in range(5):
                u, v = u1 + (u2 - u1) * i / 4, v1 + (v2 - v1) * j / 4
                if periodic.Value(u, v).Distance(result.Value(u, v)) > tolerance:
                    return result
        return periodic

    def _validate_nurbs_curve(self, curve: NurbsCurve, *, parameter: bool = False) -> None:
        expected = (3 if curve.rational else 2) if parameter else (4 if curve.rational else 3)
        if curve.vertex_dimension != expected:
            raise ValueError("NURBS curve vertex dimension does not match its rational flag")
        if curve.rational and any(values[-1] <= 0 for values in curve.control_vertices):
            raise ValueError("NURBS curve weights must be positive")
        if curve.control_vertex_count != len(curve.control_vertices):
            raise ValueError("NURBS curve control vertex count does not match its payload")
        if curve.degree < 1 or curve.degree >= curve.control_vertex_count:
            raise ValueError("NURBS curve degree is outside its valid control-point range")
        if len(curve.knots) != len(curve.knot_multiplicities) or not curve.knots:
            raise ValueError("NURBS curve knots and multiplicities must have equal non-zero length")
        self._validate_control_vertices(
            curve.control_vertices, role="NURBS curve", dimension=curve.vertex_dimension
        )
        _validate_knot_vector(curve.knots, curve.knot_multiplicities, role="NURBS curve")

    def _validate_nurbs_surface(self, surface: NurbsSurface) -> None:
        if surface.vertex_dimension != (4 if surface.rational else 3):
            raise ValueError("NURBS surface vertex dimension does not match its rational flag")
        if surface.rational and any(values[-1] <= 0 for values in surface.control_vertices):
            raise ValueError("NURBS surface weights must be positive")
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
        self._validate_control_vertices(
            surface.control_vertices, role="NURBS surface", dimension=surface.vertex_dimension
        )
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
        dimension: int = 3,
    ) -> None:
        if any(len(values) != dimension for values in vertices):
            raise ValueError(f"{role} control vertices must contain exactly {dimension} values")
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
