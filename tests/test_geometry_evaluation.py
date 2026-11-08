"""Independent analytic oracles for dependency-free source geometry evaluation."""

from dataclasses import replace
from math import pi, sqrt

import pytest

from parasolid_kit import (
    CircleCurve,
    CylinderSurface,
    GeometryEvaluationError,
    LineCurve,
    NurbsCurve,
    NurbsSurface,
    PlaneSurface,
    SphereSurface,
    Vector3,
    evaluate_curve,
    evaluate_surface,
    sample_curve,
)
from tests.support.import_contract import assert_dependency_free_imports


def curve(*, rational=False, knots=(0.0, 1.0)):
    points = ((0.0, 0.0, 0.0), (1.0, 2.0, 0.0), (3.0, 0.0, 0.0))
    if rational:
        q = sqrt(0.5)
        points = ((1.0, 0.0, 0.0, 1.0), (q, q, 0.0, q), (0.0, 1.0, 0.0, 1.0))
    return NurbsCurve(
        2, 3, 4 if rational else 3, 0, False, False, rational, 0, points, knots, (3, 3), ()
    )


def test_line_circle_and_sampling_keep_source_coordinates_and_endpoint_order():
    line = LineCurve(Vector3(2.0, 3.0, -1.0), Vector3(1.0, 0.0, 0.0))
    result = evaluate_curve(line, -2)
    assert tuple(result.point) == (0.0, 3.0, -1.0)
    assert tuple(result.tangent) == (1.0, 0.0, 0.0)
    circle = CircleCurve(
        Vector3(1.0, 2.0, 3.0), Vector3(0.0, 1.0, 0.0), Vector3(1.0, 0.0, 0.0), 2.0
    )
    at = evaluate_curve(circle, pi / 2)
    assert tuple(at.point) == pytest.approx((1, 2, 1))
    assert tuple(at.tangent) == pytest.approx((-2, 0, 0))
    samples = sample_curve(line, 2, -2, 5)
    assert [point.x for point in samples] == [4, 3, 2, 1, 0]


@pytest.mark.parametrize("t", [0.0, 0.125, 0.5, 0.875, 1.0])
def test_quadratic_nurbs_matches_bezier_position_and_exact_derivative(t):
    result = evaluate_curve(curve(), t)
    assert tuple(result.point) == pytest.approx((2 * t + t * t, 4 * t * (1 - t), 0))
    assert tuple(result.tangent) == pytest.approx((2 + 2 * t, 4 - 8 * t, 0))


def test_rational_homogeneous_quarter_circle_and_tiny_knot_domain():
    result = evaluate_curve(curve(rational=True), 0.5)
    assert tuple(result.point) == pytest.approx((sqrt(0.5), sqrt(0.5), 0))
    denominator = (1 + sqrt(0.5)) / 2
    assert tuple(result.tangent) == pytest.approx((-1 / denominator, 1 / denominator, 0))
    tiny = curve(knots=(0.0, 1e-20))
    result = evaluate_curve(tiny, 0.5e-20)
    assert tuple(result.point) == pytest.approx((1.25, 1, 0))
    assert tuple(result.tangent) == pytest.approx((3e20, 0, 0))
    assert sample_curve(tiny, 0, 1e-20, 3)[-1] == Vector3(3.0, 0.0, 0.0)


def test_tensor_product_surface_orders_controls_and_keeps_scaled_derivatives():
    surface = NurbsSurface(
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
        ((1.0, 2.0, 3.0), (1.0, 7.0, 5.0), (5.0, 2.0, 6.0), (5.0, 7.0, 8.0)),
        (0.0, 2.0),
        (10.0, 11.0),
        (2, 2),
        (2, 2),
        (),
    )
    for rational in (False, True):
        geometry = (
            replace(
                surface,
                rational=True,
                vertex_dimension=4,
                control_vertices=tuple(
                    (*[2 * v for v in p], 2.0) for p in surface.control_vertices
                ),
            )
            if rational
            else surface
        )
        result = evaluate_surface(geometry, 1, 10.5)
        assert tuple(result.point) == pytest.approx((3, 4.5, 5.5))
        assert tuple(result.du) == pytest.approx((2, 0, 1.5))
        assert tuple(result.dv) == pytest.approx((0, 5, 2))
        normal = (-7.5, -4.0, 10.0)
        length = sqrt(sum(v * v for v in normal))
        assert tuple(result.normal) == pytest.approx(tuple(v / length for v in normal))
    assert tuple(evaluate_surface(surface, 2, 11).point) == (5, 7, 8)


def test_analytic_surfaces_have_oriented_normals_including_sphere_poles():
    origin, axis, x = Vector3(1.0, 2.0, 3.0), Vector3(0.0, 0.0, 1.0), Vector3(1.0, 0.0, 0.0)
    result = evaluate_surface(PlaneSurface(origin, axis, x), 4, -2)
    assert tuple(result.point) == (5, 0, 3)
    assert tuple(result.normal) == (0, 0, 1)
    result = evaluate_surface(CylinderSurface(origin, axis, 2.0, x), pi / 2, 5)
    assert tuple(result.point) == pytest.approx((1, 4, 8))
    assert tuple(result.du) == pytest.approx((-2, 0, 0))
    sphere = SphereSurface(origin, 2.0, axis, x)
    assert tuple(evaluate_surface(sphere, 0.4, pi / 2).normal) == pytest.approx((0, 0, 1))


@pytest.mark.parametrize("parameter", [float("nan"), float("inf"), True, 10**1000])
def test_invalid_parameters_are_rejected(parameter):
    with pytest.raises(GeometryEvaluationError):
        evaluate_curve(curve(), parameter)


@pytest.mark.parametrize(
    "change",
    [
        {"periodic": True},
        {"knots": (0.0, 0.0)},
        {"knot_multiplicities": (4, 2)},
        {"control_vertex_count": 2},
        {"control_vertices": ((0.0, 0.0),) * 3},
    ],
)
def test_invalid_or_unqualified_nurbs_are_rejected(change):
    with pytest.raises(GeometryEvaluationError):
        evaluate_curve(replace(curve(), **change), 0.5)
    with pytest.raises(GeometryEvaluationError):
        evaluate_curve(curve(), 1.1)


def test_new_public_operations_do_not_import_optional_cad_runtimes():
    assert_dependency_free_imports("""
from parasolid_kit import (evaluate_curve, evaluate_surface, sample_curve,
    assembly_occurrences, faceted_geometry, decode_polyline, decode_embedded_mesh,
    LineCurve, Vector3)
from parasolid_kit.interop.preview import write_faceted_preview
assert evaluate_curve(LineCurve(Vector3(0.,0.,0.), Vector3(1.,0.,0.)), 2).point.x == 2
from parasolid_kit import FacetedGeometry, Polyline, Sense, SourceNodeRef
from parasolid_kit.binary import ByteRange
from pathlib import Path
from tempfile import TemporaryDirectory
source = SourceNodeRef(1, 180, "POLYLINE", None, ByteRange(0, 10))
line = Polyline((Vector3(0.,0.,0.), Vector3(1.,0.,0.)), False, Sense.POSITIVE, 0., source)
with TemporaryDirectory() as temporary:
    result = write_faceted_preview(FacetedGeometry((line,), (), "SCH_3900000_99001", "text"),
        Path(temporary) / "preview", source_unit="mm")
    assert result.polyline_count == 1 and result.glb_path.is_file()
""")
