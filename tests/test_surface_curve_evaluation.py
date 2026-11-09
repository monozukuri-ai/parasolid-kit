"""UV lifting checked against analytic derivatives and optional independent OCCT D1."""

from dataclasses import replace
from itertools import repeat
from math import cos, sin, sqrt

import pytest

from parasolid_kit import (
    BrepMetrics,
    BrepModel,
    ByteRange,
    CurveEvaluation2D,
    CurveGeometry,
    CurveKind,
    CylinderSurface,
    GeometryEvaluationError,
    LineCurve,
    NurbsCurve,
    NurbsSurface,
    Sense,
    SourceNodeRef,
    SurfaceCurveEvaluation,
    SurfaceGeometry,
    SurfaceKind,
    SurfaceParametricCurve,
    TopologyValidation,
    TrimmedCurve,
    UnsupportedGeometry,
    Vector3,
    evaluate_curve,
    evaluate_curve_2d,
    evaluate_curve_2d_many,
    evaluate_surface,
    evaluate_surface_curve,
    evaluate_surface_curve_many,
    sample_curve,
)
from parasolid_kit.brep import evaluation
from tests.support.import_contract import assert_dependency_free_imports

ORIGIN, Z, X = Vector3(0.0, 0.0, 0.0), Vector3(0.0, 0.0, 1.0), Vector3(1.0, 0.0, 0.0)


def uv_curve():
    # C(t) = (t, t**2).
    return NurbsCurve(
        2,
        3,
        2,
        0,
        False,
        False,
        False,
        0,
        ((0.0, 0.0), (0.5, 0.0), (1.0, 1.0)),
        (0.0, 1.0),
        (3, 3),
        (),
    )


def patch():
    # S(u,v) = (2*u, 3*v, u*v), with V-fast controls.
    return NurbsSurface(
        1,
        1,
        2,
        2,
        3,
        0,
        0,
        False,
        False,
        False,
        False,
        False,
        0,
        ((0.0, 0.0, 0.0), (0.0, 3.0, 0.0), (2.0, 0.0, 0.0), (2.0, 3.0, 1.0)),
        (0.0, 1.0),
        (0.0, 1.0),
        (2, 2),
        (2, 2),
        (),
    )


def source(entity_id):
    return SourceNodeRef(entity_id, 2, "authored", entity_id, ByteRange(0, 1))


def curve_entity(entity_id, definition, kind=CurveKind.NURBS):
    return CurveGeometry(entity_id, Sense.NEGATIVE, None, kind, definition, source(entity_id))


def model_for(uv=None, surface=None):
    uv = uv if uv is not None else uv_curve()
    surface = surface if surface is not None else patch()
    return BrepModel(
        source_format="text",
        schema_key="authored",
        complete=False,
        bodies=(),
        regions=(),
        shells=(),
        faces=(),
        loops=(),
        half_edges=(),
        edges=(),
        vertices=(),
        points=(),
        curves=(
            curve_entity(1, uv),
            curve_entity(2, SurfaceParametricCurve(1, 1, None, None), CurveKind.SURFACE_PARAMETRIC),
        ),
        surfaces=(SurfaceGeometry(1, Sense.NEGATIVE, None, SurfaceKind.NURBS, surface, source(3)),),
        topology=TopologyValidation(False, 0, 0, 0),
        metrics=BrepMetrics(None, None, None),
        diagnostics=(),
    )


@pytest.mark.parametrize("t", [0.0, 0.125, 0.5, 0.9, 1.0])
def test_uv_and_lifted_curve_match_polynomial_chain_rule(t):
    model = model_for()
    uv = evaluate_curve_2d(model.curves[0], t)
    assert isinstance(uv, CurveEvaluation2D)
    assert uv.point == pytest.approx((t, t * t))
    assert uv.tangent == pytest.approx((1, 2 * t))
    result = evaluate_surface_curve(model.curves[1], t, model=model)
    assert isinstance(result, SurfaceCurveEvaluation)
    assert result.uv == uv.point
    assert result.uv_tangent == uv.tangent
    assert tuple(result.point) == pytest.approx((2 * t, 3 * t * t, t**3))
    assert tuple(result.tangent) == pytest.approx((2, 6 * t, 3 * t * t))
    # Both source senses are negative: natural derivatives must stay unchanged.
    ordinary = evaluate_curve(model.curves[1], t, model=model)
    assert ordinary.point == result.point
    assert ordinary.tangent == result.tangent


def test_rational_2d_controls_are_homogeneous_and_derivatives_keep_parameter_scale():
    q = sqrt(0.5)
    curve = replace(
        uv_curve(),
        rational=True,
        vertex_dimension=3,
        control_vertices=((1.0, 0.0, 1.0), (q, q, q), (0.0, 1.0, 1.0)),
    )
    middle = evaluate_curve_2d(curve, 0.5)
    assert middle.point == pytest.approx((q, q))
    assert middle.tangent == pytest.approx((-2 / (1 + q), 2 / (1 + q)))
    assert evaluate_curve_2d(curve, 0).point == (1, 0)
    assert evaluate_curve_2d(curve, 1).point == (0, 1)
    tiny = replace(curve, knots=(0.0, 1e-20))
    scaled = evaluate_curve_2d(tiny, 0.5e-20)
    assert scaled.point == pytest.approx(middle.point)
    assert scaled.tangent == pytest.approx(tuple(value * 1e20 for value in middle.tangent))
    with pytest.raises(GeometryEvaluationError, match="3D"):
        evaluate_curve(curve, 0.5)


def test_cylinder_support_and_non_normalized_basis_parameters():
    uv = NurbsCurve(
        1, 2, 2, 0, False, False, False, 0, ((0.2, 1.0), (0.6, 3.0)), (2.0, 4.0), (2, 2), ()
    )
    model = model_for(uv, CylinderSurface(ORIGIN, Z, 2.0, X))
    result = evaluate_surface_curve(model.curves[1], 3, model=model)
    assert result.uv == pytest.approx((0.4, 2))
    assert result.uv_tangent == pytest.approx((0.2, 1))
    assert tuple(result.point) == pytest.approx((2 * cos(0.4), 2 * sin(0.4), 2))
    assert tuple(result.tangent) == pytest.approx((-0.4 * sin(0.4), 0.4 * cos(0.4), 1))
    with pytest.raises(GeometryEvaluationError, match="interval"):
        evaluate_surface_curve(model.curves[1], 0.5, model=model)


def rational_patch():
    return replace(
        patch(),
        rational=True,
        vertex_dimension=4,
        control_vertices=(
            (0.0, 0.0, 0.0, 1.0),
            (0.0, 6.0, 0.0, 2.0),
            (6.0, 0.0, 0.0, 3.0),
            (8.0, 12.0, 4.0, 4.0),
        ),
    )


@pytest.mark.parametrize("t", [0.0, 0.2, 0.5, 0.9, 1.0])
def test_varying_surface_weights_use_quotient_rule_before_chain_rule(t):
    model = model_for(surface=rational_patch())
    result = evaluate_surface_curve(model.curves[1], t, model=model)
    # Substitution u=t,v=t*t into this bilinear homogeneous patch.
    h = (6 * t + 2 * t**3, 6 * t * t + 6 * t**3, 4 * t**3)
    dh = (6 + 6 * t * t, 12 * t + 18 * t * t, 12 * t * t)
    w, dw = (1 + t) ** 2, 2 + 2 * t
    assert tuple(result.point) == pytest.approx(tuple(value / w for value in h))
    assert tuple(result.tangent) == pytest.approx(
        tuple((derivative * w - value * dw) / w**2 for value, derivative in zip(h, dh, strict=True))
    )


def test_one_sided_derivative_at_internal_knot_and_endpoints():
    curve = NurbsCurve(
        1,
        3,
        2,
        0,
        False,
        False,
        False,
        0,
        ((0.0, 0.0), (1.0, 0.5), (3.0, 1.0)),
        (0.0, 0.5, 1.0),
        (2, 1, 2),
        (),
    )
    values = evaluate_curve_2d_many(curve, (0.0, 0.5, 1.0))
    assert values[0].tangent == (2, 1)
    assert values[1].point == (1, 0.5)
    assert values[1].tangent == (4, 1)
    assert values[2].tangent == (4, 1)


def test_normal_singularity_does_not_invalidate_surface_curve_point_and_tangent():
    surface = replace(
        patch(),
        control_vertices=((0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 0.0, 0.0), (2.0, 0.0, 0.0)),
    )
    with pytest.raises(GeometryEvaluationError, match="degenerate"):
        evaluate_surface(surface, 0.5, 0.25)
    model = model_for(surface=surface)
    result = evaluate_surface_curve(model.curves[1], 0.5, model=model)
    assert tuple(result.point) == (0.75, 0, 0)
    assert tuple(result.tangent) == (2, 0, 0)


def test_trim_wrappers_retain_basis_parameters_and_support_sampling():
    model = model_for(replace(uv_curve(), knots=(2.0, 4.0)))
    uv_trim = curve_entity(3, TrimmedCurve(1, ORIGIN, ORIGIN, 2.5, 3.5), CurveKind.TRIMMED)
    sp = replace(model.curves[1], definition=SurfaceParametricCurve(1, 3, None, None))
    trim = curve_entity(4, TrimmedCurve(2, ORIGIN, ORIGIN, 3.25, 2.75), CurveKind.TRIMMED)
    model = replace(model, curves=(model.curves[0], sp, uv_trim, trim))
    result = evaluate_surface_curve(trim, 3, model=model)
    assert result.uv == (0.5, 0.25)
    assert result.uv_tangent == (0.5, 0.5)
    assert tuple(result.tangent) == pytest.approx((1, 1.5, 0.375))
    assert evaluate_curve_2d(uv_trim, 3, model=model).point == result.uv
    parameters = (3.25, 3.0, 2.75)
    detailed = evaluate_surface_curve_many(trim, parameters, model=model)
    assert sample_curve(trim, 3.25, 2.75, 3, model=model) == tuple(x.point for x in detailed)
    for parameter in (0.5, 2.5, 3.5):
        with pytest.raises(GeometryEvaluationError, match="trim interval"):
            evaluate_surface_curve(trim, parameter, model=model)
    with pytest.raises(GeometryEvaluationError, match="trim interval"):
        evaluate_surface_curve(sp, 2.25, model=model)


def test_batch_preserves_arbitrary_order_duplicates_empty_and_scalar_results():
    model = model_for(surface=rational_patch())
    parameters = (1.0, 0.2, 0.9, 0.2, 0.0)
    assert evaluate_curve_2d_many(uv_curve(), iter(parameters)) == tuple(
        evaluate_curve_2d(uv_curve(), t) for t in parameters
    )
    assert evaluate_surface_curve_many(model.curves[1], iter(parameters), model=model) == tuple(
        evaluate_surface_curve(model.curves[1], t, model=model) for t in parameters
    )
    assert evaluate_curve_2d_many(uv_curve(), []) == ()
    assert evaluate_surface_curve_many(model.curves[1], [], model=model) == ()


@pytest.mark.parametrize("maximum", [True, 0, -1, 1.5, 1_000_001])
def test_batch_limits_must_be_bounded_integers(maximum):
    with pytest.raises(GeometryEvaluationError, match="max_samples"):
        evaluate_curve_2d_many(uv_curve(), [], max_samples=maximum)
    model = model_for()
    with pytest.raises(GeometryEvaluationError, match="max_samples"):
        evaluate_surface_curve_many(model.curves[1], [], model=model, max_samples=maximum)


def test_batch_bounds_infinite_iterators_and_does_not_return_partial_results():
    with pytest.raises(GeometryEvaluationError, match="max_samples"):
        evaluate_curve_2d_many(uv_curve(), repeat(0.5), max_samples=3)
    model = model_for()
    with pytest.raises(GeometryEvaluationError, match="max_samples"):
        evaluate_surface_curve_many(model.curves[1], repeat(0.5), model=model, max_samples=3)
    for parameters in ((0.0, float("nan"), 1.0), (0.0, 1.1, 0.5), (0.0, True), (10**1000,)):
        with pytest.raises(GeometryEvaluationError):
            evaluate_curve_2d_many(uv_curve(), parameters)
        with pytest.raises(GeometryEvaluationError):
            evaluate_surface_curve_many(model.curves[1], parameters, model=model)


@pytest.mark.parametrize(
    "change",
    [
        {"periodic": True},
        {"vertex_dimension": 3},
        {"control_vertices": ((0.0, float("inf")), (0.5, 0.0), (1.0, 1.0))},
        {"knots": (0.0, 0.0)},
        {"knot_multiplicities": (4, 2)},
        {
            "rational": True,
            "vertex_dimension": 3,
            "control_vertices": ((0.0, 0.0, 1.0), (0.5, 0.0, 0.0), (1.0, 1.0, 1.0)),
        },
    ],
)
def test_invalid_uv_controls_rejected_before_lifting(change):
    curve = replace(uv_curve(), **change)
    with pytest.raises(GeometryEvaluationError):
        evaluate_curve_2d(curve, 0.5)
    model = model_for(curve)
    with pytest.raises(GeometryEvaluationError):
        evaluate_surface_curve(model.curves[1], 0.5, model=model)


def test_missing_ambiguous_references_and_unsupported_support_never_use_original_curve():
    model = model_for()
    original = curve_entity(9, LineCurve(ORIGIN, X), CurveKind.LINE)
    sp = SurfaceParametricCurve(1, 1, 9, 1e-3)
    cases = (
        replace(model, curves=(original,)),
        replace(model, curves=(*model.curves, model.curves[0], original)),
        replace(model, surfaces=()),
        replace(model, surfaces=model.surfaces * 2),
        model_for(surface=UnsupportedGeometry("unqualified")),
        model_for(surface=replace(patch(), u_periodic=True)),
        model_for(uv=replace(uv_curve(), control_vertices=((2.0, 0.0), (2.5, 0.0), (3.0, 1.0)))),
    )
    for candidate in cases:
        candidate = replace(candidate, curves=(*candidate.curves, original))
        with pytest.raises(GeometryEvaluationError):
            evaluate_surface_curve(sp, 0.5, model=candidate)
    with pytest.raises(GeometryEvaluationError, match="BrepModel"):
        evaluate_surface_curve(sp, 0.5, model=None)
    with pytest.raises(GeometryEvaluationError, match="SP_CURVE"):
        evaluate_surface_curve(uv_curve(), 0.5, model=model)
    with pytest.raises(GeometryEvaluationError, match="2D"):
        evaluate_curve_2d(LineCurve(ORIGIN, X), 0.5)


def test_trim_cycles_depth_and_ambiguous_bases_fail_closed():
    model = model_for()
    trim = TrimmedCurve(3, ORIGIN, ORIGIN, 0.0, 1.0)
    cycle = curve_entity(3, trim, CurveKind.TRIMMED)
    cyclic = replace(model, curves=(*model.curves, cycle))
    for evaluate in (evaluate_curve_2d, evaluate_surface_curve):
        with pytest.raises(GeometryEvaluationError, match="acyclic"):
            evaluate(cycle, 0.5, model=cyclic)
    chain = tuple(
        curve_entity(i, TrimmedCurve(i + 1, ORIGIN, ORIGIN, 0.0, 1.0), CurveKind.TRIMMED)
        for i in range(10, 76)
    )
    deep = replace(model, curves=(*model.curves, *chain, curve_entity(76, uv_curve())))
    with pytest.raises(GeometryEvaluationError, match="depth"):
        evaluate_curve_2d(chain[0], 0.5, model=deep)
    ambiguous = replace(model, curves=(*model.curves, model.curves[0]))
    with pytest.raises(GeometryEvaluationError, match="ambiguous"):
        evaluate_curve_2d(TrimmedCurve(1, ORIGIN, ORIGIN, 0, 1), 0.5, model=ambiguous)


def test_batch_preparation_cost_is_independent_of_parameter_count(monkeypatch):
    model = model_for(surface=rational_patch())
    before = model.to_dict()
    counts = dict.fromkeys(("_knots", "_controls", "_derivative_controls"), 0)
    for name in counts:
        original = getattr(evaluation, name)

        def counted(*args, _name=name, _original=original, **kwargs):
            counts[_name] += 1
            return _original(*args, **kwargs)

        monkeypatch.setattr(evaluation, name, counted)
    for run in (
        lambda values: evaluate_curve_2d_many(model.curves[0], values),
        lambda values: evaluate_surface_curve_many(model.curves[1], values, model=model),
    ):
        for name in counts:
            counts[name] = 0
        run([0.5])
        once = counts.copy()
        assert all(value > 0 for value in once.values())
        for name in counts:
            counts[name] = 0
        run([0.5] * 30)
        assert counts == once
    assert model.to_dict() == before


def test_multispan_rational_support_agrees_with_independent_occt_d1():
    pytest.importorskip("OCP")
    from OCP.gp import gp_Pnt, gp_Vec

    from parasolid_kit.interop.occt import OcctConversionOptions
    from parasolid_kit.interop.occt.geometry import GeometryFactory

    controls = tuple(
        (i * w, j * w, (i * j + 0.3 * j * j) * w, w)
        for i in range(5)
        for j in range(5)
        for w in [1 + 0.2 * i + 0.1 * j]
    )
    surface = replace(
        patch(),
        u_degree=2,
        v_degree=2,
        u_control_vertex_count=5,
        v_control_vertex_count=5,
        vertex_dimension=4,
        rational=True,
        control_vertices=controls,
        u_knots=(0.0, 0.3, 1.0),
        v_knots=(0.0, 0.7, 1.0),
        u_knot_multiplicities=(3, 2, 3),
        v_knot_multiplicities=(3, 2, 3),
    )
    model = model_for(surface=surface)
    occt = GeometryFactory(OcctConversionOptions(source_unit="mm", target_unit="mm"))
    support = occt.nurbs_surface(surface)
    parameters = (0.0, 0.1, 0.3, 0.6, sqrt(0.7), 0.9, 1.0)
    for t, value in zip(
        parameters,
        evaluate_surface_curve_many(model.curves[1], parameters, model=model),
        strict=True,
    ):
        point, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
        support.D1(t, t * t, point, du, dv)
        assert tuple(value.point) == pytest.approx(point.Coord(), abs=1e-12)
        assert tuple(value.tangent) == pytest.approx(
            tuple(a + 2 * t * b for a, b in zip(du.Coord(), dv.Coord(), strict=True)), abs=1e-11
        )


def test_public_uv_apis_are_dependency_free_in_fresh_interpreter():
    assert_dependency_free_imports("""
from parasolid_kit import *
from parasolid_kit.brep import evaluate_curve_2d_many, evaluate_surface_curve_many
uv = NurbsCurve(1,2,2,0,False,False,False,0,((0.,0.),(1.,1.)),(0.,1.),(2,2),())
assert evaluate_curve_2d_many(uv, [0,1])[1].point == (1,1)
model = BrepModel(source_format='text', schema_key='authored', complete=False,
    bodies=(), regions=(), shells=(), faces=(), loops=(), half_edges=(), edges=(),
    vertices=(), points=(),
    curves=(CurveGeometry(1,Sense.POSITIVE,None,CurveKind.NURBS,uv,None),),
    surfaces=(SurfaceGeometry(1,Sense.POSITIVE,None,SurfaceKind.PLANE,
        PlaneSurface(Vector3(0.,0.,0.),Vector3(0.,0.,1.),Vector3(1.,0.,0.)),None),),
    topology=TopologyValidation(False,0,0,0), metrics=BrepMetrics(None,None,None),
    diagnostics=())
values = evaluate_surface_curve_many(SurfaceParametricCurve(1,1,None,None), [0,1], model=model)
assert tuple(values[1].point) == (1,1,0)
assert tuple(values[1].tangent) == (1,1,0)
import sys
assert 'numpy' not in sys.modules
""")
