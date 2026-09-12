"""Analytic oracles for the bounded UV/intersection conversion path."""

from dataclasses import replace
from math import cos, sin

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
from tests._occt_fixtures import _Sources, make_box_model

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
