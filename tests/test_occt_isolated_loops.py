"""Isolated loops (one vertex inside a face) trim nothing and do not stop a conversion."""

from __future__ import annotations

import importlib.util
from dataclasses import replace

import pytest

from parasolid_kit import PlaneSurface, Sense, Vector3
from parasolid_kit.brep import HalfEdge, Loop, PointGeometry, Vertex
from parasolid_kit.interop.occt import to_occt
from tests._occt_fixtures import _Sources, make_box_model, make_swept_cylinder_model

pytestmark = pytest.mark.skipif(
    importlib.util.find_spec("OCP") is None, reason="OCP is not installed"
)


def _with_isolated_vertex(model, face_id: int, position: Vector3):
    """Add an isolated loop, a fin pointing at itself around a new vertex, to one face."""
    sources = _Sources()
    vertex_id = max((item.id for item in model.vertices), default=-1) + 1
    point_id = max((item.id for item in model.points), default=-1) + 1
    fin_id = max(item.id for item in model.half_edges) + 1
    loop_id = max(item.id for item in model.loops) + 1
    body = model.bodies[0]
    faces = tuple(
        replace(face, loops=(*face.loops, loop_id)) if face.id == face_id else face
        for face in model.faces
    )
    return replace(
        model,
        bodies=(replace(body, vertices=(*body.vertices, vertex_id)),),
        faces=faces,
        loops=(*model.loops, Loop(loop_id, face_id, (fin_id,), sources.next("loop"))),
        half_edges=(
            *model.half_edges,
            # The reference leaves the sense of an isolated fin unset.
            HalfEdge(
                fin_id,
                loop_id,
                fin_id,
                fin_id,
                vertex_id,
                None,
                None,
                None,
                Sense.UNKNOWN,
                False,
                sources.next("fin"),
            ),
        ),
        vertices=(
            *model.vertices,
            Vertex(vertex_id, point_id, None, body.source, sources.next("vertex")),
        ),
        points=(*model.points, PointGeometry(point_id, position, None, sources.next("point"))),
    )


def _top_face(model):
    surfaces = {item.id: item for item in model.surfaces}
    for face in model.faces:
        definition = surfaces[face.surface].definition
        if isinstance(definition, PlaneSurface) and definition.normal.z > 0.5:
            return face, definition
    raise AssertionError("no top face")


def test_an_isolated_vertex_in_a_planar_face_is_skipped_on_the_analytic_path():
    plain = make_box_model()
    face, plane = _top_face(plain)
    model = _with_isolated_vertex(plain, face.id, Vector3(20.0, 15.0, plane.point.z))
    reference = to_occt(plain, source_unit="mm")
    result = to_occt(model, source_unit="mm")
    assert result.report.occt_valid and result.report.conversion_complete
    assert result.report.metrics.volume == pytest.approx(reference.report.metrics.volume)
    assert result.report.output_topology == reference.report.output_topology
    assert "skip_isolated_vertex_loops" in result.report.topology_operations
    isolated = model.vertices[-1].id
    assert not any(
        r.source.kind.value == "vertex" and r.source.entity_id == isolated
        for r in result.source_map.relations
    )


def test_an_isolated_vertex_in_a_cap_is_skipped_on_the_parametric_path():
    plain = make_swept_cylinder_model()
    face, plane = _top_face(plain)
    model = _with_isolated_vertex(plain, face.id, Vector3(0.0, 0.0, plane.point.z))
    reference = to_occt(plain, source_unit="mm")
    result = to_occt(model, source_unit="mm")
    assert result.report.occt_valid and result.report.conversion_complete
    assert result.report.metrics.volume == pytest.approx(reference.report.metrics.volume)
    assert "skip_isolated_vertex_loops" in result.report.topology_operations
    assert "project_3d_curve_to_surface_parameters" in result.report.topology_operations
