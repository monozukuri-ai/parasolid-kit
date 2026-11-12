"""Ring edges on a whole trimmed basis, pointed cones and wire bodies without edges.

The XT reference (printed pp. 48-49) trims the whole of a periodic basis with
coincident points and parameters a period apart; a ring edge without vertices
may use such a curve. A cone face bounded by one ring has its apex as an
isolated vertex loop. A wire body may hold isolated vertices alone. The models
are authored from the frustum and box fixtures.
"""

from __future__ import annotations

import importlib.util
from dataclasses import replace
from math import pi

import pytest

from parasolid_kit import Sense, Vector3
from parasolid_kit.brep import (
    Body,
    BodyKind,
    CurveGeometry,
    CurveKind,
    Region,
    RegionKind,
    Shell,
    TopologyValidation,
    TrimmedCurve,
)
from parasolid_kit.interop.errors import OcctConversionError
from parasolid_kit.interop.occt import to_occt
from tests._occt_fixtures import _Sources, make_box_model, make_cone_frustum_model
from tests.test_occt_isolated_loops import _with_isolated_vertex

pytestmark = pytest.mark.skipif(
    importlib.util.find_spec("OCP") is None, reason="OCP is not installed"
)

LOWER, UPPER, HEIGHT = 8.0, 4.0, 12.0
FRUSTUM_VOLUME = pi * HEIGHT * (LOWER**2 + LOWER * UPPER + UPPER**2) / 3.0


def _with_whole_trim(model, *, negative: bool):
    """Put the lower ring edge on a trimmed curve spanning its whole circle.

    With a basis of negative sense the trim runs down its parameter (pi to
    -pi), so the trimmed curve, and the edge on it, run clockwise: its fins
    take the opposite sense to keep their direction.
    """
    sources = _Sources()
    circle = model.curves[0]
    basis = replace(circle, sense=Sense.NEGATIVE) if negative else circle
    start = Vector3(-LOWER, 0.0, 0.0) if negative else Vector3(LOWER, 0.0, 0.0)
    trimmed = CurveGeometry(
        3,
        Sense.POSITIVE,
        circle.owner,
        CurveKind.TRIMMED,
        TrimmedCurve(
            basis.id,
            start,
            start,
            pi if negative else 0.0,
            -pi if negative else 2.0 * pi,
        ),
        sources.next("trimmed_curve"),
    )
    sense = Sense.NEGATIVE if negative else Sense.POSITIVE
    half_edges = tuple(
        replace(item, curve=3, sense=sense) if item.edge == 1 else item for item in model.half_edges
    )
    edges = tuple(replace(item, curve=3) if item.id == 1 else item for item in model.edges)
    return replace(
        model,
        curves=(basis, *model.curves[1:], trimmed),
        edges=edges,
        half_edges=half_edges,
    )


def test_a_ring_edge_on_the_whole_of_a_trimmed_circle_converts():
    results = []
    for negative in (False, True):
        model = _with_whole_trim(make_cone_frustum_model(LOWER, UPPER, HEIGHT), negative=negative)
        converted = to_occt(model, source_unit="mm")
        assert converted.report.conversion_complete
        assert converted.report.occt_valid
        assert converted.report.metrics.volume == pytest.approx(FRUSTUM_VOLUME, rel=1e-9)
        results.append(converted.report)
    # The clockwise trim reaches OCCT along the same fins: its edge is built up
    # the basis parameter and reversed, so both models take the same fixes.
    assert set(results[0].topology_operations) == set(results[1].topology_operations)


def test_a_partial_trim_cannot_carry_a_ring_edge():
    model = _with_whole_trim(make_cone_frustum_model(LOWER, UPPER, HEIGHT), negative=False)
    trimmed = model.curves[-1]
    half = replace(
        trimmed,
        definition=replace(
            trimmed.definition, end_point=Vector3(-LOWER, 0.0, 0.0), end_parameter=pi
        ),
    )
    with pytest.raises(OcctConversionError) as captured:
        to_occt(replace(model, curves=(*model.curves[:-1], half)), source_unit="mm")
    assert captured.value.diagnostic.code == "occt.unsupported_curve"
    assert "requires two endpoint vertices" in captured.value.diagnostic.message


def _pointed_cone():
    """The frustum without its upper cap, its cone running to the apex."""
    frustum = make_cone_frustum_model(LOWER, 0.0, HEIGHT)
    keep_faces = (1, 3)
    model = replace(
        frustum,
        bodies=(replace(frustum.bodies[0], edges=(1,)),),
        shells=(replace(frustum.shells[0], front_faces=keep_faces),),
        faces=tuple(
            replace(face, loops=(1,)) if face.id == 1 else face
            for face in frustum.faces
            if face.id in keep_faces
        ),
        loops=tuple(loop for loop in frustum.loops if loop.id in (1, 4)),
        half_edges=tuple(item for item in frustum.half_edges if item.id in (1, 4)),
        edges=frustum.edges[:1],
        curves=frustum.curves[:1],
        surfaces=tuple(item for item in frustum.surfaces if item.id in (1, 3)),
        topology=TopologyValidation(True, 2, 1, 1),
    )
    return _with_isolated_vertex(model, 1, Vector3(0.0, 0.0, HEIGHT))


def test_a_pointed_cone_closes_at_its_apex():
    converted = to_occt(_pointed_cone(), source_unit="mm")
    assert converted.report.conversion_complete
    assert converted.report.occt_valid
    assert converted.report.metrics.volume == pytest.approx(pi * LOWER**2 * HEIGHT / 3.0, rel=1e-9)
    assert "insert_degenerated_cone_apex_edge" in converted.report.topology_operations


def _with_edgeless_wire_body(model):
    sources = _Sources()
    body_id = max(item.id for item in model.bodies) + 1
    region_id = max(item.id for item in model.regions) + 1
    shell_id = max(item.id for item in model.shells) + 1
    return replace(
        model,
        bodies=(
            *model.bodies,
            Body(
                body_id, BodyKind.WIRE, 1.0e-6, 1.0e-8, (region_id,), (), (), sources.next("body")
            ),
        ),
        regions=(
            *model.regions,
            Region(region_id, RegionKind.VOID, body_id, (shell_id,), sources.next("region")),
        ),
        shells=(
            *model.shells,
            Shell(shell_id, region_id, (), (), (), None, sources.next("shell")),
        ),
    )


def test_a_wire_body_without_edges_is_left_out_of_a_partial_conversion():
    model = _with_edgeless_wire_body(make_box_model())
    converted = to_occt(model, source_unit="mm", require_complete=False, partial=True)
    failed = converted.report.bodies_failed
    assert [(item.body_id, item.reason) for item in failed] == [
        (model.bodies[-1].id, "wire body has no edges")
    ]
    assert converted.report.occt_valid
    assert converted.report.output_topology.to_dict()["faces"] == 6
    with pytest.raises(OcctConversionError) as captured:
        to_occt(model, source_unit="mm", require_complete=False)
    assert "has no edges" in captured.value.diagnostic.message
