"""Instance placement: the bodies positioned by an assembly come back as transformed copies."""

from __future__ import annotations

import importlib.util
from dataclasses import replace
from math import isclose

import pytest

from parasolid_kit import (
    Assembly,
    AssemblyKind,
    Instance,
    InstancePartKind,
    Transform,
    Vector3,
)
from parasolid_kit.interop import OcctConversionError
from parasolid_kit.interop.occt import to_occt
from tests._occt_fixtures import _Sources, make_box_model

pytestmark = pytest.mark.skipif(
    importlib.util.find_spec("OCP") is None, reason="OCP is not installed"
)

IDENTITY = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
# A quarter turn about z.
QUARTER = ((0.0, -1.0, 0.0), (1.0, 0.0, 0.0), (0.0, 0.0, 1.0))


def _transform(sources, rotation, translation, scale=1.0):
    return Transform(
        rotation=rotation,
        translation=translation,
        scale=scale,
        flags=Transform.TRANSLATION | Transform.ROTATION,
        perspective=Vector3(0.0, 0.0, 0.0),
        source=sources.next("transform"),
    )


def _assembled_box(*, nested: bool = False, negative: bool = False):
    """A root assembly placing the box twice: shifted along x, and a quarter turn on z.

    With ``nested`` the second placement goes through a sub-assembly that holds
    the turn while the root holds the shift.
    """
    model = make_box_model()
    sources = _Sources()
    body_id = model.bodies[0].id
    shift = Vector3(100.0, 0.0, 0.0)
    turn = Vector3(0.0, 0.0, 50.0)
    if not nested:
        instances = (
            Instance(
                0,
                False,
                InstancePartKind.BODY,
                body_id,
                _transform(sources, IDENTITY, shift),
                0,
                sources.next("instance"),
            ),
            Instance(
                1,
                negative,
                InstancePartKind.BODY,
                body_id,
                _transform(sources, QUARTER, turn),
                0,
                sources.next("instance"),
            ),
        )
        assemblies = (
            Assembly(0, AssemblyKind.COLLECTIVE, 1e3, 1e-8, (0, 1), (), sources.next("assembly")),
        )
    else:
        instances = (
            Instance(
                0,
                False,
                InstancePartKind.BODY,
                body_id,
                _transform(sources, IDENTITY, shift),
                0,
                sources.next("instance"),
            ),
            Instance(
                1,
                False,
                InstancePartKind.ASSEMBLY,
                1,
                _transform(sources, IDENTITY, shift),
                0,
                sources.next("instance"),
            ),
            Instance(
                2,
                negative,
                InstancePartKind.BODY,
                body_id,
                _transform(sources, QUARTER, turn),
                1,
                sources.next("instance"),
            ),
        )
        assemblies = (
            Assembly(0, AssemblyKind.COLLECTIVE, 1e3, 1e-8, (0, 1), (), sources.next("assembly")),
            Assembly(1, AssemblyKind.COLLECTIVE, 1e3, 1e-8, (2,), (1,), sources.next("assembly")),
        )
    return replace(model, assemblies=assemblies, instances=instances), shift, turn


def _bounds(result):
    return result.report.metrics.bounding_box


def test_bodies_stay_unplaced_unless_placement_is_requested():
    model, _shift, _turn = _assembled_box()
    result = to_occt(model, source_unit="mm")
    assert result.report.instances_placed == 0
    assert not result.report.placement_requested
    assert result.report.output_topology.to_dict()["solids"] == 1
    assert _bounds(result) == pytest.approx((0.0, 0.0, 0.0, 40.0, 30.0, 20.0), abs=1e-6)


def test_each_instance_places_a_transformed_copy_with_its_own_relations():
    model, _shift, _turn = _assembled_box()
    plain = to_occt(model, source_unit="mm")
    result = to_occt(model, source_unit="mm", place_instances=True)
    report = result.report
    assert report.placement_requested
    assert report.instances_placed == 2
    assert report.instances_negative == 0
    assert report.bodies_unreferenced == 0
    assert report.occt_valid and report.conversion_complete
    assert report.output_topology.to_dict()["solids"] == 2
    assert "instances_placed" in report.topology_operations
    assert isclose(report.metrics.volume, 2 * plain.report.metrics.volume, rel_tol=1e-9)
    # Shifted box: x in [100, 140]; turned box: (x, y) -> (-y, x) then +50 in z,
    # so x in [-30, 0], y in [0, 40], z in [50, 70].
    assert _bounds(result) == pytest.approx((-30.0, 0.0, 0.0, 140.0, 40.0, 70.0), abs=1e-6)
    relations = result.source_map.relations
    instance_relations = [r for r in relations if r.source.kind.value == "instance"]
    assert sorted(r.source.entity_id for r in instance_relations) == [0, 1]
    assert all(r.relation.value == "generated" for r in instance_relations)
    face_relations = [r for r in relations if r.source.kind.value == "face"]
    assert len(face_relations) == 2 * 6
    assert {r.note.split(";")[-1].strip() for r in face_relations} == {"instance 0", "instance 1"}
    assert report.to_dict()["placement"] == {
        "requested": True,
        "instances_placed": 2,
        "instances_negative": 0,
        "bodies_unreferenced": 0,
    }


def test_nested_assemblies_compose_their_transforms():
    model, _shift, _turn = _assembled_box(nested=True, negative=True)
    result = to_occt(model, source_unit="mm", place_instances=True)
    report = result.report
    assert report.instances_placed == 2
    assert report.instances_negative == 1
    # The turned box is shifted by the sub-assembly's instance as well:
    # x in [70, 100], y in [0, 40], z in [50, 70].
    assert _bounds(result) == pytest.approx((70.0, 0.0, 0.0, 140.0, 40.0, 70.0), abs=1e-6)
    chains = {r.note for r in result.source_map.relations if r.source.kind.value == "instance"}
    assert chains == {"instance 0", "instance 1/2 (negative)"}


def test_a_body_no_instance_references_stays_in_place():
    model, _shift, _turn = _assembled_box()
    instances = (model.instances[0],)
    assemblies = (replace(model.assemblies[0], instances=(0,)),)
    second = make_box_model(_origin=(0.0, 200.0, 0.0), _id_offset=100, _source_offset=100)
    combined = replace(
        model,
        assemblies=assemblies,
        instances=instances,
        bodies=(*model.bodies, *second.bodies),
        regions=(*model.regions, *second.regions),
        shells=(*model.shells, *second.shells),
        faces=(*model.faces, *second.faces),
        loops=(*model.loops, *second.loops),
        half_edges=(*model.half_edges, *second.half_edges),
        edges=(*model.edges, *second.edges),
        vertices=(*model.vertices, *second.vertices),
        points=(*model.points, *second.points),
        curves=(*model.curves, *second.curves),
        surfaces=(*model.surfaces, *second.surfaces),
        metrics=replace(model.metrics, bounding_box=None, surface_area=None, volume=None),
    )
    result = to_occt(combined, source_unit="mm", place_instances=True)
    assert result.report.instances_placed == 1
    assert result.report.bodies_unreferenced == 1
    assert result.report.output_topology.to_dict()["solids"] == 2
    assert _bounds(result) == pytest.approx((0.0, 0.0, 0.0, 140.0, 230.0, 20.0), abs=1e-6)


def test_a_non_rigid_transform_is_rejected():
    model, _shift, _turn = _assembled_box()
    sheared = replace(
        model.instances[0],
        transform=replace(
            model.instances[0].transform,
            rotation=((1.0, 0.5, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)),
        ),
    )
    model = replace(model, instances=(sheared, model.instances[1]))
    with pytest.raises(OcctConversionError, match="not a rotation with a uniform scale"):
        to_occt(model, source_unit="mm", place_instances=True)
