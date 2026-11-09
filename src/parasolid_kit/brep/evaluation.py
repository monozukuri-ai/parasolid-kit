"""Dependency-free evaluation in the stored source parameter and unit system."""

from __future__ import annotations

from bisect import bisect_right
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from functools import wraps
from math import cos, cosh, hypot, isfinite, pi, sin, sinh
from typing import TypeVar

from ..diagnostics import DiagnosticKind
from ._errors import _OperationError
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
    SurfaceParametricCurve,
    TorusSurface,
    TrimmedCurve,
)
from .model import BrepModel
from .topology import Vector3


class GeometryEvaluationError(_OperationError):
    """The geometry, parameter, or requested operation cannot be evaluated."""

    default_code = "geometry.invalid_geometry"


@dataclass(frozen=True, slots=True)
class CurveEvaluation:
    point: Vector3
    tangent: Vector3


@dataclass(frozen=True, slots=True)
class CurveEvaluation2D:
    """Stored 2D coordinates and their derivative with respect to parameter."""

    point: tuple[float, float]
    tangent: tuple[float, float]


@dataclass(frozen=True, slots=True)
class SurfaceCurveEvaluation:
    """A parameter-space curve lifted onto its support surface."""

    uv: tuple[float, float]
    uv_tangent: tuple[float, float]
    point: Vector3
    tangent: Vector3


@dataclass(frozen=True, slots=True)
class SurfaceEvaluation:
    point: Vector3
    du: Vector3
    dv: Vector3
    normal: Vector3


def _parameter(value: float, field="parameter") -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise GeometryEvaluationError(
            "parameters must be finite numbers",
            code="geometry.invalid_parameter",
            field=field,
            details={"parameter": value},
        )
    try:
        result = float(value)
    except OverflowError as error:
        raise GeometryEvaluationError(
            "parameters must be finite numbers",
            code="geometry.invalid_parameter",
            field=field,
            details={"parameter": value},
        ) from error
    if not isfinite(result):
        raise GeometryEvaluationError(
            "parameters must be finite numbers",
            code="geometry.invalid_parameter",
            field=field,
            details={"parameter": value},
        )
    return result


def _vector(values, field=None) -> Vector3:
    try:
        return Vector3(*(float(v) for v in values))
    except (ValueError, OverflowError) as error:
        raise GeometryEvaluationError(
            "evaluation produced a non-finite vector", code="geometry.nonfinite_result", field=field
        ) from error


def _add(a, b):
    return tuple(x + y for x, y in zip(a, b, strict=True))


def _scale(a, factor):
    return tuple(x * factor for x in a)


def _cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _unit(a, field=None):
    length = hypot(*a)
    if not isfinite(length) or length == 0:
        raise GeometryEvaluationError(
            "geometry has a degenerate direction or normal",
            code="geometry.degenerate_geometry",
            field=field,
        )
    return _scale(a, 1 / length)


def _frame(axis, reference, axis_field="normal"):
    z = _unit(tuple(axis), axis_field)
    x = tuple(reference)
    projection = sum(a * b for a, b in zip(x, z, strict=True))
    x = _unit(_add(x, _scale(z, -projection)), "x_axis")
    return x, _cross(z, x), z


def _knots(degree, count, knots, multiplicities, prefix=""):
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
        raise GeometryEvaluationError(
            "invalid NURBS degree, control count, or knot arrays",
            code="geometry.invalid_nurbs",
            field=prefix + "knots",
            details={"degree": degree, "control_count": count},
        )
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
            raise GeometryEvaluationError(
                "invalid NURBS knot or multiplicity",
                code="geometry.invalid_nurbs",
                field=prefix + "knots",
                details={"knot_index": i},
            )
        previous = knot
        total += mult
        if total > count + degree + 1:
            raise GeometryEvaluationError(
                "NURBS knot total exceeds its control count",
                code="geometry.invalid_nurbs",
                field=prefix + "knot_multiplicities",
            )
    if total != count + degree + 1:
        raise GeometryEvaluationError(
            "NURBS knot total does not match its control count",
            code="geometry.invalid_nurbs",
            field=prefix + "knot_multiplicities",
        )
    expanded = tuple(k for k, m in zip(knots, multiplicities, strict=True) for _ in range(m))
    if expanded[degree] >= expanded[count]:
        raise GeometryEvaluationError(
            "NURBS active parameter interval is empty",
            code="geometry.invalid_nurbs",
            field=prefix + "knots",
        )
    return expanded


def _controls(vertices, count, dimension, rational):
    expected = dimension + int(rational)
    if len(vertices) != count or any(len(p) != expected for p in vertices):
        raise GeometryEvaluationError(
            "NURBS control dimensions do not match",
            code="geometry.invalid_nurbs",
            field="control_vertices",
        )
    if any(not isfinite(v) for p in vertices for v in p):
        raise GeometryEvaluationError(
            "NURBS controls must be finite", code="geometry.invalid_nurbs", field="control_vertices"
        )
    if rational and any(p[-1] <= 0 for p in vertices):
        raise GeometryEvaluationError(
            "NURBS weights must be positive",
            code="geometry.invalid_nurbs",
            field="control_vertices",
        )
    return tuple(tuple(p) if rational else (*p, 1.0) for p in vertices)


def _span(degree, count, knots, t, field="parameter"):
    if not knots[degree] <= t <= knots[count]:
        raise GeometryEvaluationError(
            "parameter is outside the NURBS active knot interval",
            code="geometry.parameter_out_of_range",
            field=field,
            details={"parameter": t, "minimum": knots[degree], "maximum": knots[count]},
        )
    return count - 1 if t == knots[count] else bisect_right(knots, t) - 1


def _deboor(controls, degree, knots, t):
    """Evaluate homogeneous controls; endpoints use the adjacent active span."""
    span = _span(degree, len(controls), knots, t)
    return _deboor_local(controls[span - degree : span + 1], degree, knots, t, span)


def _deboor_local(controls, degree, knots, t, span):
    work = [list(p) for p in controls]
    for level in range(1, degree + 1):
        for j in range(degree, level - 1, -1):
            i = span - degree + j
            width = knots[i + degree - level + 1] - knots[i]
            alpha = (t - knots[i]) / width if width else 0.0
            work[j] = [
                (1 - alpha) * a + alpha * b for a, b in zip(work[j - 1], work[j], strict=True)
            ]
    return tuple(work[degree])


def _derivative_controls(controls, degree, knots):
    derived = []
    for i in range(len(controls) - 1):
        width = knots[i + degree + 1] - knots[i + 1]
        derived.append(
            tuple(
                (b - a) * degree / width if width else 0.0
                for a, b in zip(controls[i], controls[i + 1], strict=True)
            )
        )
    return tuple(derived)


def _cartesian(point, derivative):
    weight = point[-1]
    if weight <= 0 or not isfinite(weight):
        raise GeometryEvaluationError(
            "NURBS evaluation has an invalid homogeneous weight",
            code="geometry.nonfinite_result",
            field="weight",
        )
    position = tuple(v / weight for v in point[:-1])
    tangent = tuple(
        (v - p * derivative[-1]) / weight for p, v in zip(position, derivative[:-1], strict=True)
    )
    return position, tangent


def _pair(values):
    result = tuple(values)
    if len(result) != 2 or not all(isfinite(value) for value in result):
        raise GeometryEvaluationError(
            "evaluation produced invalid 2D coordinates", code="geometry.nonfinite_result"
        )
    return result


class _NurbsCurveEvaluator:
    def __init__(self, curve, dimension=3):
        if curve.periodic:
            raise GeometryEvaluationError(
                "periodic NURBS evaluation is not yet supported",
                code="geometry.unsupported_geometry",
                field="periodic",
                kind=DiagnosticKind.UNSUPPORTED,
            )
        if curve.vertex_dimension - int(curve.rational) != dimension:
            raise GeometryEvaluationError(
                f"curve evaluation requires {dimension}D NURBS controls",
                code="geometry.invalid_nurbs",
                field="vertex_dimension",
            )
        self.degree = curve.degree
        self.knots = _knots(
            curve.degree, curve.control_vertex_count, curve.knots, curve.knot_multiplicities
        )
        self.controls = _controls(
            curve.control_vertices, curve.control_vertex_count, dimension, curve.rational
        )
        self.derivatives = _derivative_controls(self.controls, self.degree, self.knots)
        self.derivative_knots = self.knots[1:-1]

    def values(self, parameter):
        point = _deboor(self.controls, self.degree, self.knots, parameter)
        derivative = _deboor(self.derivatives, self.degree - 1, self.derivative_knots, parameter)
        return _cartesian(point, derivative)

    def evaluate(self, parameter):
        position, tangent = self.values(parameter)
        return CurveEvaluation(_vector(position), _vector(tangent))

    def evaluate_2d(self, parameter):
        position, tangent = self.values(parameter)
        return CurveEvaluation2D(_pair(position), _pair(tangent))


def _reference(items, entity_id, role, field):
    matches = [item for item in items if item.id == entity_id]
    if len(matches) != 1:
        raise GeometryEvaluationError(
            f"{role} is missing or ambiguous",
            code="geometry.invalid_reference",
            field=field,
            details={"reference_id": entity_id},
        )
    return matches[0]


def _trim_evaluator(definition, model, seen, prepare):
    if len(seen) >= 64:
        raise GeometryEvaluationError(
            "trimmed curve depth exceeds 64",
            code="geometry.depth_exceeded",
            field="basis_curve",
            kind=DiagnosticKind.LIMIT,
            details={"limit": 64},
        )
    if model is None or definition.basis_curve in seen:
        raise GeometryEvaluationError(
            "trimmed curve needs an acyclic basis in its BrepModel",
            code="geometry.missing_model" if model is None else "geometry.cyclic_reference",
            field="model" if model is None else "basis_curve",
            details={"reference_id": definition.basis_curve},
        )
    if not isinstance(model, BrepModel):
        raise TypeError("model must be a BrepModel")
    start, end = (
        _parameter(definition.start_parameter, "start_parameter"),
        _parameter(definition.end_parameter, "end_parameter"),
    )
    basis = _reference(model.curves, definition.basis_curve, "trimmed basis curve", "basis_curve")
    evaluate = prepare(basis, model, (*seen, definition.basis_curve))

    def trimmed(t):
        if not min(start, end) <= t <= max(start, end):
            raise GeometryEvaluationError(
                "parameter is outside the stored trim interval",
                code="geometry.parameter_out_of_range",
                field="parameter",
                details={"parameter": t, "minimum": min(start, end), "maximum": max(start, end)},
            )
        return evaluate(t)

    return trimmed


def _curve_context(prepare):
    """Attach the failing curve, including nested bases and parameter curves."""

    @wraps(prepare)
    def contextual(curve, model, seen=()):
        try:
            evaluate = prepare(curve, model, seen)
        except GeometryEvaluationError as error:
            error.add_context(entity=curve, schema_key=getattr(model, "schema_key", None))
            raise

        def evaluated(parameter):
            try:
                return evaluate(parameter)
            except GeometryEvaluationError as error:
                error.add_context(
                    entity=curve, schema_key=getattr(model, "schema_key", None), parameter=parameter
                )
                raise

        return evaluated

    return contextual


@_curve_context
def _curve_2d_evaluator(curve, model, seen=()):
    definition = curve.definition if isinstance(curve, CurveGeometry) else curve
    if isinstance(definition, NurbsCurve):
        return _NurbsCurveEvaluator(definition, dimension=2).evaluate_2d
    if isinstance(definition, TrimmedCurve):
        return _trim_evaluator(definition, model, seen, _curve_2d_evaluator)
    raise GeometryEvaluationError(
        f"unsupported 2D curve evaluation: {type(definition).__name__}",
        code="geometry.unsupported_geometry",
        kind=DiagnosticKind.UNSUPPORTED,
    )


@_curve_context
def _surface_curve_evaluator(curve, model, seen=()):
    definition = curve.definition if isinstance(curve, CurveGeometry) else curve
    if isinstance(definition, TrimmedCurve):
        return _trim_evaluator(definition, model, seen, _surface_curve_evaluator)
    if not isinstance(definition, SurfaceParametricCurve):
        raise GeometryEvaluationError(
            "surface curve evaluation requires an SP_CURVE definition",
            code="geometry.unsupported_geometry",
            kind=DiagnosticKind.UNSUPPORTED,
        )
    if not isinstance(model, BrepModel):
        raise GeometryEvaluationError(
            "surface curve evaluation needs its BrepModel",
            code="geometry.missing_model",
            field="model",
        )
    uv_curve = _reference(
        model.curves, definition.parameter_curve, "SP_CURVE parameter curve", "parameter_curve"
    )
    support = _reference(model.surfaces, definition.surface, "SP_CURVE support surface", "surface")
    evaluate_uv = _curve_2d_evaluator(uv_curve, model, seen)
    try:
        derivatives = _surface_derivatives_evaluator(support)
    except GeometryEvaluationError as error:
        error.add_context(entity=support)
        raise

    def evaluate(t):
        uv = evaluate_uv(t)
        try:
            point, du, dv = derivatives(*uv.point)
        except GeometryEvaluationError as error:
            error.add_context(entity=support, u=uv.point[0], v=uv.point[1], curve_parameter=t)
            raise
        tangent = _vector(_add(_scale(du, uv.tangent[0]), _scale(dv, uv.tangent[1])))
        return SurfaceCurveEvaluation(uv.point, uv.tangent, point, tangent)

    return evaluate


@_curve_context
def _curve_evaluator(curve, model, seen=()):
    if len(seen) > 64:
        raise GeometryEvaluationError(
            "trimmed curve depth exceeds 64",
            code="geometry.depth_exceeded",
            field="basis_curve",
            kind=DiagnosticKind.LIMIT,
            details={"limit": 64},
        )
    definition = curve.definition if isinstance(curve, CurveGeometry) else curve
    if isinstance(definition, NurbsCurve):
        return _NurbsCurveEvaluator(definition).evaluate
    if isinstance(definition, TrimmedCurve):
        return _trim_evaluator(definition, model, seen, _curve_evaluator)
    if isinstance(definition, SurfaceParametricCurve):
        evaluate_sp = _surface_curve_evaluator(curve, model, seen)

        def surface_curve(t):
            result = evaluate_sp(t)
            return CurveEvaluation(result.point, result.tangent)

        return surface_curve
    if isinstance(definition, LineCurve):
        _unit(tuple(definition.direction), "direction")
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
                raise GeometryEvaluationError(
                    "analytic curve radii must be finite and positive",
                    code="geometry.invalid_geometry",
                    field="radius",
                )
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
    raise GeometryEvaluationError(
        f"unsupported curve evaluation: {type(definition).__name__}",
        code="geometry.unsupported_geometry",
        kind=DiagnosticKind.UNSUPPORTED,
    )


def evaluate_curve(
    curve: CurveGeometry | CurveDefinition, parameter: float, *, model: BrepModel | None = None
) -> CurveEvaluation:
    """Evaluate P(t) and dP/dt without OCCT. Source sense does not reverse dP/dt.

    Trimmed curves use their stored basis parameter, never a guessed 0..1 range.
    NURBS controls are homogeneous when rational; periodic forms are rejected.
    """
    try:
        return _curve_evaluator(curve, model)(_parameter(parameter))
    except GeometryEvaluationError as error:
        error.add_context(
            entity=curve, schema_key=getattr(model, "schema_key", None), parameter=parameter
        )
        raise
    except OverflowError as error:
        raise GeometryEvaluationError(
            "curve evaluation overflowed", code="geometry.nonfinite_result"
        ).add_context(
            entity=curve, parameter=parameter, schema_key=getattr(model, "schema_key", None)
        ) from error


def evaluate_curve_2d(
    curve: CurveGeometry | NurbsCurve | TrimmedCurve,
    parameter: float,
    *,
    model: BrepModel | None = None,
) -> CurveEvaluation2D:
    """Evaluate a nonperiodic 2D NURBS point and d(U,V)/dt, without OCCT.

    Rational controls are homogeneous (u*w, v*w, w). Trim wrappers require
    their model and retain stored basis parameters. Source sense is not applied.
    """
    try:
        return _curve_2d_evaluator(curve, model)(_parameter(parameter))
    except GeometryEvaluationError as error:
        error.add_context(
            entity=curve, schema_key=getattr(model, "schema_key", None), parameter=parameter
        )
        raise
    except OverflowError as error:
        raise GeometryEvaluationError(
            "2D curve evaluation overflowed", code="geometry.nonfinite_result"
        ).add_context(
            entity=curve, parameter=parameter, schema_key=getattr(model, "schema_key", None)
        ) from error


def evaluate_surface_curve(
    curve: CurveGeometry | SurfaceParametricCurve | TrimmedCurve,
    parameter: float,
    *,
    model: BrepModel,
) -> SurfaceCurveEvaluation:
    """Evaluate SP_CURVE UV values, 3D point, and dS/du*du/dt + dS/dv*dv/dt.

    References must resolve uniquely within model. Unsupported supports are
    rejected; original_curve is never substituted. Trim containment on a FACE,
    source senses, units conversion, and normal generation are not applied.
    """
    try:
        return _surface_curve_evaluator(curve, model)(_parameter(parameter))
    except GeometryEvaluationError as error:
        error.add_context(
            entity=curve, schema_key=getattr(model, "schema_key", None), parameter=parameter
        )
        raise
    except OverflowError as error:
        raise GeometryEvaluationError(
            "surface curve evaluation overflowed", code="geometry.nonfinite_result"
        ).add_context(
            entity=curve, parameter=parameter, schema_key=getattr(model, "schema_key", None)
        ) from error


_Evaluation = TypeVar("_Evaluation")


def _evaluate_many(
    evaluate: Callable[[float], _Evaluation],
    parameters: Iterable[float],
    max_samples: int,
) -> tuple[_Evaluation, ...]:
    if type(max_samples) is not int or not 1 <= max_samples <= 1_000_000:
        raise GeometryEvaluationError(
            "max_samples must be an integer between 1 and 1000000",
            code="geometry.invalid_parameter",
            field="max_samples",
            details={"value": max_samples},
        )
    results = []
    parameter = None
    try:
        for parameter in parameters:
            if len(results) >= max_samples:
                raise GeometryEvaluationError(
                    "batch evaluation exceeds max_samples",
                    code="geometry.limit_exceeded",
                    kind=DiagnosticKind.LIMIT,
                    field="max_samples",
                    details={
                        "resource": "max_samples",
                        "observed": len(results) + 1,
                        "limit": max_samples,
                    },
                )
            results.append(evaluate(_parameter(parameter)))
    except GeometryEvaluationError as error:
        error.add_context(sample_index=len(results), parameter=parameter)
        raise
    except OverflowError as error:
        raise GeometryEvaluationError(
            "batch curve evaluation overflowed", code="geometry.nonfinite_result"
        ).add_context(sample_index=len(results), parameter=parameter) from error
    return tuple(results)


def evaluate_curve_2d_many(
    curve: CurveGeometry | NurbsCurve | TrimmedCurve,
    parameters: Iterable[float],
    *,
    model: BrepModel | None = None,
    max_samples: int = 1_000_000,
) -> tuple[CurveEvaluation2D, ...]:
    """Evaluate 2D parameters in input order, preparing knots/derivatives once.

    Iterables are bounded by max_samples; an empty input returns (). An invalid
    parameter or sample-limit failure raises without returning partial results.
    """
    try:
        return _evaluate_many(_curve_2d_evaluator(curve, model), parameters, max_samples)
    except GeometryEvaluationError as error:
        error.add_context(entity=curve, schema_key=getattr(model, "schema_key", None))
        raise


def evaluate_surface_curve_many(
    curve: CurveGeometry | SurfaceParametricCurve | TrimmedCurve,
    parameters: Iterable[float],
    *,
    model: BrepModel,
    max_samples: int = 1_000_000,
) -> tuple[SurfaceCurveEvaluation, ...]:
    """Lift a batch onto one support, preparing references and NURBS data once.

    Parameter order, units, sense and failure semantics match the scalar API.
    NURBS evaluation touches only each sample's local knot spans. No global
    cache is retained and the immutable source model is unchanged.
    """
    try:
        return _evaluate_many(_surface_curve_evaluator(curve, model), parameters, max_samples)
    except GeometryEvaluationError as error:
        error.add_context(entity=curve, schema_key=getattr(model, "schema_key", None))
        raise


def sample_curve(
    curve: CurveGeometry | CurveDefinition,
    start: float,
    end: float,
    count: int = 100,
    *,
    model: BrepModel | None = None,
) -> tuple[Vector3, ...]:
    """Sample uniformly in stored parameter, including endpoints; bounded to a million points."""
    try:
        return _sample_curve(curve, start, end, count, model=model)
    except GeometryEvaluationError as error:
        error.add_context(entity=curve, schema_key=getattr(model, "schema_key", None))
        raise
    except OverflowError as error:
        raise GeometryEvaluationError(
            "curve sampling overflowed", code="geometry.nonfinite_result"
        ).add_context(entity=curve, schema_key=getattr(model, "schema_key", None)) from error


def _sample_curve(
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
    start, end = _parameter(start, "start"), _parameter(end, "end")
    if isinstance(count, bool) or not isinstance(count, int) or not 2 <= count <= 1_000_000:
        raise GeometryEvaluationError(
            "sample count must be an integer between 2 and 1000000",
            code="geometry.invalid_parameter",
            field="count",
            details={"value": count},
        )
    evaluate = _curve_evaluator(curve, model)
    results = []
    try:
        for i in range(count):
            parameter = (
                start
                if i == 0
                else end
                if i == count - 1
                else start * (1 - i / (count - 1)) + end * i / (count - 1)
            )
            results.append(evaluate(parameter).point)
    except GeometryEvaluationError as error:
        error.add_context(sample_index=i, parameter=parameter)
        raise
    except OverflowError as error:
        raise GeometryEvaluationError(
            "curve sampling overflowed", code="geometry.nonfinite_result"
        ).add_context(sample_index=i, parameter=parameter) from error
    return tuple(results)


def _tensor_point(rows, u_degree, v_degree, u_knots, v_knots, u, v):
    u_span = _span(u_degree, len(rows), u_knots, u, "u")
    v_span = _span(v_degree, len(rows[0]), v_knots, v, "v")
    values = tuple(
        _deboor_local(row[v_span - v_degree : v_span + 1], v_degree, v_knots, v, v_span)
        for row in rows[u_span - u_degree : u_span + 1]
    )
    return _deboor_local(values, u_degree, u_knots, u, u_span)


class _NurbsSurfaceEvaluator:
    def __init__(self, surface):
        if surface.u_periodic or surface.v_periodic:
            raise GeometryEvaluationError(
                "periodic NURBS evaluation is not yet supported",
                code="geometry.unsupported_geometry",
                field="u_periodic" if surface.u_periodic else "v_periodic",
                kind=DiagnosticKind.UNSUPPORTED,
            )
        if surface.vertex_dimension != 3 + int(surface.rational):
            raise GeometryEvaluationError(
                "surface evaluation requires 3D NURBS controls",
                code="geometry.invalid_nurbs",
                field="vertex_dimension",
            )
        nu, nv = surface.u_control_vertex_count, surface.v_control_vertex_count
        self.u_degree, self.v_degree = surface.u_degree, surface.v_degree
        self.u_knots = _knots(
            self.u_degree, nu, surface.u_knots, surface.u_knot_multiplicities, "u_"
        )
        self.v_knots = _knots(
            self.v_degree, nv, surface.v_knots, surface.v_knot_multiplicities, "v_"
        )
        controls = _controls(surface.control_vertices, nu * nv, 3, surface.rational)
        # Parasolid stores V as the fast-changing control index.
        self.rows = tuple(controls[i * nv : (i + 1) * nv] for i in range(nu))
        self.du_rows = tuple(
            zip(
                *(
                    _derivative_controls(tuple(column), self.u_degree, self.u_knots)
                    for column in zip(*self.rows, strict=True)
                ),
                strict=True,
            )
        )
        self.dv_rows = tuple(
            _derivative_controls(row, self.v_degree, self.v_knots) for row in self.rows
        )
        self.du_knots, self.dv_knots = self.u_knots[1:-1], self.v_knots[1:-1]

    def derivatives(self, u, v):
        point = _tensor_point(
            self.rows, self.u_degree, self.v_degree, self.u_knots, self.v_knots, u, v
        )
        derivative_u = _tensor_point(
            self.du_rows, self.u_degree - 1, self.v_degree, self.du_knots, self.v_knots, u, v
        )
        derivative_v = _tensor_point(
            self.dv_rows, self.u_degree, self.v_degree - 1, self.u_knots, self.dv_knots, u, v
        )
        position, du = _cartesian(point, derivative_u)
        _, dv = _cartesian(point, derivative_v)
        return _vector(position, "point"), _vector(du, "du"), _vector(dv, "dv")

    def evaluate(self, u, v):
        point, du, dv = self.derivatives(u, v)
        return SurfaceEvaluation(
            point, du, dv, _vector(_unit(_cross(tuple(du), tuple(dv)), "normal"))
        )


def _surface_derivatives_evaluator(surface):
    definition = surface.definition if isinstance(surface, SurfaceGeometry) else surface
    if isinstance(definition, NurbsSurface):
        return _NurbsSurfaceEvaluator(definition).derivatives
    if not isinstance(definition, (PlaneSurface, CylinderSurface, SphereSurface, TorusSurface)):
        raise GeometryEvaluationError(
            f"unsupported surface evaluation: {type(definition).__name__}",
            code="geometry.unsupported_geometry",
            kind=DiagnosticKind.UNSUPPORTED,
        )

    def derivatives(u, v):
        result = evaluate_surface(definition, u, v)
        return result.point, result.du, result.dv

    return derivatives


def evaluate_surface(
    surface: SurfaceGeometry | SurfaceDefinition, u: float, v: float
) -> SurfaceEvaluation:
    """Evaluate a supported source surface, its partials and du-cross-dv normal."""
    try:
        return _evaluate_surface(surface, u, v)
    except GeometryEvaluationError as error:
        error.add_context(entity=surface, u=u, v=v)
        raise
    except OverflowError as error:
        raise GeometryEvaluationError(
            "surface evaluation overflowed", code="geometry.nonfinite_result"
        ).add_context(entity=surface, u=u, v=v) from error


def _evaluate_surface(
    surface: SurfaceGeometry | SurfaceDefinition, u: float, v: float
) -> SurfaceEvaluation:
    """Evaluate a plane, cylinder, sphere, torus, or nonperiodic NURBS surface.

    Parameters and coordinates retain source units. Normal follows du cross dv;
    sphere poles use the analytic limiting normal. Face/surface senses and trim
    containment are not applied. Singular NURBS normals raise an error.
    """
    u, v = _parameter(u, "u"), _parameter(v, "v")
    definition = surface.definition if isinstance(surface, SurfaceGeometry) else surface
    if isinstance(definition, NurbsSurface):
        return _NurbsSurfaceEvaluator(definition).evaluate(u, v)
    if isinstance(definition, PlaneSurface):
        x, y, z = _frame(definition.normal, definition.x_axis)
        point = _add(definition.point, _add(_scale(x, u), _scale(y, v)))
        return SurfaceEvaluation(_vector(point), _vector(x), _vector(y), _vector(z))
    if not isinstance(definition, (CylinderSurface, SphereSurface, TorusSurface)):
        raise GeometryEvaluationError(
            f"unsupported surface evaluation: {type(definition).__name__}",
            code="geometry.unsupported_geometry",
            kind=DiagnosticKind.UNSUPPORTED,
        )
    x, y, z = _frame(definition.axis, definition.x_axis, "axis")
    radial = _add(_scale(x, cos(u)), _scale(y, sin(u)))
    angular = _add(_scale(x, -sin(u)), _scale(y, cos(u)))
    if isinstance(definition, CylinderSurface):
        radius = definition.radius
        point = _add(definition.point, _add(_scale(radial, radius), _scale(z, v)))
        du, dv, normal = _scale(angular, radius), z, radial
    elif isinstance(definition, SphereSurface):
        radius = definition.radius
        if not -pi / 2 <= v <= pi / 2:
            raise GeometryEvaluationError(
                "sphere latitude must be between -pi/2 and pi/2",
                code="geometry.parameter_out_of_range",
                field="v",
                details={"parameter": v, "minimum": -pi / 2, "maximum": pi / 2},
            )
        normal = _add(_scale(radial, cos(v)), _scale(z, sin(v)))
        point = _add(definition.center, _scale(normal, radius))
        du = _scale(angular, radius * cos(v))
        dv = _scale(_add(_scale(radial, -sin(v)), _scale(z, cos(v))), radius)
    else:
        radius = definition.minor_radius
        if not isfinite(definition.major_radius) or definition.major_radius <= radius:
            raise GeometryEvaluationError(
                "torus evaluation currently requires a ring torus",
                code="geometry.unsupported_geometry",
                kind=DiagnosticKind.UNSUPPORTED,
            )
        normal = _add(_scale(radial, cos(v)), _scale(z, sin(v)))
        point = _add(
            definition.center, _add(_scale(radial, definition.major_radius), _scale(normal, radius))
        )
        du = _scale(angular, definition.major_radius + radius * cos(v))
        dv = _scale(_add(_scale(radial, -sin(v)), _scale(z, cos(v))), radius)
    if not isfinite(radius) or radius <= 0:
        raise GeometryEvaluationError(
            "surface radius must be finite and positive",
            code="geometry.invalid_geometry",
            field="radius",
        )
    return SurfaceEvaluation(_vector(point), _vector(du), _vector(dv), _vector(normal))
