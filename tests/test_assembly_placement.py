"""Placement oracles independent of the optional OCCT implementation."""

from dataclasses import replace

import pytest

from parasolid_kit import (
    AssemblyPlacementError,
    InstancePartKind,
    Placement,
    Vector3,
    assembly_occurrences,
)
from tests.test_occt_placement import _assembled_box


def test_nested_occurrences_have_stable_paths_and_noncommuting_placements():
    model, _, _ = _assembled_box(nested=True)
    original = model.to_dict()
    occurrences = assembly_occurrences(model)
    assert [o.instance_path for o in occurrences] == [(0,), (1, 2)]
    assert [o.assembly_path for o in occurrences] == [(0,), (0, 1)]
    assert tuple(occurrences[1].placement.apply_point(Vector3(2.0, 3.0, 4.0))) == (97, 2, 54)
    assert tuple(occurrences[1].placement.apply_normal(Vector3(1.0, 0.0, 0.0))) == (0, 1, 0)
    assert assembly_occurrences(model, 1)[0].instance_path == (2,)
    assert tuple(
        assembly_occurrences(model, 1)[0].placement.apply_point(Vector3(2.0, 3.0, 4.0))
    ) == (-3, 2, 54)
    assert model.to_dict() == original


def test_general_affine_normals_use_inverse_transpose_and_source_scale_applies_to_translation():
    placement = Placement(((2.0, 0.0, 0.0, 10.0), (0.0, 3.0, 0.0, 20.0), (0.0, 0.0, -4.0, 30.0)))
    assert placement.determinant == -24
    assert tuple(placement.apply_point(Vector3(1.0, 2.0, 3.0))) == (12, 26, 18)
    normal = placement.apply_normal(Vector3(1.0, 1.0, 0.0))
    assert normal.x / normal.y == pytest.approx(1.5)
    model, _, _ = _assembled_box()
    transform = replace(model.instances[0].transform, scale=2.0)
    scaled = Placement.from_transform(transform)
    assert tuple(scaled.apply_point(Vector3(1.0, 0.0, 0.0))) == (202, 0, 0)


@pytest.mark.parametrize("mutation", ["missing", "owner", "duplicate", "cycle", "perspective"])
def test_bad_graphs_and_transforms_fail_instead_of_dropping_occurrences(mutation):
    model, _, _ = _assembled_box(nested=True)
    if mutation == "missing":
        model = replace(
            model, instances=(replace(model.instances[0], part=999), *model.instances[1:])
        )
    elif mutation == "owner":
        model = replace(
            model, instances=(replace(model.instances[0], assembly=99), *model.instances[1:])
        )
    elif mutation == "duplicate":
        model = replace(model, instances=(*model.instances, model.instances[0]))
    elif mutation == "cycle":
        model = replace(
            model,
            instances=(
                *model.instances[:2],
                replace(model.instances[2], part_kind=InstancePartKind.ASSEMBLY, part=0),
            ),
        )
    else:
        transform = replace(model.instances[0].transform, perspective=Vector3(1.0, 0.0, 0.0))
        model = replace(
            model,
            instances=(replace(model.instances[0], transform=transform), *model.instances[1:]),
        )
    with pytest.raises(AssemblyPlacementError):
        assembly_occurrences(model)


def test_expansion_budget_and_standalone_bodies_are_explicit():
    model, _, _ = _assembled_box(nested=True)
    with pytest.raises(AssemblyPlacementError, match="max_instances"):
        assembly_occurrences(model, max_instances=2)
    standalone = replace(model, assemblies=(), instances=())
    assert assembly_occurrences(standalone)[0].placement == Placement()
    with pytest.raises(AssemblyPlacementError):
        Placement(((0.0, 0.0, 0.0, 0.0),) * 3)


def test_nested_negative_instances_are_never_silently_cancelled():
    model, _, _ = _assembled_box(nested=True, negative=True)
    instances = list(model.instances)
    instances[1] = replace(instances[1], negative=True)
    occurrences = assembly_occurrences(replace(model, instances=tuple(instances)))
    assert occurrences[1].negative


def test_assembly_preview_keeps_source_mesh_once_and_scales_translation(tmp_path):
    pytest.importorskip("OCP")
    from parasolid_kit.interop import PreviewError
    from parasolid_kit.interop.occt import to_occt
    from parasolid_kit.interop.preview import tessellate_preview

    model, _, _ = _assembled_box(nested=True)
    plain = to_occt(model, source_unit="cm", target_unit="mm")
    result = tessellate_preview(plain, model, assembly=True)
    assert result.face_primitive_count == 6
    assert [o["instance_path"] for o in result.manifest["occurrences"]] == [[0], [1, 2]]
    assert result.manifest["occurrences"][1]["matrix"][0][3] == 1000
    assert result.manifest["occurrences"][1]["matrix"][2][3] == 500
    already_placed = to_occt(model, source_unit="mm", place_instances=True)
    with pytest.raises(PreviewError, match="unplaced"):
        tessellate_preview(already_placed, model, assembly=True)


def test_cli_assembly_flag_writes_occurrence_metadata(tmp_path, monkeypatch, capsys):
    pytest.importorskip("OCP")
    import json
    from types import SimpleNamespace

    from parasolid_kit import cli

    model, _, _ = _assembled_box(nested=True)
    source = tmp_path / "assembly.x_t"
    source.write_bytes(b"author-created B-Rep at parser boundary")
    monkeypatch.setattr(
        cli,
        "read_brep",
        lambda *_args, **_kwargs: SimpleNamespace(
            brep=model, document=SimpleNamespace(raw_bytes=source.read_bytes())
        ),
    )
    assert (
        cli.main(["viewer", str(source), "--assembly", "--source-unit", "mm", "--write-only"]) == 0
    )
    report = json.loads(capsys.readouterr().out)
    manifest = json.loads(
        (tmp_path / "assembly.parasolid-preview/preview.manifest.json").read_bytes()
    )
    assert len(manifest["occurrences"]) == 2
    assert report["preview"]["counts"]["face_primitives"] == 6
