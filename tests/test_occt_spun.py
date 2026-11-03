"""Spun surfaces: the source's (profile parameter, angle) parameters map onto OCCT's revolution."""

from __future__ import annotations

import importlib.util
from math import cos, pi, sin

import pytest

from parasolid_kit import BrepMetrics, CurveKind, Sense, SurfaceKind, Vector3
from parasolid_kit.brep import (
    Body,
    BodyKind,
    BoundingBox,
    BrepModel,
    CircleCurve,
    CurveGeometry,
    Edge,
    Face,
    HalfEdge,
    LineCurve,
    Loop,
    NurbsCurve,
    PlaneSurface,
    Region,
    RegionKind,
    Shell,
    SpunSurface,
    SurfaceGeometry,
    TopologyValidation,
)
from parasolid_kit.interop.occt import to_occt
from tests._occt_fixtures import _Sources

pytestmark = pytest.mark.skipif(
    importlib.util.find_spec("OCP") is None, reason="OCP is not installed"
)


def test_parameter_curves_on_a_spun_surface_follow_the_source_formula():
    """A 2D line in (u, v) evaluates to R = Z(u) + (C(u) - Z(u)) cos v + A x (C(u) - Z(u)) sin v."""
    from parasolid_kit.interop.occt.geometry import GeometryFactory
    from parasolid_kit.interop.occt.options import OcctConversionOptions

    factory = GeometryFactory(OcctConversionOptions(source_unit="mm", target_unit="mm"))
    radius = 2.0
    profile = LineCurve(Vector3(radius, 0.0, 0.0), Vector3(0.0, 0.0, 1.0))
    spun = SpunSurface(
        1, Vector3(0.0, 0.0, 0.0), Vector3(0.0, 0.0, 1.0), None, None, None, None, None
    )
    surface = factory.surface3d(
        spun,
        resolve_basis=lambda _i: None,
        resolve_curve=lambda _i: factory.curve3d(profile, resolve_basis=lambda _j: None),
    )
    # From (u, v) = (0.5, 0.3) to (1.5, 1.0): a length along the profile and an angle.
    line = NurbsCurve(
        1, 2, 2, 0, False, False, False, 0, ((0.5, 0.3), (1.5, 1.0)), (0.0, 1.0), (2, 2), ()
    )
    pcurve = factory.parameter_curve(line, spun, section=profile)
    for t in (0.0, 0.25, 0.5, 0.75, 1.0):
        uv = pcurve.Value(t)
        point = surface.Value(uv.X(), uv.Y())
        u, v = 0.5 + t, 0.3 + 0.7 * t
        expected = (radius * cos(v), radius * sin(v), u)
        assert point.Distance(factory.point(Vector3(*expected))) < 1e-9


def make_spun_cylinder_model(radius: float = 1.0, height: float = 2.0) -> BrepModel:
    """A cylinder whose wall is a spun surface: a line parallel to z revolved about z."""
    sources = _Sources()
    body_source = sources.next("body")
    region_source = sources.next("region")
    shell_source = sources.next("shell")
    z_axis, x_axis = Vector3(0.0, 0.0, 1.0), Vector3(1.0, 0.0, 0.0)
    edge_sources = {1: sources.next("edge"), 2: sources.next("edge")}

    def circle(z: float) -> CircleCurve:
        return CircleCurve(Vector3(0.0, 0.0, z), z_axis, x_axis, radius)

    curves = (
        CurveGeometry(
            1,
            Sense.POSITIVE,
            edge_sources[1],
            CurveKind.CIRCLE,
            circle(0.0),
            sources.next("circle"),
        ),
        CurveGeometry(
            2,
            Sense.POSITIVE,
            edge_sources[2],
            CurveKind.CIRCLE,
            circle(height),
            sources.next("circle"),
        ),
        CurveGeometry(
            3,
            Sense.POSITIVE,
            None,
            CurveKind.LINE,
            LineCurve(Vector3(radius, 0.0, 0.0), z_axis),
            sources.next("line"),
        ),
    )
    edges = (
        Edge(1, body_source, (1, 4), None, None, 1, None, edge_sources[1]),
        Edge(2, body_source, (2, 3), None, None, 2, None, edge_sources[2]),
    )
    half_edge_sources = {i: sources.next("fin") for i in range(1, 5)}
    # The spun surface's own normal, dR/du x dR/dv with u along the profile
    # and v the angle, points inward, so the wall is a negative-sense face.
    half_edges = (
        HalfEdge(1, 1, 1, 1, None, 4, 1, 1, Sense.NEGATIVE, False, half_edge_sources[1]),
        HalfEdge(2, 2, 2, 2, None, 3, 2, 2, Sense.POSITIVE, False, half_edge_sources[2]),
        HalfEdge(3, 3, 3, 3, None, 2, 2, 2, Sense.POSITIVE, False, half_edge_sources[3]),
        HalfEdge(4, 4, 4, 4, None, 1, 1, 1, Sense.NEGATIVE, False, half_edge_sources[4]),
    )
    loop_sources = {i: sources.next("loop") for i in range(1, 5)}
    loops = (
        Loop(1, 1, (1,), loop_sources[1]),
        Loop(2, 1, (2,), loop_sources[2]),
        Loop(3, 2, (3,), loop_sources[3]),
        Loop(4, 3, (4,), loop_sources[4]),
    )
    face_sources = {i: sources.next("face") for i in (1, 2, 3)}
    surfaces = (
        SurfaceGeometry(
            1,
            Sense.POSITIVE,
            face_sources[1],
            SurfaceKind.SPUN,
            SpunSurface(3, Vector3(0.0, 0.0, 0.0), z_axis, None, None, None, None, x_axis),
            sources.next("spun"),
        ),
        SurfaceGeometry(
            2,
            Sense.POSITIVE,
            face_sources[2],
            SurfaceKind.PLANE,
            PlaneSurface(Vector3(0.0, 0.0, height), z_axis, x_axis),
            sources.next("plane"),
        ),
        SurfaceGeometry(
            3,
            Sense.POSITIVE,
            face_sources[3],
            SurfaceKind.PLANE,
            PlaneSurface(Vector3(0.0, 0.0, 0.0), Vector3(0.0, 0.0, -1.0), x_axis),
            sources.next("plane"),
        ),
    )
    faces = (
        Face(1, 1, 1, (1, 2), 1, Sense.NEGATIVE, face_sources[1]),
        Face(2, 1, 1, (3,), 2, Sense.POSITIVE, face_sources[2]),
        Face(3, 1, 1, (4,), 3, Sense.POSITIVE, face_sources[3]),
    )
    return BrepModel(
        source_format="binary",
        schema_key="synthetic-spun-cylinder",
        complete=True,
        bodies=(Body(1, BodyKind.SOLID, 1.0e-6, 1.0e-8, (1,), (1, 2), (), body_source),),
        regions=(Region(1, RegionKind.SOLID, 1, (1,), region_source),),
        shells=(Shell(1, 1, (), (1, 2, 3), (), None, shell_source),),
        faces=faces,
        loops=loops,
        half_edges=half_edges,
        edges=edges,
        vertices=(),
        points=(),
        curves=curves,
        surfaces=surfaces,
        topology=TopologyValidation(True, 4, 2, 1),
        metrics=BrepMetrics(
            BoundingBox(Vector3(-radius, -radius, 0.0), Vector3(radius, radius, height)),
            2.0 * pi * radius * height + 2.0 * pi * radius**2,
            pi * radius**2 * height,
        ),
        diagnostics=(),
    )


def test_a_spun_line_wall_converts_to_the_cylinder_it_describes():
    radius, height = 1.5, 4.0
    result = to_occt(make_spun_cylinder_model(radius, height), source_unit="mm")
    report = result.report
    assert report.occt_valid and report.conversion_complete
    assert report.metrics.volume == pytest.approx(pi * radius**2 * height, rel=1e-9)
    assert report.metrics.surface_area == pytest.approx(
        2 * pi * radius * height + 2 * pi * radius**2, rel=1e-9
    )
    assert report.input_surface_kinds.to_dict().get("spun") == 1
