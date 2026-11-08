"""Stored geometry is decoded independently of B-Rep topology and optional kernels."""

import json
from dataclasses import replace

import pytest

from parasolid_kit import (
    FacetedGeometryError,
    Vector3,
    compare_documents,
    decode_embedded_mesh,
    decode_polyline,
    faceted_geometry,
    parse_xt,
)
from parasolid_kit.interop import InteropLimits
from parasolid_kit.interop.preview import validate_glb_bytes, write_faceted_preview
from tests.support.faceted import stored_document
from tests.test_builtin_embedded_runtime import BASE, general_body


def test_authored_text_and_binary_preserve_pools_polylines_and_source_ranges():
    text, binary = stored_document(), stored_document("x_b")
    assert compare_documents(text, binary).equivalent
    for document in (text, binary):
        geometry = faceted_geometry(document)
        polyline, mesh = geometry.polylines[0], geometry.meshes[0]
        assert polyline.base_parameter == 17
        assert polyline.closed
        assert polyline.display_points() == (*polyline.points, polyline.points[0])
        assert mesh.positions.count == 3 and mesh.positions.capacity == 4
        assert mesh.positions.values[-3:] == (90, 90, 90)
        assert mesh.position_indices.values == (0, 1, 2, 99)
        assert mesh.inferred_index_origin() == 0
        assert mesh.triangles() == (
            (Vector3(0.0, 0.0, 0.0), Vector3(2.0, 0.0, 0.0), Vector3(0.0, 3.0, 0.0)),
        )
        assert mesh.source.byte_range == document.nodes[3].byte_range
        assert mesh == decode_embedded_mesh(document, 4)
        assert polyline == decode_polyline(document, 1)


def test_index_origin_is_selected_from_active_indices_and_ambiguity_is_rejected():
    one_based = decode_embedded_mesh(stored_document(indices=(1, 2, 3)), 4)
    assert one_based.inferred_index_origin() == 1
    ambiguous = decode_embedded_mesh(stored_document(indices=(1, 2, 3), position_count=4), 4)
    with pytest.raises(FacetedGeometryError, match="ambiguous"):
        ambiguous.triangles()
    assert ambiguous.triangles(index_origin=0) != ambiguous.triangles(index_origin=1)
    with pytest.raises(FacetedGeometryError):
        one_based.triangles(index_origin=0)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_linked_mesh_transform_places_vertices_and_source_orientation_is_explicit(encoding):
    mesh = decode_embedded_mesh(stored_document(encoding, transform=True), 4)
    assert mesh.triangles()[0] == (
        Vector3(20.0, 0.0, 0.0),
        Vector3(20.0, 4.0, 0.0),
        Vector3(14.0, 0.0, 0.0),
    )
    polyline = decode_polyline(stored_document(encoding, sense=ord("-")), 1)
    assert polyline.display_points()[0] == polyline.points[-1]


def test_missing_meshes_bad_pools_and_budgets_fail_without_guessing():
    document = stored_document(embedded=False)
    mesh = decode_embedded_mesh(document, 4)
    assert mesh.positions is None
    with pytest.raises(FacetedGeometryError, match="not embedded"):
        mesh.triangles()
    for parameters in ({"capacity": 3}, {"normal_type": 2}, {"indices": (-1, 1, 2)}):
        with pytest.raises(FacetedGeometryError):
            faceted_geometry(stored_document(**parameters))
    with pytest.raises(FacetedGeometryError, match="max_pool_values"):
        faceted_geometry(stored_document(), max_pool_values=12)
    with pytest.raises(FacetedGeometryError, match="explicit"):
        faceted_geometry(parse_xt(general_body("x_t", BASE)))


def test_native_preview_displays_only_selected_stored_geometry_without_occt_claims(tmp_path):
    geometry = faceted_geometry(stored_document())
    result = write_faceted_preview(geometry, tmp_path / "stored", source_unit="cm")
    assert result.triangle_count == 1
    assert validate_glb_bytes(result.glb_path.read_bytes()).valid
    manifest = json.loads(result.manifest_path.read_bytes())
    assert manifest["conversion"]["backend"] == "stored"
    assert manifest["conversion"]["occt_valid"] is None
    assert manifest["source"]["scope"] == "stored_geometry_subset"
    assert not manifest["source"]["complete"] and manifest["preview"]["partial"]
    assert manifest["preview"]["bounds"] == [0, 0, 0, 20, 30, 10]
    assert manifest["preview"]["counts"]["vertices"] == 7
    assert all(
        p["body_ids"] == [] and p["parasolid_face_ids"] == [] for p in manifest["primitives"]
    )
    with pytest.raises(FileExistsError):
        write_faceted_preview(geometry, result.directory, source_unit="cm")
    failed = tmp_path / "budget"
    with pytest.raises(FacetedGeometryError, match="max_vertices"):
        write_faceted_preview(
            geometry, failed, source_unit="mm", limits=InteropLimits(max_vertices=2)
        )
    assert not failed.exists()
    with pytest.raises(FacetedGeometryError, match="not embedded"):
        write_faceted_preview(
            faceted_geometry(stored_document(embedded=False)), failed, source_unit="mm"
        )
    with pytest.raises(FacetedGeometryError, match="no geometry"):
        write_faceted_preview(replace(geometry, meshes=(), polylines=()), failed, source_unit="mm")


def test_cli_stored_geometry_writes_without_entering_occt_conversion(tmp_path, monkeypatch, capsys):
    from parasolid_kit import cli
    from parasolid_kit.interop import occt

    document = stored_document()
    source = tmp_path / "stored.x_t"
    source.write_bytes(document.raw_bytes)
    monkeypatch.setattr(cli, "_document", lambda *_: document)
    monkeypatch.setattr(
        occt, "to_occt", lambda *_args, **_kwargs: pytest.fail("stored route called OCCT")
    )
    assert (
        cli.main(
            [
                "viewer",
                str(source),
                "--stored-geometry",
                "--schema-dir",
                str(tmp_path),
                "--source-unit",
                "mm",
                "--write-only",
            ]
        )
        == 0
    )
    report = json.loads(capsys.readouterr().out)
    assert report["preview"]["backend"] == "stored"
    assert report["preview"]["occt_valid"] is None
    assert report["preview"]["triangle_count"] == 1
    assert (
        cli.main(
            ["viewer", str(source), "--stored-geometry", "--source-unit", "mm", "--write-only"]
        )
        == 2
    )
    assert "--schema-dir" in capsys.readouterr().err


def test_stored_preview_reports_float32_overflow_and_buffer_limits_before_writing(tmp_path):
    geometry = faceted_geometry(stored_document())
    line = replace(geometry.polylines[0], points=(Vector3(1e40, 0.0, 0.0), Vector3(2e40, 1.0, 0.0)))
    large = replace(geometry, meshes=(), polylines=(line,))
    with pytest.raises(FacetedGeometryError, match="Float32"):
        write_faceted_preview(large, tmp_path / "overflow", source_unit="mm")
    assert not (tmp_path / "overflow").exists()
    with pytest.raises(FacetedGeometryError, match="max_output_bytes"):
        write_faceted_preview(
            geometry,
            tmp_path / "budget",
            source_unit="mm",
            limits=InteropLimits(max_output_bytes=8),
        )
    assert not (tmp_path / "budget").exists()
