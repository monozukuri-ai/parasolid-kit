"""Closed and periodic source geometry on the OCCT path.

Periodic B-surfaces, spun surfaces that meet their axis at one end, pipes
around closed blend spines and FIN trims stored a period away from their
curve's range.
"""

from __future__ import annotations

from math import cos, pi, sin
from types import SimpleNamespace

import pytest

from parasolid_kit import BlendedEdgeSurface, NurbsSurface, SpunSurface
from parasolid_kit.brep.geometry import BlendType
from parasolid_kit.brep.topology import Vector3
from parasolid_kit.interop.occt import OcctConversionOptions

pytest.importorskip("OCP")


def periodic_ring(height: float = 5.0) -> NurbsSurface:
    """A uniform cubic ring in u, in the XT periodic form, by a line in v.

    Four distinct poles around the z axis; the stored form repeats the first
    three at the end and carries the degree's worth of knots on each side
    of the period [0, 4].
    """
    corners = [(4.0, 0.0), (0.0, 3.0), (-4.0, 0.0), (0.0, -3.0)]
    poles = [*corners, *corners[:3]]
    control_vertices = tuple((x, y, z) for x, y in poles for z in (0.0, height))
    return NurbsSurface(
        u_degree=3,
        v_degree=1,
        u_control_vertex_count=len(poles),
        v_control_vertex_count=2,
        vertex_dimension=3,
        u_knot_type=1,
        v_knot_type=1,
        u_periodic=True,
        v_periodic=False,
        u_closed=True,
        v_closed=False,
        rational=False,
        surface_form=0,
        control_vertices=control_vertices,
        u_knots=tuple(float(k) for k in range(-3, 8)),
        v_knots=(0.0, 1.0),
        u_knot_multiplicities=(1,) * 11,
        v_knot_multiplicities=(2, 2),
        sources=(),
    )


def test_a_periodic_nurbs_surface_is_built_periodic_and_repeats_its_period():
    from OCP.Geom import Geom_BSplineSurface
    from OCP.gp import gp_Pnt
    from OCP.TColgp import TColgp_Array2OfPnt
    from OCP.TColStd import TColStd_Array1OfInteger, TColStd_Array1OfReal

    from parasolid_kit.interop.occt.geometry import GeometryFactory

    definition = periodic_ring()
    surface = GeometryFactory(OcctConversionOptions(source_unit="mm")).nurbs_surface(definition)
    assert surface.IsUPeriodic()
    assert not surface.IsVPeriodic()
    assert surface.Bounds() == pytest.approx((0.0, 4.0, 0.0, 1.0))

    # The flat form OCCT would otherwise hold: equal inside the period, but
    # it continues the polynomial of its end spans beyond it.
    poles = TColgp_Array2OfPnt(1, 7, 1, 2)
    for index, vertex in enumerate(definition.control_vertices):
        poles.SetValue(index // 2 + 1, index % 2 + 1, gp_Pnt(*vertex))
    u_knots = TColStd_Array1OfReal(1, 11)
    u_mults = TColStd_Array1OfInteger(1, 11)
    for index in range(11):
        u_knots.SetValue(index + 1, float(index - 3))
        u_mults.SetValue(index + 1, 1)
    v_knots = TColStd_Array1OfReal(1, 2)
    v_knots.SetValue(1, 0.0)
    v_knots.SetValue(2, 1.0)
    v_mults = TColStd_Array1OfInteger(1, 2)
    v_mults.SetValue(1, 2)
    v_mults.SetValue(2, 2)
    flat = Geom_BSplineSurface(poles, u_knots, v_knots, u_mults, v_mults, 3, 1)
    for u in (0.0, 0.7, 1.9, 3.2, 4.0):
        for v in (0.0, 0.4, 1.0):
            assert surface.Value(u, v).Distance(flat.Value(u, v)) < 1e-12
    for u in (0.3, 2.5):
        assert surface.Value(u + 4.0, 0.5).Distance(flat.Value(u, 0.5)) < 1e-12
        assert surface.Value(u - 4.0, 0.5).Distance(flat.Value(u, 0.5)) < 1e-12
    assert flat.Value(4.3, 0.5).Distance(flat.Value(0.3, 0.5)) > 1e-3


def test_a_spun_surface_meeting_its_axis_at_one_end_revolves_from_that_point():
    from OCP.Geom import Geom_BSplineCurve
    from OCP.gp import gp_Pnt
    from OCP.TColgp import TColgp_Array1OfPnt
    from OCP.TColStd import TColStd_Array1OfInteger, TColStd_Array1OfReal

    from parasolid_kit.interop.occt.geometry import GeometryFactory

    poles = TColgp_Array1OfPnt(1, 2)
    poles.SetValue(1, gp_Pnt(0.0, 0.0, 2.0))
    poles.SetValue(2, gp_Pnt(3.0, 0.0, 0.0))
    knots = TColStd_Array1OfReal(1, 2)
    knots.SetValue(1, 0.25)
    knots.SetValue(2, 1.0)
    multiplicities = TColStd_Array1OfInteger(1, 2)
    multiplicities.SetValue(1, 2)
    multiplicities.SetValue(2, 2)
    profile = Geom_BSplineCurve(poles, knots, multiplicities, 1)
    factory = GeometryFactory(OcctConversionOptions(source_unit="mm"))
    resolve = {"resolve_basis": lambda _: None, "resolve_curve": lambda _: profile}
    base, axis = Vector3(0.0, 0.0, 0.0), Vector3(0.0, 0.0, 1.0)

    # The profile's first point lies on the spin axis; XT stores it as the
    # start with its parameter and leaves the end null.
    cap = SpunSurface(1, base, axis, Vector3(0.0, 0.0, 2.0), None, 0.25, None, None)
    surface = factory.surface3d(cap, **resolve)
    assert surface.Bounds()[2:] == pytest.approx((0.25, 1.0))
    for u in (0.0, 1.3, 4.0):
        assert surface.Value(u, 0.25).Coord() == pytest.approx((0.0, 0.0, 2.0), abs=1e-12)
        radius = 3.0
        expected = (radius * cos(u), -radius * sin(u), 0.0)
        assert surface.Value(u, 1.0).Coord() == pytest.approx(expected, abs=1e-12)

    # The same point recorded as the end of a reversed profile.
    reversed_profile = profile.Reversed()
    end_cap = SpunSurface(
        1, base, axis, None, Vector3(0.0, 0.0, 2.0), None, reversed_profile.LastParameter(), None
    )
    reversed_surface = factory.surface3d(
        end_cap, resolve_basis=lambda _: None, resolve_curve=lambda _: reversed_profile
    )
    assert reversed_surface.Bounds()[2:] == pytest.approx(
        (reversed_profile.FirstParameter(), reversed_profile.LastParameter())
    )

    # A one-sided record whose point is missing, off the axis or off the
    # profile still fails closed.
    for bad, message in (
        (SpunSurface(1, base, axis, None, None, 0.25, None, None), "one-sided"),
        (
            SpunSurface(1, base, axis, Vector3(3.0, 0.0, 0.0), None, 1.0, None, None),
            "one-sided",
        ),
        (
            SpunSurface(1, base, axis, Vector3(0.0, 0.0, 2.0), None, 0.5, None, None),
            "start point is not on the profile",
        ),
    ):
        with pytest.raises(ValueError, match=message):
            factory.surface3d(bad, **resolve)


def closed_spine():
    """A closed planar B-spline spine through twelve points of an ellipse."""
    from OCP.GeomAPI import GeomAPI_Interpolate
    from OCP.gp import gp_Pnt
    from OCP.TColgp import TColgp_HArray1OfPnt

    points = TColgp_HArray1OfPnt(1, 12)
    for index in range(12):
        angle = 2 * pi * index / 12
        points.SetValue(index + 1, gp_Pnt(30 * cos(angle), 20 * sin(angle), 5.0))
    fit = GeomAPI_Interpolate(points, True, 1e-9)
    fit.Perform()
    return fit.Curve()


def test_a_blend_around_a_closed_spine_is_a_periodic_pipe_like_a_torus():
    from OCP.GeomAPI import GeomAPI_ProjectPointOnCurve

    from parasolid_kit.interop.occt.geometry import GeometryFactory

    spine = closed_spine()
    blend = BlendedEdgeSurface(
        blend_type=BlendType.ROLLING_BALL,
        supporting_surfaces=(2, 3),
        spine_curve=4,
        ranges=(3.0, 3.0),
        thumb_weights=(1.0, 1.0),
        boundary_surfaces=(None, None),
        start=None,
        end=None,
    )
    factory = GeometryFactory(OcctConversionOptions(source_unit="mm"))
    surface = factory.blend_surface(blend, lambda _: spine)
    # Closed and periodic both ways, with the period of the circle along the
    # spine too: the rotation-minimising frame would return rotated and
    # leave the pipe open along a closed spine.
    assert surface.IsUPeriodic()
    assert surface.IsVPeriodic()
    u1, u2, v1, v2 = surface.Bounds()
    assert (u2 - u1, v2 - v1) == pytest.approx((2 * pi, 2 * pi))
    for u in (0.1, 2.0, 4.5):
        for v in (0.0, 1.7, 3.9, 6.1):
            point = surface.Value(u, v)
            assert GeomAPI_ProjectPointOnCurve(point, spine).LowerDistance() == pytest.approx(
                3.0, abs=1e-5
            )
        assert surface.Value(u, v2).Distance(surface.Value(u, v1)) < 1e-9


def test_a_fin_trim_a_period_away_from_its_closed_curve_is_wrapped_into_its_range():
    from OCP.Geom import Geom_CylindricalSurface
    from OCP.Geom2d import Geom2d_BSplineCurve
    from OCP.gp import gp_Ax3, gp_Dir, gp_Pnt, gp_Pnt2d
    from OCP.TColgp import TColgp_Array1OfPnt2d
    from OCP.TColStd import TColStd_Array1OfInteger, TColStd_Array1OfReal

    from parasolid_kit.interop.occt.intersection import SupportSurface
    from parasolid_kit.interop.occt.parametric import ParametricTopologyBuilder

    def line2d(start, end):
        poles = TColgp_Array1OfPnt2d(1, 2)
        poles.SetValue(1, gp_Pnt2d(*start))
        poles.SetValue(2, gp_Pnt2d(*end))
        knots = TColStd_Array1OfReal(1, 2)
        knots.SetValue(1, 0.0)
        knots.SetValue(2, 1.0)
        multiplicities = TColStd_Array1OfInteger(1, 2)
        multiplicities.SetValue(1, 2)
        multiplicities.SetValue(2, 2)
        return Geom2d_BSplineCurve(poles, knots, multiplicities, 1)

    cylinder = SupportSurface(
        Geom_CylindricalSurface(gp_Ax3(gp_Pnt(0.0, 0.0, 0.0), gp_Dir(0.0, 0.0, 1.0)), 2.0), 1e-9
    )
    builder = object.__new__(ParametricTopologyBuilder)
    builder.operations = []
    circle = line2d((0.0, 1.0), (2 * pi, 1.0))  # once around: its image closes
    arc = line2d((0.0, 1.0), (pi, 1.0))  # half way: its image is open

    def trim(start, end):
        return SimpleNamespace(start_parameter=start, end_parameter=end)

    # A ring FIN stored from 0 to -1 is the loop from 1 to 0: same direction.
    assert builder._wrapped_trim(trim(0.0, -1.0), circle, cylinder, 1e-6) == (1.0, 0.0)
    assert builder.operations == ["wrap_closed_fin_trim"]
    # A trim inside the range, and one on an open curve, keep their values.
    assert builder._wrapped_trim(trim(0.2, 0.7), circle, cylinder, 1e-6) == (0.2, 0.7)
    assert builder._wrapped_trim(trim(0.0, -1.0), arc, cylinder, 1e-6) == (0.0, -1.0)
    # A trim that crosses the end of the range stays, and its points beyond
    # the end are taken from the curve's start.
    assert builder._wrapped_trim(trim(0.9, 1.2), circle, cylinder, 1e-6) == (0.9, 1.2)
    period = builder._closed_period(circle, cylinder, 1e-6)
    assert period == pytest.approx(1.0)
    assert builder._closed_period(arc, cylinder, 1e-6) is None
    uv = builder._trim_uv(circle, 1.2, period)
    assert (uv.X(), uv.Y()) == pytest.approx((0.2 * 2 * pi, 1.0))
    uv = builder._trim_uv(circle, 0.6, period)
    assert (uv.X(), uv.Y()) == pytest.approx((0.6 * 2 * pi, 1.0))
