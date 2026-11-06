"""Support-surface evaluation, intersection walking and wire bodies of the OCCT path."""

from dataclasses import replace

import pytest

from parasolid_kit.brep.model import BrepMetrics
from parasolid_kit.brep.topology import BodyKind, RegionKind
from parasolid_kit.interop.occt import OcctConversionOptions, to_occt
from tests._occt_fixtures import make_box_model

pytest.importorskip("OCP")


def quadratic_patch():
    """A single-span biquadratic B-surface curved in u: its polynomial and linear
    continuations past u = 1 differ."""
    from OCP.Geom import Geom_BSplineSurface
    from OCP.gp import gp_Pnt
    from OCP.TColgp import TColgp_Array2OfPnt
    from OCP.TColStd import TColStd_Array1OfInteger, TColStd_Array1OfReal

    poles = TColgp_Array2OfPnt(1, 3, 1, 3)
    for i in range(3):
        for j in range(3):
            poles.SetValue(i + 1, j + 1, gp_Pnt(2.0 * i, 2.0 * j, 0.5 * (i - 1) ** 2 + 0.3 * i * j))
    knots = TColStd_Array1OfReal(1, 2)
    knots.SetValue(1, 0.0)
    knots.SetValue(2, 1.0)
    multiplicities = TColStd_Array1OfInteger(1, 2)
    multiplicities.SetValue(1, 3)
    multiplicities.SetValue(2, 3)
    return Geom_BSplineSurface(poles, knots, knots, multiplicities, multiplicities, 2, 2)


def test_support_surface_continues_a_bounded_b_surface_linearly():
    from OCP.Geom import Geom_OffsetSurface
    from OCP.gp import gp_Pnt, gp_Vec

    from parasolid_kit.interop.occt.intersection import SupportSurface

    basis = quadratic_patch()
    point, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
    basis.D1(1.0, 0.4, point, du, dv)
    # 0.3 past the u boundary along the boundary tangent: where the source
    # evaluates the surface; the polynomial continuation is elsewhere.
    beyond = gp_Pnt(point.XYZ() + du.XYZ() * 0.3)
    assert basis.Value(1.3, 0.4).Distance(beyond) > 1e-3
    support = SupportSurface(basis, 1e-12)
    assert support.value(1.3, 0.4).Distance(beyond) < 1e-12
    foot, normal, distance = support.project(beyond, 1e-9)
    assert distance < 1e-9 and foot.Distance(beyond) < 1e-9
    assert support.seed is not None and not support.inside(*support.seed)
    # A point of the surface proper projects onto itself.
    inside = basis.Value(0.5, 0.5)
    assert support.project(inside, 1e-9)[2] < 1e-12
    # An offset follows the normal of the extension.
    offset = SupportSurface(Geom_OffsetSurface(basis, 0.7, True), 1e-12)
    shifted = gp_Pnt(beyond.XYZ() + gp_Vec(*normal).XYZ() * 0.7)
    assert offset.project(shifted, 1e-9)[2] < 1e-9
    # A direction the source calls periodic is never extended.
    periodic = SupportSurface(basis, 1e-12, (True, False))
    assert periodic.inside(1.3, 0.4) and periodic.u_range is None


def test_march_intersection_walks_the_line_of_two_planes():
    from OCP.Geom import Geom_Plane
    from OCP.gp import gp_Dir, gp_Pnt

    from parasolid_kit.interop.occt.intersection import SupportSurface, march_intersection

    supports = [
        SupportSurface(Geom_Plane(gp_Pnt(), gp_Dir(0, 0, 1)), 1e-12),
        SupportSurface(Geom_Plane(gp_Pnt(), gp_Dir(0, 1, 0)), 1e-12),
    ]
    points = march_intersection(supports, gp_Pnt(0, 0, 0), (1.0, 0.0, 0.0), 5.0, 1.0, 1e-9)
    assert len(points) == 5
    assert points[-1].Distance(gp_Pnt(5, 0, 0)) < 1e-9
    assert all(abs(p.Y()) < 1e-9 and abs(p.Z()) < 1e-9 for p in points)
    # A seed off the line is pulled back onto it.
    skew = march_intersection(supports, gp_Pnt(0, 0.2, -0.1), (1.0, 0.0, 0.0), 2.0, 1.0, 1e-9)
    assert all(abs(p.Y()) < 1e-9 and abs(p.Z()) < 1e-9 for p in skew)


def test_interpolation_parameters_are_the_knots():
    from OCP.GeomAPI import GeomAPI_Interpolate
    from OCP.gp import gp_Pnt
    from OCP.TColgp import TColgp_HArray1OfPnt

    from parasolid_kit.interop.occt.intersection import _interpolation_parameters, project_curve

    points = [gp_Pnt(i, (i % 3) * 0.4, 0.1 * i * i) for i in range(6)]
    array = TColgp_HArray1OfPnt(1, len(points))
    for index, point in enumerate(points, 1):
        array.SetValue(index, point)
    fit = GeomAPI_Interpolate(array, False, 1e-9)
    fit.Perform()
    curve = fit.Curve()
    parameters = _interpolation_parameters(curve, points, False, 1e-9)
    assert parameters == pytest.approx([project_curve(curve, p)[0] for p in points], abs=1e-9)


def test_fin_deviation_cap_is_validated_and_reported():
    assert OcctConversionOptions(source_unit="mm").to_dict()["fin_deviation_cap"] is None
    options = OcctConversionOptions(source_unit="mm", fin_deviation_cap=0.5)
    assert options.to_dict()["fin_deviation_cap"] == 0.5
    for bad in (-1.0, float("nan"), True, "0.1"):
        with pytest.raises(ValueError, match="fin_deviation_cap"):
            OcctConversionOptions(source_unit="mm", fin_deviation_cap=bad)


def test_wire_body_converts_to_its_edges():
    from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_WIRE
    from OCP.TopExp import TopExp_Explorer

    model = make_box_model(4.0, 3.0, 2.0)
    body = model.bodies[0]
    region = next(r for r in model.regions if r.id == body.regions[0])
    shell = next(s for s in model.shells if s.id == region.shells[0])
    # The box's twelve edges as a wire body: on the shell of one void region,
    # no faces, no loops.
    wire = replace(
        model,
        bodies=(replace(body, kind=BodyKind.WIRE, regions=(region.id,)),),
        regions=(replace(region, kind=RegionKind.VOID, shells=(shell.id,)),),
        shells=(
            replace(
                shell,
                back_faces=(),
                front_faces=(),
                wire_edges=tuple(edge.id for edge in model.edges),
            ),
        ),
        faces=(),
        loops=(),
        half_edges=(),
        edges=tuple(replace(edge, half_edges=()) for edge in model.edges),
        # A wire body has no area or volume to compare with.
        metrics=BrepMetrics(model.metrics.bounding_box, None, None),
    )
    result = to_occt(wire, source_unit="mm")

    def count(kind):
        explorer = TopExp_Explorer(result.shape, kind)
        total = 0
        while explorer.More():
            total += 1
            explorer.Next()
        return total

    assert count(TopAbs_FACE) == 0
    assert count(TopAbs_EDGE) == 12
    assert count(TopAbs_WIRE) >= 1
    assert "wire_body_from_edges" in result.report.topology_operations
