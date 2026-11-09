"""Common assembly decisions before optional runtimes, plus placed-shape oracles."""

from dataclasses import replace
from types import SimpleNamespace

import pytest

from parasolid_kit import (
    Assembly,
    AssemblyKind,
    AssemblyPlacementError,
    Instance,
    InstancePartKind,
    Vector3,
    assembly_occurrences,
    read_brep,
)
from parasolid_kit.interop import InteropLimits, OcctConversionError, PreviewError
from parasolid_kit.interop.occt import conversion, to_occt
from parasolid_kit.interop.occt.placement import prepare_instances
from parasolid_kit.interop.preview import tessellate_preview
from parasolid_kit.interop.preview.assembly import add_assembly_manifest
from tests._occt_fixtures import _Sources
from tests.support.import_contract import assert_dependency_free_imports
from tests.test_builtin_assembly_runtime import STANDARD_KEY, fixture
from tests.test_occt_placement import _assembled_box


def _layered_model(depth, width=1):
    """Shared subassemblies: width**depth leaves, using only depth*width links."""
    model, _, _ = _assembled_box()
    sources = _Sources()
    instances = []
    incoming = {i: [] for i in range(depth)}
    for level in range(depth):
        for _ in range(width):
            entity_id = len(instances)
            part_kind = InstancePartKind.BODY if level == depth - 1 else InstancePartKind.ASSEMBLY
            part = model.bodies[0].id if level == depth - 1 else level + 1
            instances.append(
                Instance(entity_id, False, part_kind, part, None, level, sources.next("instance"))
            )
            if part_kind is InstancePartKind.ASSEMBLY:
                incoming[part].append(entity_id)
    assemblies = tuple(
        Assembly(
            i,
            AssemblyKind.COLLECTIVE,
            1e3,
            1e-8,
            tuple(range(i * width, (i + 1) * width)),
            tuple(incoming[i]),
            sources.next("assembly"),
        )
        for i in range(depth)
    )
    return replace(model, assemblies=assemblies, instances=tuple(instances))


def _malformed_model(case):
    model, _, _ = _assembled_box(nested=True)
    assemblies, instances = list(model.assemblies), list(model.instances)
    if case == "cycle":
        instances[2] = replace(instances[2], part_kind=InstancePartKind.ASSEMBLY, part=0)
        assemblies[0] = replace(assemblies[0], referencing_instances=(2,))
    elif case == "unreachable_cycle":
        # A legitimate root must not hide a disconnected cyclic component.
        assemblies.extend(
            (
                replace(assemblies[0], id=10, instances=(10,), referencing_instances=(11,)),
                replace(assemblies[0], id=11, instances=(11,), referencing_instances=(10,)),
            )
        )
        instances.extend(
            (
                replace(instances[1], id=10, assembly=10, part=11),
                replace(instances[1], id=11, assembly=11, part=10),
            )
        )
    elif case == "missing_part":
        instances[0] = replace(instances[0], part=999)
    elif case == "missing_assembly":
        instances[1] = replace(instances[1], part=999)
    elif case == "missing_child":
        assemblies[0] = replace(assemblies[0], instances=(0, 1, 999))
    elif case == "wrong_owner":
        instances[0] = replace(instances[0], assembly=1)
    elif case == "orphan_instance":
        assemblies[0] = replace(assemblies[0], instances=(1,))
    elif case == "duplicate_instance_id":
        instances.append(instances[0])
    elif case == "duplicate_assembly_id":
        assemblies.append(assemblies[0])
    elif case == "duplicate_body_id":
        return replace(model, bodies=model.bodies * 2)
    elif case == "duplicate_child":
        assemblies[0] = replace(assemblies[0], instances=(0, 1, 0))
    elif case == "missing_reverse_link":
        assemblies[1] = replace(assemblies[1], referencing_instances=())
    elif case == "spurious_reverse_link":
        assemblies[0] = replace(assemblies[0], referencing_instances=(0,))
    elif case == "duplicate_reverse_link":
        assemblies[1] = replace(assemblies[1], referencing_instances=(1, 1))
    elif case == "perspective":
        instances[0] = replace(
            instances[0],
            transform=replace(instances[0].transform, perspective=Vector3(1.0, 0.0, 0.0)),
        )
    elif case == "unknown_flags":
        instances[0] = replace(instances[0], transform=replace(instances[0].transform, flags=32))
    elif case == "zero_scale":
        instances[0] = replace(instances[0], transform=replace(instances[0].transform, scale=0.0))
    elif case == "nonfinite_matrix":
        instances[0] = replace(
            instances[0],
            transform=replace(
                instances[0].transform,
                rotation=((float("nan"), 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)),
            ),
        )
    elif case == "depth":
        return _layered_model(65)
    else:
        raise AssertionError(case)
    return replace(model, assemblies=tuple(assemblies), instances=tuple(instances))


def _manifest(model, **kwargs):
    converted = SimpleNamespace(
        report=SimpleNamespace(
            placement_requested=False, options=SimpleNamespace(applied_scale=1.0)
        )
    )
    return add_assembly_manifest(
        {"primitives": [{"body_ids": [model.bodies[0].id]}]},
        model,
        converted,
        InteropLimits(**kwargs),
    )


@pytest.mark.parametrize(
    "case",
    [
        "cycle",
        "unreachable_cycle",
        "missing_part",
        "missing_assembly",
        "missing_child",
        "wrong_owner",
        "orphan_instance",
        "duplicate_instance_id",
        "duplicate_assembly_id",
        "duplicate_body_id",
        "duplicate_child",
        "missing_reverse_link",
        "spurious_reverse_link",
        "duplicate_reverse_link",
        "perspective",
        "unknown_flags",
        "zero_scale",
        "nonfinite_matrix",
        "depth",
    ],
)
@pytest.mark.parametrize("partial", [False, True])
def test_bad_assembly_is_rejected_consistently_without_optional_runtime(case, partial, monkeypatch):
    model = _malformed_model(case)
    before = model.to_dict()
    with pytest.raises(AssemblyPlacementError):
        assembly_occurrences(model)
    with pytest.raises(AssemblyPlacementError):
        _manifest(model)
    monkeypatch.setattr(
        conversion, "require_occt", lambda: pytest.fail("runtime must not be loaded")
    )
    with pytest.raises(OcctConversionError) as caught:
        to_occt(model, source_unit="mm", place_instances=True, partial=partial)
    assert caught.value.diagnostic.code == "occt.invalid_topology"
    assert caught.value.report.ocp_version is None
    assert not caught.value.report.conversion_complete
    assert model.to_dict() == before


def test_shared_budget_counts_expanded_links_including_repeated_subassemblies(monkeypatch):
    model = _layered_model(5, width=2)
    # Only ten distinct links, but expansion follows 2+4+8+16+32 = 62 links.
    assert len(model.instances) == 10
    assert len(assembly_occurrences(model, max_instances=62)) == 32
    assert len(prepare_instances(model, 62)) == 32
    assert len(_manifest(model, max_instances=62)["occurrences"]) == 32
    with pytest.raises(AssemblyPlacementError, match="max_instances"):
        assembly_occurrences(model, max_instances=61)
    with pytest.raises(AssemblyPlacementError, match="max_instances"):
        _manifest(model, max_instances=61)
    monkeypatch.setattr(
        conversion, "require_occt", lambda: pytest.fail("runtime must not be loaded")
    )
    with pytest.raises(OcctConversionError) as caught:
        to_occt(
            model, source_unit="mm", place_instances=True, limits=InteropLimits(max_instances=61)
        )
    assert caught.value.diagnostic.code == "occt.limit_exceeded"
    details = caught.value.diagnostic.details
    assert {key: details[key] for key in ("resource", "observed", "limit")} == {
        "resource": "max_instances",
        "observed": 62,
        "limit": 61,
    }
    assert details["cause_code"] == "assembly.limit_exceeded"
    assert details["node_index"] == model.assemblies[-1].source.node_index
    assert caught.value.report.ocp_version is None


def test_standalone_occurrences_also_obey_shared_budget():
    model, _, _ = _assembled_box()
    model = replace(
        model,
        assemblies=(),
        instances=(),
        bodies=(model.bodies[0], replace(model.bodies[0], id=100)),
    )
    assert len(prepare_instances(model, 2)) == 2
    with pytest.raises(AssemblyPlacementError, match="max_instances"):
        prepare_instances(model, 1)


def test_depth_boundary_is_64_assemblies_on_every_path():
    model = _layered_model(64)
    (occurrence,) = assembly_occurrences(model)
    assert len(occurrence.assembly_path) == 64
    assert prepare_instances(model, 100_000) == (occurrence,)
    assert _manifest(model)["occurrences"][0]["instance_path"] == list(range(64))


def test_root_selection_is_explicit_and_preserves_valid_incoming_links():
    model, _, _ = _assembled_box(nested=True)
    (occurrence,) = assembly_occurrences(model, 1)
    assert occurrence.instance_path == (2,)
    assert occurrence.assembly_path == (1,)
    for root in (True, 999):
        with pytest.raises(AssemblyPlacementError, match="root"):
            assembly_occurrences(model, root)


def test_occt_similarity_constraint_remains_stricter_than_general_affine_placement(monkeypatch):
    model, _, _ = _assembled_box(nested=True)
    # Cancelling shears cannot bypass the requirement for each individual transform.
    outer = replace(
        model.instances[1].transform, rotation=((1.0, 0.5, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
    )
    inner = replace(
        model.instances[2].transform, rotation=((1.0, -0.5, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
    )
    model = replace(
        model,
        instances=(
            model.instances[0],
            replace(model.instances[1], transform=outer),
            replace(model.instances[2], transform=inner),
        ),
    )
    assert len(assembly_occurrences(model)) == 2
    monkeypatch.setattr(
        conversion, "require_occt", lambda: pytest.fail("runtime must not be loaded")
    )
    with pytest.raises(OcctConversionError, match="rotation with a uniform scale"):
        to_occt(model, source_unit="mm", place_instances=True)


@pytest.mark.parametrize("leaf_negative", [False, True])
def test_negative_parent_marks_descendant_without_boolean_cancellation(leaf_negative):
    pytest.importorskip("OCP")
    model, _, _ = _assembled_box(nested=True, negative=leaf_negative)
    model = replace(
        model,
        instances=(
            model.instances[0],
            replace(model.instances[1], negative=True),
            model.instances[2],
        ),
    )
    assert assembly_occurrences(model)[1].negative
    result = to_occt(model, source_unit="mm", place_instances=True)
    assert result.report.instances_placed == 2
    assert result.report.instances_negative == 1
    assert result.report.output_topology.to_dict()["solids"] == 2
    notes = {
        relation.note
        for relation in result.source_map.relations
        if relation.source.kind.value == "instance"
    }
    assert notes == {"instance 0", "instance 1/2 (negative)"}
    with pytest.raises(AssemblyPlacementError, match="Boolean"):
        _manifest(model)


@pytest.mark.parametrize("inner_scale", [0.5, -0.5])
@pytest.mark.parametrize("source_unit,unit_scale", [("mm", 1.0), ("cm", 10.0)])
def test_occt_copies_match_common_scaled_composed_and_reflected_placements(
    inner_scale, source_unit, unit_scale
):
    pytest.importorskip("OCP")
    model, _, _ = _assembled_box(nested=True)
    instances = list(model.instances)
    instances[1] = replace(instances[1], transform=replace(instances[1].transform, scale=2.0))
    instances[2] = replace(
        instances[2], transform=replace(instances[2].transform, scale=inner_scale)
    )
    model = replace(model, instances=tuple(instances))
    before = model.to_dict()
    points = [
        occurrence.placement.apply_point(point.position)
        for occurrence in assembly_occurrences(model)
        for point in model.points
    ]
    expected = tuple(
        unit_scale * func(tuple(point)[axis] for point in points)
        for func in (min, max)
        for axis in range(3)
    )
    result = to_occt(model, source_unit=source_unit, target_unit="mm", place_instances=True)
    assert result.report.occt_valid
    assert result.report.metrics.bounding_box == pytest.approx(expected, abs=1e-6)
    assert result.report.instances_placed == 2
    assert model.to_dict() == before


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_parsed_assembly_and_occt_agree_on_inherited_negative_instances(encoding):
    pytest.importorskip("OCP")
    model = read_brep(fixture(encoding, STANDARD_KEY)).brep
    occurrences = assembly_occurrences(model)
    assert [occurrence.instance_path for occurrence in occurrences] == [(0,), (1, 2)]
    assert [occurrence.negative for occurrence in occurrences] == [False, True]
    result = to_occt(model, source_unit="mm", place_instances=True)
    assert result.report.instances_placed == len(occurrences)
    assert result.report.instances_negative == sum(
        occurrence.negative for occurrence in occurrences
    )


def test_shared_subassembly_produces_distinct_occt_copies_and_source_paths():
    pytest.importorskip("OCP")
    model = _layered_model(2, width=2)
    result = to_occt(model, source_unit="mm", place_instances=True)
    assert result.report.instances_placed == 4
    assert result.report.output_topology.to_dict()["solids"] == 4
    instances = [
        relation
        for relation in result.source_map.relations
        if relation.source.kind.value == "instance"
    ]
    assert sorted(relation.source.entity_id for relation in instances) == [2, 2, 3, 3]
    assert len({relation.target_key for relation in instances}) == 4
    assert {relation.note for relation in instances} == {
        "instance 0/2",
        "instance 0/3",
        "instance 1/2",
        "instance 1/3",
    }


def test_preview_reports_the_same_expansion_resource_limit():
    pytest.importorskip("OCP")
    model, _, _ = _assembled_box(nested=True)
    converted = to_occt(model, source_unit="mm")
    with pytest.raises(PreviewError) as caught:
        tessellate_preview(converted, model, assembly=True, limits=InteropLimits(max_instances=2))
    assert caught.value.diagnostic.code == "preview.limit_exceeded"
    details = caught.value.diagnostic.details
    assert {key: details[key] for key in ("resource", "observed", "limit")} == {
        "resource": "max_instances",
        "observed": 3,
        "limit": 2,
    }
    assert details["cause_code"] == "assembly.limit_exceeded"
    assert details["node_index"] == model.assemblies[1].source.node_index


def test_assembly_preflight_does_not_load_optional_runtime_in_fresh_interpreter():
    data = fixture("x_t", STANDARD_KEY)
    assert_dependency_free_imports(f"""
from dataclasses import replace
from parasolid_kit import read_brep, assembly_occurrences
from parasolid_kit.interop import InteropLimits, OcctConversionError
from parasolid_kit.interop.occt import to_occt
model = read_brep({data!r}).brep
assert [item.negative for item in assembly_occurrences(model)] == [False, True]
try:
    to_occt(model, source_unit="mm", place_instances=True,
            limits=InteropLimits(max_instances=2))
except OcctConversionError as error:
    assert error.diagnostic.code == "occt.limit_exceeded"
    assert error.report.ocp_version is None
else:
    raise AssertionError("expanded-link budget was not enforced")
bad = replace(model, assemblies=(replace(model.assemblies[0], referencing_instances=(999,)),
                                 model.assemblies[1]))
try:
    to_occt(bad, source_unit="mm", place_instances=True)
except OcctConversionError as error:
    assert error.diagnostic.code == "occt.invalid_topology"
    assert error.report.ocp_version is None
else:
    raise AssertionError("invalid assembly reached the optional runtime")
""")
