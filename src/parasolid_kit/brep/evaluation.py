"""Dependency-free evaluation in the stored source parameter and unit system."""

from __future__ import annotations

from bisect import bisect_right
from dataclasses import dataclass
from math import cos, cosh, hypot, isfinite, pi, sin, sinh

from .geometry import (
    CircleCurve,
    CurveDefinition,
    CurveGeometry,
    CylinderSurface,
    EllipseCurve,
    HyperbolaCurve,
    LineCurve,
    NurbsCurve,
    NurbsSurface,
    ParabolaCurve,
    PlaneSurface,
    SphereSurface,
    SurfaceDefinition,
    SurfaceGeometry,
    TorusSurface,
    TrimmedCurve,
)
from .model import BrepModel
from .topology import Vector3


class GeometryEvaluationError(ValueError):
    """The geometry, parameter, or requested operation cannot be evaluated."""


@dataclass(frozen=True, slots=True)
class CurveEvaluation:
    point: Vector3
    tangent: Vector3


@dataclass(frozen=True, slots=True)
class SurfaceEvaluation:
    point: Vector3
    du: Vector3
    dv: Vector3
    normal: Vector3


def _parameter(value: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise GeometryEvaluationError("parameters must be finite numbers")
    try:
        result = float(value)
    except OverflowError as error:
        raise GeometryEvaluationError("parameters must be finite numbers") from error
    if not isfinite(result):
        raise GeometryEvaluationError("parameters must be finite numbers")
    return result


def _vector(values) -> Vector3:
    try:
        return Vector3(*(float(v) for v in values))
    except (ValueError, OverflowError) as error:
        raise GeometryEvaluationError("evaluation produced a non-finite vector") from error


def _add(a, b):
    return tuple(x + y for x, y in zip(a, b, strict=True))


def _scale(a, factor):
    return tuple(x * factor for x in a)


def _cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _unit(a):
    length = hypot(*a)
    if not isfinite(length) or length == 0:
        raise GeometryEvaluationError("geometry has a degenerate direction or normal")
    return _scale(a, 1 / length)


def _frame(axis, reference):
    z = _unit(tuple(axis))
    x = tuple(reference)
    projection = sum(a * b for a, b in zip(x, z, strict=True))
    x = _unit(_add(x, _scale(z, -projection)))
    return x, _cross(z, x), z


def _knots(degree, count, knots, multiplicities):
    if (
        isinstance(degree, bool)
        or not isinstance(degree, int)
        or not 1 <= degree <= 64
        or isinstance(count, bool)
        or not isinstance(count, int)
        or not degree < count <= 1_048_576
        or len(knots) != len(multiplicities)
        or len(knots) < 2
    ):
        raise GeometryEvaluationError("invalid NURBS degree, control count, or knot arrays")
    total = 0
    previous = float("-inf")
    for i, (knot, mult) in enumerate(zip(knots, multiplicities, strict=True)):
        if (
            not isfinite(knot)
            or knot <= previous
            or isinstance(mult, bool)
            or not isinstance(mult, int)
            or not 1 <= mult <= degree + (i in (0, len(knots) - 1))
        ):
            raise GeometryEvaluationError("invalid NURBS knot or multiplicity")
        previous = knot
        total += mult
        if total > count + degree + 1:
            raise GeometryEvaluationError("NURBS knot total exceeds its control count")
    if total != count + degree + 1:
        raise GeometryEvaluationError("NURBS knot total does not match its control count")
    expanded = tuple(k for k, m in zip(knots, multiplicities, strict=True) for _ in range(m))
    if expanded[degree] >= expanded[count]:
        raise GeometryEvaluationError("NURBS active parameter interval is empty")
    return expanded


def _controls(vertices, count, dimension, rational):
    expected = dimension + int(rational)
    if len(vertices) != count or any(len(p) != expected for p in vertices):
        raise GeometryEvaluationError("NURBS control dimensions do not match")
    if any(not isfinite(v) for p in vertices for v in p):
        raise GeometryEvaluationError("NURBS controls must be finite")
    if rational and any(p[-1] <= 0 for p in vertices):
        raise GeometryEvaluationError("NURBS weights must be positive")
    return tuple(tuple(p) if rational else (*p, 1.0) for p in vertices)


def _deboor(controls, degree, knots, t):
    """Evaluate homogeneous controls; endpoints use the adjacent active span."""
    count = len(controls)
    if not knots[degree] <= t <= knots[count]:
        raise GeometryEvaluationError("parameter is outside the NURBS active knot interval")
    span = count - 1 if t == knots[count] else bisect_right(knots, t) - 1
    work = [list(p) for p in controls[span - degree : span + 1]]
    for level in range(1, degree + 1):
        for j in range(degree, level - 1, -1):
            i = span - degree + j
            width = knots[i + degree - level + 1] - knots[i]
            alpha = (t - knots[i]) / width if width else 0.0
            work[j] = [
                (1 - alpha) * a + alpha * b for a, b in zip(work[j - 1], work[j], strict=True)
            ]
    return tuple(work[degree])


def _derivative(controls, degree, knots, t):
    derived = []
    for i in range(len(controls) - 1):
        width = knots[i + degree + 1] - knots[i + 1]
        derived.append(
            tuple(
                (b - a) * degree / width if width else 0.0
                for a, b in zip(controls[i], controls[i + 1], strict=True)
            )
        )
    return _deboor(derived, degree - 1, knots[1:-1], t)


def _cartesian(point, derivative):
    weight = point[-1]
    if weight <= 0 or not isfinite(weight):
        raise GeometryEvaluationError("NURBS evaluation has an invalid homogeneous weight")
    position = tuple(v / weight for v in point[:-1])
    tangent = tuple(
        (v - p * derivative[-1]) / weight for p, v in zip(position, derivative[:-1], strict=True)
    )
    return position, tangent


class _NurbsCurveEvaluator:
    def __init__(self, curve):
        if curve.periodic:
            raise GeometryEvaluationError("periodic NURBS evaluation is not yet supported")
        dimension = curve.vertex_dimension - int(curve.rational)
        if dimension != 3:
            raise GeometryEvaluationError("curve evaluation requires 3D NURBS controls")
        self.degree = curve.degree
        self.knots = _knots(
            curve.degree, curve.control_vertex_count, curve.knots, curve.knot_multiplicities
        )
        self.controls = _controls(
            curve.control_vertices, curve.control_vertex_count, 3, curve.rational
        )

    def evaluate(self, parameter):
        point = _deboor(self.controls, self.degree, self.knots, parameter)
        derivative = _derivative(self.controls, self.degree, self.knots, parameter)
        position, tangent = _cartesian(point, derivative)
        return CurveEvaluation(_vector(position), _vector(tangent))


def _curve_evaluator(curve, model, seen=()):
    if len(seen) > 64:
        raise GeometryEvaluationError("trimmed curve depth exceeds 64")
    definition = curve.definition if isinstance(curve, CurveGeometry) else curve
    if isinstance(definition, NurbsCurve):
        return _NurbsCurveEvaluator(definition).evaluate
    if isinstance(definition, TrimmedCurve):
        if model is None or definition.basis_curve in seen:
            raise GeometryEvaluationError("trimmed curve needs an acyclic basis in its BrepModel")
        bases = [c for c in model.curves if c.id == definition.basis_curve]
        if len(bases) != 1:
            raise GeometryEvaluationError("trimmed basis curve is missing or ambiguous")
        evaluate = _curve_evaluator(bases[0], model, (*seen, definition.basis_curve))

        def trimmed(t):
            if (
                not min(definition.start_parameter, definition.end_parameter)
                <= t
                <= max(definition.start_parameter, definition.end_parameter)
            ):
                raise GeometryEvaluationError("parameter is outside the stored trim interval")
            return evaluate(t)

        return trimmed
    if isinstance(definition, LineCurve):
        _unit(tuple(definition.direction))
        return lambda t: CurveEvaluation(
            _vector(_add(definition.point, _scale(definition.direction, t))), definition.direction
        )
    if isinstance(definition, (CircleCurve, EllipseCurve, ParabolaCurve, HyperbolaCurve)):
        x, y, _ = _frame(definition.normal, definition.x_axis)
        center = (
            definition.center
            if isinstance(definition, (CircleCurve, EllipseCurve))
            else definition.origin
        )

        def analytic(t):
            if isinstance(definition, CircleCurve):
                a = b = definition.radius
            elif isinstance(definition, EllipseCurve):
                a, b = definition.major_radius, definition.minor_radius
            elif isinstance(definition, HyperbolaCurve):
                a, b = definition.transverse_radius, definition.conjugate_radius
            else:
                a = b = definition.focal_length
            if not all(isfinite(v) and v > 0 for v in (a, b)):
                raise GeometryEvaluationError("analytic curve radii must be finite and positive")
            if isinstance(definition, ParabolaCurve):
                coordinates, derivative = (t * t / (4 * a), t), (t / (2 * a), 1.0)
            elif isinstance(definition, HyperbolaCurve):
                coordinates, derivative = (a * cosh(t), b * sinh(t)), (a * sinh(t), b * cosh(t))
            else:
                coordinates, derivative = (a * cos(t), b * sin(t)), (-a * sin(t), b * cos(t))
            point = _add(center, _add(_scale(x, coordinates[0]), _scale(y, coordinates[1])))
            tangent = _add(_scale(x, derivative[0]), _scale(y, derivative[1]))
            return CurveEvaluation(_vector(point), _vector(tangent))

        return analytic
    raise GeometryEvaluationError(f"unsupported curve evaluation: {type(definition).__name__}")


def evaluate_curve(
    curve: CurveGeometry | CurveDefinition, parameter: float, *, model: BrepModel | None = None
) -> CurveEvaluation:
    """Evaluate P(t) and dP/dt without OCCT. Source sense does not reverse dP/dt.

    Trimmed curves use their stored basis parameter, never a guessed 0..1 range.
    NURBS controls are homogeneous when rational; periodic forms are rejected.
    """
    try:
        return _curve_evaluator(curve, model)(_parameter(parameter))
    except OverflowError as error:
        raise GeometryEvaluationError("curve evaluation overflowed") from error


def sample_curve(
    curve: CurveGeometry | CurveDefinition,
    start: float,
    end: float,
    count: int = 100,
    *,
    model: BrepModel | None = None,
) -> tuple[Vector3, ...]:
    """Sample uniformly in parameter (not arc length), including both endpoints.

    Knots and controls are prepared once per call. The output is bounded to one
    million samples and preserves descending intervals supplied by the caller.
    """
    start, end = _parameter(start), _parameter(end)
    if isinstance(count, bool) or not isinstance(count, int) or not 2 <= count <= 1_000_000:
        raise GeometryEvaluationError("sample count must be an integer between 2 and 1000000")
    evaluate = _curve_evaluator(curve, model)
    try:
        return tuple(
            evaluate(
                start
                if i == 0
                else end
                if i == count - 1
                else start * (1 - i / (count - 1)) + end * i / (count - 1)
            ).point
            for i in range(count)
        )
    except OverflowError as error:
        raise GeometryEvaluationError("curve sampling overflowed") from error


def _nurbs_surface(surface, u, v):
    if surface.u_periodic or surface.v_periodic:
        raise GeometryEvaluationError("periodic NURBS evaluation is not yet supported")
    if surface.vertex_dimension != 3 + int(surface.rational):
        raise GeometryEvaluationError("surface evaluation requires 3D NURBS controls")
    nu, nv = surface.u_control_vertex_count, surface.v_control_vertex_count
    ku = _knots(surface.u_degree, nu, surface.u_knots, surface.u_knot_multiplicities)
    kv = _knots(surface.v_degree, nv, surface.v_knots, surface.v_knot_multiplicities)
    controls = _controls(surface.control_vertices, nu * nv, 3, surface.rational)
    # Parasolid stores V as the fast-changing control index.
    rows = [controls[i * nv : (i + 1) * nv] for i in range(nu)]
    points = tuple(_deboor(row, surface.v_degree, kv, v) for row in rows)
    v_derivatives = tuple(_derivative(row, surface.v_degree, kv, v) for row in rows)
    point = _deboor(points, surface.u_degree, ku, u)
    position, du = _cartesian(point, _derivative(points, surface.u_degree, ku, u))
    _, dv = _cartesian(point, _deboor(v_derivatives, surface.u_degree, ku, u))
    return SurfaceEvaluation(
        _vector(position), _vector(du), _vector(dv), _vector(_unit(_cross(du, dv)))
    )


def evaluate_surface(
    surface: SurfaceGeometry | SurfaceDefinition, u: float, v: float
) -> SurfaceEvaluation:
    """Evaluate a plane, cylinder, sphere, torus, or nonperiodic NURBS surface.

    Parameters and coordinates retain source units. Normal follows du cross dv;
    sphere poles use the analytic limiting normal. Face/surface senses and trim
    containment are not applied. Singular NURBS normals raise an error.
    """
    u, v = _parameter(u), _parameter(v)
    definition = surface.definition if isinstance(surface, SurfaceGeometry) else surface
    if isinstance(definition, NurbsSurface):
        return _nurbs_surface(definition, u, v)
    if isinstance(definition, PlaneSurface):
        x, y, z = _frame(definition.normal, definition.x_axis)
        point = _add(definition.point, _add(_scale(x, u), _scale(y, v)))
        return SurfaceEvaluation(_vector(point), _vector(x), _vector(y), _vector(z))
    if not isinstance(definition, (CylinderSurface, SphereSurface, TorusSurface)):
        raise GeometryEvaluationError(
            f"unsupported surface evaluation: {type(definition).__name__}"
        )
    x, y, z = _frame(definition.axis, definition.x_axis)
    radial = _add(_scale(x, cos(u)), _scale(y, sin(u)))
    angular = _add(_scale(x, -sin(u)), _scale(y, cos(u)))
    if isinstance(definition, CylinderSurface):
        radius = definition.radius
        point = _add(definition.point, _add(_scale(radial, radius), _scale(z, v)))
        du, dv, normal = _scale(angular, radius), z, radial
    elif isinstance(definition, SphereSurface):
        radius = definition.radius
        if not -pi / 2 <= v <= pi / 2:
            raise GeometryEvaluationError("sphere latitude must be between -pi/2 and pi/2")
        normal = _add(_scale(radial, cos(v)), _scale(z, sin(v)))
        point = _add(definition.center, _scale(normal, radius))
        du = _scale(angular, radius * cos(v))
        dv = _scale(_add(_scale(radial, -sin(v)), _scale(z, cos(v))), radius)
    else:
        radius = definition.minor_radius
        if not isfinite(definition.major_radius) or definition.major_radius <= radius:
            raise GeometryEvaluationError("torus evaluation currently requires a ring torus")
        normal = _add(_scale(radial, cos(v)), _scale(z, sin(v)))
        point = _add(
            definition.center, _add(_scale(radial, definition.major_radius), _scale(normal, radius))
        )
        du = _scale(angular, definition.major_radius + radius * cos(v))
        dv = _scale(_add(_scale(radial, -sin(v)), _scale(z, cos(v))), radius)
    if not isfinite(radius) or radius <= 0:
        raise GeometryEvaluationError("surface radius must be finite and positive")
    return SurfaceEvaluation(_vector(point), _vector(du), _vector(dv), _vector(normal))
