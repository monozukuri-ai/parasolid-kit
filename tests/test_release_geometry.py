"""Independent exact shapes exercise the release oracle's numerical evaluator."""

import math
from types import SimpleNamespace

import pytest

from scripts.release_geometry import curve_value, surface_value


def test_rational_quadratic_traces_a_unit_quarter_circle():
    weight = math.sqrt(0.5)
    curve = SimpleNamespace(
        degree=2,
        control_vertex_count=3,
        rational=True,
        periodic=False,
        knots=(0.0, 1.0),
        knot_multiplicities=(3, 3),
        control_vertices=((1.0, 0.0, 1.0), (weight, weight, weight), (0.0, 1.0, 1.0)),
    )
    for t in (0, 0.071, 0.173, 0.5, 0.619, 0.937, 1):
        x, y = curve_value(curve, t)
        assert math.hypot(x, y) == pytest.approx(1, abs=1e-14)
    assert curve_value(curve, 0.5) == pytest.approx([weight, weight], abs=1e-14)
    with pytest.raises(ValueError, match="outside"):
        curve_value(curve, 1.01)


def test_asymmetric_bilinear_saddle_preserves_surface_axis_order():
    surface = SimpleNamespace(
        u_degree=1,
        v_degree=1,
        u_control_vertex_count=2,
        v_control_vertex_count=2,
        u_knots=(0, 1),
        v_knots=(0, 1),
        u_knot_multiplicities=(2, 2),
        v_knot_multiplicities=(2, 2),
        u_periodic=False,
        v_periodic=False,
        rational=False,
        control_vertices=((0, 0, 0), (0, 3, 0), (2, 0, 0), (2, 3, 7)),
    )
    for u, v in ((0, 0), (1, 1), (0.173, 0.619), (0.371, 0.811)):
        assert surface_value(surface, u, v) == pytest.approx([2 * u, 3 * v, 7 * u * v], abs=1e-14)
