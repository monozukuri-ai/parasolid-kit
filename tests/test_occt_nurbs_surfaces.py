"""Rational and closed NURBS surfaces convert exactly.

Weights and the stored full knot vectors are honoured.
"""

from __future__ import annotations

import importlib.util
from dataclasses import replace
from math import pi, sqrt

import pytest

from parasolid_kit import NurbsSurface
from parasolid_kit.interop.occt import to_occt
from tests._occt_fixtures import make_nurbs_surface_model

pytestmark = pytest.mark.skipif(
    importlib.util.find_spec("OCP") is None, reason="OCP is not installed"
)

RADIUS, HEIGHT = 3.0, 5.0


def _rational_patch(arcs: int, *, closed: bool) -> NurbsSurface:
    """A cylinder wall of ``arcs`` quarter turns as a rational degree-2 x degree-1 surface."""
    corners = [(RADIUS, 0.0), (RADIUS, RADIUS), (0.0, RADIUS), (-RADIUS, RADIUS), (-RADIUS, 0.0)]
    corners += [(-RADIUS, -RADIUS), (0.0, -RADIUS), (RADIUS, -RADIUS), (RADIUS, 0.0)]
    weight = sqrt(0.5)
    poles = corners[: 2 * arcs + 1]
    weights = [1.0 if index % 2 == 0 else weight for index in range(len(poles))]
    control_vertices = []
    for (x, y), w in zip(poles, weights, strict=True):
        for z in (0.0, HEIGHT):
            control_vertices.append((x * w, y * w, z * w, w))
    return NurbsSurface(
        u_degree=2,
        v_degree=1,
        u_control_vertex_count=len(poles),
        v_control_vertex_count=2,
        vertex_dimension=4,
        u_knot_type=1,
        v_knot_type=1,
        u_periodic=False,
        v_periodic=False,
        u_closed=closed,
        v_closed=False,
        rational=True,
        surface_form=0,
        control_vertices=tuple(control_vertices),
        u_knots=tuple(float(k) for k in range(arcs + 1)),
        v_knots=(0.0, 1.0),
        u_knot_multiplicities=(3, *([2] * (arcs - 1)), 3),
        v_knot_multiplicities=(2, 2),
        sources=(),
    )


def _model(surface: NurbsSurface):
    model = make_nurbs_surface_model()
    basis = model.surfaces[0]
    return replace(
        model,
        surfaces=(replace(basis, definition=replace(surface, sources=basis.definition.sources)),),
    )


def test_a_rational_quarter_cylinder_patch_has_the_exact_area():
    result = to_occt(_model(_rational_patch(1, closed=False)), source_unit="mm")
    assert result.report.occt_valid
    assert result.report.metrics.surface_area == pytest.approx(pi / 2 * RADIUS * HEIGHT, rel=1e-9)


def test_a_closed_rational_cylinder_surface_converts_as_one_closed_face():
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps

    result = to_occt(_model(_rational_patch(4, closed=True)), source_unit="mm")
    assert result.report.occt_valid
    assert result.report.output_topology.to_dict()["faces"] == 1
    # The report's fixed Gauss rule integrates the four rational spans to a
    # tenth of a percent; the adaptive rule recovers the exact area.
    assert result.report.metrics.surface_area == pytest.approx(2 * pi * RADIUS * HEIGHT, rel=3e-3)
    properties = GProp_GProps()
    BRepGProp.SurfaceProperties_s(result.shape, properties, 1e-12)
    assert properties.Mass() == pytest.approx(2 * pi * RADIUS * HEIGHT, rel=1e-9)


def test_a_surface_flagged_closed_whose_ends_do_not_meet_is_rejected():
    from parasolid_kit.interop import OcctConversionError

    with pytest.raises(OcctConversionError, match="flagged closed"):
        to_occt(_model(_rational_patch(3, closed=True)), source_unit="mm")
