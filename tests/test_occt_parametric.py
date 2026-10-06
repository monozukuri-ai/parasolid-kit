"""Analytic oracles for the bounded UV/intersection conversion path."""

from dataclasses import replace
from itertools import pairwise
from math import cos, pi, sin

import pytest

from parasolid_kit import (
    CurveGeometry,
    CurveKind,
    NurbsCurve,
    SurfaceParametricCurve,
    TrimmedCurve,
    Vector3,
)
from parasolid_kit.interop import OcctConversionError
from parasolid_kit.interop.occt import OcctConversionOptions, SourceEntityKind, to_occt
from parasolid_kit.interop.occt.geometry import GeometryFactory
from tests._occt_fixtures import (
    _Sources,
    make_box_model,
    make_sphere_octant_model,
    make_torus_elbow_model,
)

pytest.importorskip("OCP")


def uv_box(*, displacement=0.0):
    """Replace one box edge with independent UV curves on its two plane faces."""
    model = make_box_model(4.0, 3.0, 2.0)
    edge = model.edges[0]
    faces = {f.id: f for f in model.faces}
    loops = {p.id: p for p in model.loops}
    surfaces = {s.id: s for s in model.surfaces}
    points = {p.id: p.position for p in model.points}
    vertices = {v.id: v for v in model.vertices}
    endpoints = [points[vertices[i].point] for i in (edge.start_vertex, edge.end_vertex)]
    half_edges = []
    curves = list(model.curves)
    sources = _Sources(10000)
    new_id = 100
    for half_edge in model.half_edges:
        if half_edge.id not in edge.half_edges:
            half_edges.append(half_edge)
            continue
        surface = surfaces[faces[loops[half_edge.loop].face].surface]
        definition = surface.definition
        x, n = tuple(definition.x_axis), tuple(definition.normal)
        y = (n[1] * x[2] - n[2] * x[1], n[2] * x[0] - n[0] * x[2], n[0] * x[1] - n[1] * x[0])
        poles = []
        for point in endpoints:
            delta = [p - q for p, q in zip(point, definition.point, strict=True)]
            poles.append(
                (
                    sum(a * b for a, b in zip(delta, x, strict=True)),
                    sum(a * b for a, b in zip(delta, y, strict=True)),
                )
            )
        # Degree 2 with a displaced middle pole preserves both source endpoints.
        middle = tuple((a + b) / 2 for a, b in zip(*poles, strict=True))
        if new_id > 100:
            middle = (middle[0], middle[1] + displacement)
        uv = NurbsCurve(
            2, 3, 2, 0, False, False, False, 0, (poles[0], middle, poles[1]), (0.0, 1.0), (3, 3), ()
        )
        definitions = [
            (CurveKind.NURBS, uv),
            (CurveKind.SURFACE_PARAMETRIC, SurfaceParametricCurve(surface.id, new_id, None, None)),
            (CurveKind.TRIMMED, TrimmedCurve(new_id + 1, *endpoints, 0.0, 1.0)),
        ]
        for offset, (kind, d) in enumerate(definitions):
            curves.append(
                CurveGeometry(
                    id=new_id + offset,
                    kind=kind,
                    definition=d,
                    owner=None,
                    sense=Sense.POSITIVE,
                    source=sources.next(kind.value),
                )
            )
        half_edges.append(replace(half_edge, curve=new_id + 2))
        new_id += 3
    return replace(
        model,
        curves=tuple(curves),
        half_edges=tuple(half_edges),
        edges=(replace(edge, curve=None, tolerance=1e-7), *model.edges[1:]),
    )


from parasolid_kit import Sense  # noqa: E402


@pytest.mark.parametrize("source_unit,scale", [("mm", 1.0), ("cm", 10.0)])
def test_dual_fin_box_is_valid_and_keeps_source_mapping(source_unit, scale):
    model = uv_box()
    result = to_occt(model, source_unit=source_unit)
    assert result.report.occt_valid
    assert result.report.metrics.volume == pytest.approx(24 * scale**3)
    assert result.report.metrics.surface_area == pytest.approx(52 * scale**2)
    assert result.report.output_topology.to_dict()["edges"] == 12
    mapped = {
        r.source.entity_id
        for r in result.source_map.relations
        if r.source.kind is SourceEntityKind.CURVE
    }
    assert set(range(100, 106)) <= mapped
    assert "surface_parametric_curve_3d_approximation" in result.report.topology_operations


def test_incompatible_fins_fail_without_allowing_partial_conversion():
    with pytest.raises(OcctConversionError):
        to_occt(uv_box(displacement=0.5), source_unit="mm", require_complete=False)


def test_parameter_units_and_lemon_torus_follow_source_formulas():
    from parasolid_kit import CylinderSurface, TorusSurface

    factory = GeometryFactory(OcctConversionOptions(source_unit="cm", target_unit="mm"))
    uv = NurbsCurve(
        1, 2, 2, 0, False, False, False, 0, ((0.2, 1.0), (0.6, 3.0)), (0.0, 1.0), (2, 2), ()
    )
    cylinder = CylinderSurface(
        Vector3(0.0, 0.0, 0.0), Vector3(0.0, 0.0, 1.0), 2.0, Vector3(1.0, 0.0, 0.0)
    )
    pc = factory.parameter_curve(uv, cylinder)
    assert pc.Value(0.5).Coord() == pytest.approx((0.4, 20.0))
    torus = TorusSurface(
        Vector3(1.0, 2.0, 3.0), Vector3(0.0, 0.0, 1.0), -2.0, 3.0, Vector3(1.0, 0.0, 0.0)
    )
    surface = factory.lemon_torus(torus)
    for u, v in ((0.1, 0.2), (1.3, -0.5), (2.0, 0.7)):
        expected = (
            10 + (-20 + 30 * cos(v)) * cos(u),
            20 + (-20 + 30 * cos(v)) * sin(u),
            30 + 30 * sin(v),
        )
        assert surface.Value(u, v).Coord() == pytest.approx(expected, abs=1e-12)


def test_numerical_intersection_matches_independent_sphere_plane_circle():
    from OCP.Geom import Geom_Plane, Geom_SphericalSurface
    from OCP.gp import gp_Ax3, gp_Dir, gp_Pnt

    from parasolid_kit.interop.occt.intersection import fit_intersection

    surfaces = [Geom_SphericalSurface(gp_Ax3(), 2.0), Geom_Plane(gp_Pnt(), gp_Dir(0, 0, 1))]
    points = [gp_Pnt(2 * cos(a), 2 * sin(a), 0) for a in (0, 0.2, 0.5, 0.8)]
    curve = fit_intersection(surfaces, points, [points[0], points[-1]], 1e-7)
    for i in range(137):
        point = curve.Value(
            curve.FirstParameter() + (curve.LastParameter() - curve.FirstParameter()) * i / 136
        )
        assert abs((point.X() ** 2 + point.Y() ** 2) ** 0.5 - 2.0) < 2e-7
        assert abs(point.Z()) < 1e-12
    with pytest.raises(ValueError, match="support-surface tolerance"):
        fit_intersection(surfaces, [gp_Pnt(0.5, 0.5, 0.1), *points], [], 1e-7)


def test_vertex_bounds_are_contained_for_curves_but_remain_strict_for_polygons():
    from parasolid_kit.interop.occt.model import OcctMetrics
    from parasolid_kit.interop.occt.validation import metric_diagnostics

    model = uv_box()
    options = OcctConversionOptions(source_unit="mm")
    metrics = OcctMetrics((-1.0, -1.0, -1.0, 5.0, 4.0, 3.0), 52.0, 24.0)
    assert not metric_diagnostics(model, metrics, options)
    assert metric_diagnostics(make_box_model(4, 3, 2), metrics, options)


def test_material_region_preserves_inner_cavity_as_a_separate_shell():
    outer = uv_box()
    inner = make_box_model(
        1.0, 0.8, 0.6, _origin=(1.0, 0.7, 0.5), _id_offset=1000, _source_offset=20000
    )
    body = replace(
        outer.bodies[0],
        edges=(*outer.bodies[0].edges, *inner.bodies[0].edges),
        vertices=(*outer.bodies[0].vertices, *inner.bodies[0].vertices),
    )
    region = replace(outer.regions[0], shells=(*outer.regions[0].shells, *inner.regions[0].shells))
    model = replace(
        outer,
        bodies=(body,),
        regions=(region,),
        shells=(*outer.shells, *(replace(s, region=region.id) for s in inner.shells)),
        faces=(*outer.faces, *(replace(f, sense=Sense.NEGATIVE) for f in inner.faces)),
        loops=(*outer.loops, *inner.loops),
        half_edges=(*outer.half_edges, *inner.half_edges),
        edges=(*outer.edges, *inner.edges),
        vertices=(*outer.vertices, *inner.vertices),
        points=(*outer.points, *inner.points),
        curves=(*outer.curves, *inner.curves),
        surfaces=(*outer.surfaces, *inner.surfaces),
        metrics=replace(
            outer.metrics, volume=24.0 - 0.48, surface_area=52.0 + 2 * (0.8 + 0.48 + 0.6)
        ),
    )
    result = to_occt(model, source_unit="mm")
    assert result.report.occt_valid
    assert result.report.output_topology.to_dict()["shells"] == 2
    assert result.report.output_topology.to_dict()["solids"] == 1
    assert result.report.metrics.volume == pytest.approx(23.52)


def test_bounded_approximation_is_visible_in_conversion_and_preview_reports():
    from parasolid_kit.interop.preview import tessellate_preview

    model = uv_box(displacement=3e-7)
    model = replace(model, metrics=replace(model.metrics, surface_area=None, volume=None))
    converted = to_occt(model, source_unit="mm")
    diagnostics = [
        d for d in converted.report.diagnostics if d.code == "occt.parametric_approximation"
    ]
    assert len(diagnostics) == 1
    assert not diagnostics[0].fatal
    preview = tessellate_preview(converted, model)
    assert preview.manifest["diagnostics"][0]["code"] == "occt.parametric_approximation"
    assert preview.manifest["conversion"]["source_unit"] == "mm"
    assert (
        "surface_parametric_curve_3d_approximation"
        in preview.manifest["conversion"]["topology_operations"]
    )
    assert preview.missing_face_count == preview.missing_edge_count == 0


def test_opposed_periodic_face_keeps_its_source_region():
    from OCP.BRepCheck import BRepCheck_Analyzer
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps

    from parasolid_kit.interop.occt.parametric import ParametricTopologyBuilder

    # The elbow's inner wall opposes its torus; its two end circles bound the
    # quarter turn, not the complementary three quarters.
    major, outer, inner = 8.0, 6.75, 5.5
    model = make_torus_elbow_model(major, outer, inner)
    factory = GeometryFactory(OcctConversionOptions(source_unit="mm"))
    built = ParametricTopologyBuilder(model, factory).build()
    assert BRepCheck_Analyzer(built.shape).IsValid()
    assert "reverse_opposed_face_loops" in built.operations
    volume, area = GProp_GProps(), GProp_GProps()
    BRepGProp.VolumeProperties_s(built.shape, volume)
    BRepGProp.SurfaceProperties_s(built.shape, area)
    quarter = pi / 2
    assert volume.Mass() == pytest.approx(pi * (outer**2 - inner**2) * major * quarter)
    assert area.Mass() == pytest.approx(
        2 * pi * major * quarter * (outer + inner) + 2 * pi * (outer**2 - inner**2)
    )


def test_closed_intersection_branch_becomes_a_periodic_curve():
    from OCP.Geom import Geom_CylindricalSurface
    from OCP.gp import gp_Ax3, gp_Dir, gp_Pnt

    from parasolid_kit.interop.occt.intersection import fit_intersection

    # A unit cylinder on Z pierces a radius-2 cylinder on X: one closed branch.
    surfaces = [
        Geom_CylindricalSurface(gp_Ax3(), 1.0),
        Geom_CylindricalSurface(gp_Ax3(gp_Pnt(), gp_Dir(1, 0, 0), gp_Dir(0, 1, 0)), 2.0),
    ]
    angles = [2 * pi * i / 24 for i in range(24)]
    points = [gp_Pnt(cos(a), sin(a), (4 - sin(a) ** 2) ** 0.5) for a in angles]
    curve = fit_intersection(surfaces, [*points, points[0]], [], 1e-7)
    assert curve.IsPeriodic()
    first, last = curve.FirstParameter(), curve.LastParameter()
    for i in range(241):
        x, y, z = curve.Value(first + (last - first) * i / 240).Coord()
        assert abs((x * x + y * y) ** 0.5 - 1.0) < 2e-7
        assert abs((y * y + z * z) ** 0.5 - 2.0) < 2e-7
    with pytest.raises(ValueError, match="fewer than two distinct points"):
        fit_intersection(surfaces, [points[0], points[0]], [], 1e-7)


@pytest.mark.parametrize("major,minor", [(5.0, 5.0), (3.0, 5.0)])
def test_horn_and_apple_tori_are_exact_only_for_trimmed_faces(major, minor):
    from parasolid_kit import TorusSurface

    factory = GeometryFactory(OcctConversionOptions(source_unit="mm"))
    torus = TorusSurface(
        Vector3(1.0, 2.0, 3.0), Vector3(0.0, 0.0, 1.0), major, minor, Vector3(1.0, 0.0, 0.0)
    )
    surface = factory.surface3d(torus, resolve_basis=lambda _: None)
    for u, v in ((0.1, 0.2), (1.3, -0.5), (2.0, 0.7)):
        expected = (
            1 + (major + minor * cos(v)) * cos(u),
            2 + (major + minor * cos(v)) * sin(u),
            3 + minor * sin(v),
        )
        assert surface.Value(u, v).Coord() == pytest.approx(expected, abs=1e-12)
    with pytest.raises(ValueError, match="ring torus"):
        factory.torus(torus)


def test_loop_through_a_sphere_pole_gets_its_degenerated_edge():
    from OCP.BRepCheck import BRepCheck_Analyzer
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps

    from parasolid_kit.interop.occt.parametric import ParametricTopologyBuilder

    radius = 2.0
    factory = GeometryFactory(OcctConversionOptions(source_unit="mm"))
    built = ParametricTopologyBuilder(make_sphere_octant_model(radius), factory).build()
    assert BRepCheck_Analyzer(built.shape).IsValid()
    assert "insert_degenerated_singularity_edges" in built.operations
    volume, area = GProp_GProps(), GProp_GProps()
    BRepGProp.VolumeProperties_s(built.shape, volume)
    BRepGProp.SurfaceProperties_s(built.shape, area)
    assert volume.Mass() == pytest.approx(pi * radius**3 / 6)
    assert area.Mass() == pytest.approx(5 * pi * radius**2 / 4)


@pytest.mark.parametrize("cut", ["z", "x"])
def test_closed_analytic_branch_follows_the_source_order_across_its_origin(cut):
    from OCP.Geom import Geom_Circle, Geom_Plane, Geom_SurfaceOfRevolution, Geom_TrimmedCurve
    from OCP.gp import gp_Ax1, gp_Ax2, gp_Ax3, gp_Dir, gp_Pnt

    from parasolid_kit.interop.occt.intersection import intersect_analytic, project_curve

    # A unit sphere as a revolved half circle meets the plane z = 0.3, or the
    # plane x = 0.3 through its seam, in a closed branch that OCCT may return
    # in pieces. The source points run clockwise and cross the parameter
    # origin of that branch.
    meridian = Geom_TrimmedCurve(
        Geom_Circle(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 1, 0), gp_Dir(0, 0, 1)), 1.0), 0.0, pi
    )
    sphere = Geom_SurfaceOfRevolution(meridian, gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)))
    radius = (1 - 0.3**2) ** 0.5
    angles = [0.5, 0.2, -0.1, -0.4, -0.7, -1.0]
    if cut == "z":
        plane = Geom_Plane(gp_Ax3(gp_Pnt(0, 0, 0.3), gp_Dir(0, 0, 1)))
        samples = [gp_Pnt(radius * cos(a), radius * sin(a), 0.3) for a in angles]
    else:
        plane = Geom_Plane(gp_Ax3(gp_Pnt(0.3, 0, 0), gp_Dir(1, 0, 0)))
        samples = [gp_Pnt(0.3, radius * sin(a), radius * cos(a)) for a in angles]
    curve = intersect_analytic([plane, sphere], samples, 1e-7)
    assert curve.IsPeriodic()
    parameters = [project_curve(curve, p)[0] for p in samples]
    period = curve.Period()
    unwrapped = [parameters[0]]
    for value in parameters[1:]:
        step = value - unwrapped[-1]
        unwrapped.append(unwrapped[-1] + (step + period / 2) % period - period / 2)
    assert all(b > a for a, b in pairwise(unwrapped))
    for point in samples:
        assert project_curve(curve, point)[1] < 1e-6


def test_unbounded_analytic_branch_keeps_the_source_order():
    from OCP.Geom import Geom_ConicalSurface, Geom_Plane
    from OCP.gp import gp_Ax3, gp_Dir, gp_Pnt

    from parasolid_kit.interop.occt.intersection import intersect_analytic, project_curve

    # A plane parallel to the axis of a cone cuts it in a hyperbola, whose
    # OCCT parameter range is unbounded. The source points run against the
    # branch parameter and must simply be reversed.
    cone = Geom_ConicalSurface(gp_Ax3(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), pi / 4, 1.0)
    plane = Geom_Plane(gp_Ax3(gp_Pnt(0.5, 0, 0), gp_Dir(1, 0, 0)))
    samples = []
    for z in (2.0, 1.5, 1.0, 0.5, 0.0):
        radius = 1.0 + z
        samples.append(gp_Pnt(0.5, (radius**2 - 0.25) ** 0.5, z))
    curve = intersect_analytic([plane, cone], samples, 1e-7)
    assert not curve.IsClosed()
    parameters = [project_curve(curve, p)[0] for p in samples]
    assert all(b > a for a, b in pairwise(parameters))
    for point in samples:
        assert project_curve(curve, point)[1] < 1e-6


def test_rolling_ball_blend_is_the_pipe_of_its_radius_around_the_spine():
    from OCP.Geom import Geom_Circle, Geom_Line
    from OCP.GeomAPI import GeomAPI_ProjectPointOnCurve
    from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt

    from parasolid_kit import BlendedEdgeSurface, BlendType

    factory = GeometryFactory(OcctConversionOptions(source_unit="mm"))
    resolve = {"resolve_basis": lambda _: None}
    # Around a straight spine the blend is a cylinder of the blend radius; the
    # two offsets may carry opposite signs.
    line = Geom_Line(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1))
    blend = BlendedEdgeSurface(
        BlendType.ROLLING_BALL, (1, 2), 7, (2.0, -2.0), (1.0, 1.0), (None, None), None, None
    )
    surface = factory.surface3d(blend, resolve_curve=lambda _: line, **resolve)
    u1, u2, v1, v2 = surface.Bounds()
    for u, v in ((u1, v1), (0.3, 0.7), (u2, v2)):
        point = surface.Value(u, v)
        assert point.X() ** 2 + point.Y() ** 2 == pytest.approx(4.0, abs=1e-9)
    # The circular section is periodic, so seam fixes can place its seam.
    assert surface.IsUPeriodic() or surface.IsVPeriodic()
    # Around a circular spine every surface point is at the blend radius.
    circle = Geom_Circle(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1), gp_Dir(1, 0, 0)), 5.0)
    torus = factory.surface3d(
        replace_blend(blend, ranges=(0.5, 0.5)), resolve_curve=lambda _: circle, **resolve
    )
    assert torus.IsUPeriodic() and torus.IsVPeriodic()
    u1, u2, v1, v2 = torus.Bounds()
    for i in range(5):
        for j in range(5):
            point = torus.Value(u1 + (u2 - u1) * i / 4, v1 + (v2 - v1) * j / 4)
            assert GeomAPI_ProjectPointOnCurve(point, circle).LowerDistance() == pytest.approx(
                0.5, abs=1e-6
            )
    for bad, message in (
        (replace_blend(blend, blend_type=BlendType.CLIFF_EDGE), "cliff-edge"),
        (replace_blend(blend, ranges=(2.0, 1.0)), "equal magnitude"),
        (replace_blend(blend, ranges=(0.0, 0.0)), "non-zero"),
        (replace_blend(blend, thumb_weights=(1.0, 0.5)), "thumb weights"),
    ):
        with pytest.raises(ValueError, match=message):
            factory.surface3d(bad, resolve_curve=lambda _: line, **resolve)
    with pytest.raises(ValueError, match="spine curve resolver"):
        factory.surface3d(blend, **resolve)


def replace_blend(blend, **changes):
    from dataclasses import replace

    return replace(blend, **changes)


def test_spun_surface_revolves_its_profile_about_the_spin_axis():
    from OCP.Geom import Geom_Circle
    from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt, gp_Vec

    from parasolid_kit import SpunSurface

    factory = GeometryFactory(OcctConversionOptions(source_unit="mm"))
    # A unit circle in the xz plane centred 3 mm from the z axis: a ring torus.
    circle = Geom_Circle(gp_Ax2(gp_Pnt(3, 0, 0), gp_Dir(0, 1, 0), gp_Dir(1, 0, 0)), 1.0)
    spun = SpunSurface(
        1, Vector3(0.0, 0.0, 0.0), Vector3(0.0, 0.0, 1.0), None, None, None, None, None
    )
    surface = factory.surface3d(spun, resolve_basis=lambda _: None, resolve_curve=lambda _: circle)
    # The source normal is d/d(profile) x d/d(angle): outward on this torus.
    # OCCT orders the parameters (angle, profile), so the axis is reversed and
    # the angle runs clockwise about +z; the normal then agrees with the source.
    point, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
    for u, v in ((0.0, 0.0), (0.7, 1.1), (2.5, -0.4), (4.0, 2.9)):
        radius = 3 + cos(v)
        expected = (radius * cos(u), -radius * sin(u), -sin(v))
        assert surface.Value(u, v).Coord() == pytest.approx(expected, abs=1e-12)
        surface.D1(u, v, point, du, dv)
        normal = du.Crossed(dv).Normalized()
        assert normal.Coord() == pytest.approx(
            (cos(v) * cos(u), -cos(v) * sin(u), -sin(v)), abs=1e-12
        )
    with pytest.raises(ValueError, match="profile curve resolver"):
        factory.surface3d(spun, resolve_basis=lambda _: None)
    # Stored profile parameters trim the profile: the half circle from (4, 0, 0)
    # at 0 to (2, 0, 0) at pi is revolved once, and the stored end points must
    # lie on the profile at those parameters.
    resolve = {"resolve_basis": lambda _: None, "resolve_curve": lambda _: circle}
    half = SpunSurface(
        1,
        Vector3(0.0, 0.0, 0.0),
        Vector3(0.0, 0.0, 1.0),
        Vector3(4.0, 0.0, 0.0),
        Vector3(2.0, 0.0, 0.0),
        0.0,
        pi,
        None,
    )
    trimmed = factory.surface3d(half, **resolve)
    assert trimmed.Bounds()[2:] == pytest.approx((0.0, pi))
    assert trimmed.Value(1.0, pi).Coord() == pytest.approx((2 * cos(1.0), -2 * sin(1.0), 0.0))
    reversed_ = SpunSurface(1, half.base, half.axis, half.end, half.start, pi, 0.0, None)
    assert factory.surface3d(reversed_, **resolve).Bounds()[2:] == pytest.approx((pi, 2 * pi))
    for bad, message in (
        (SpunSurface(1, half.base, half.axis, None, None, 0.0, None, None), "one-sided"),
        (SpunSurface(1, half.base, half.axis, None, None, 1.0, 1.0, None), "finite and distinct"),
        (
            SpunSurface(1, half.base, half.axis, Vector3(4.0, 0.0, 0.5), None, 0.0, pi, None),
            "start point is not on the profile",
        ),
    ):
        with pytest.raises(ValueError, match=message):
            factory.surface3d(bad, **resolve)
