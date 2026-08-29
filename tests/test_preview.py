from __future__ import annotations

import hashlib
import http.client
import importlib.util
import json
import shutil
from dataclasses import FrozenInstanceError, replace
from pathlib import Path

import pytest

from parasolid_kit.interop import InteropLimits, PreviewError
from parasolid_kit.interop.occt import SourceEntityKind, SourceShapeMap, to_occt
from parasolid_kit.interop.preview import (
    STATIC_ASSET_NAMES,
    STATIC_ASSET_SHA256,
    GlbBuilder,
    PreviewOptions,
    create_preview_server,
    validate_glb_bytes,
    write_preview,
)
from tests._occt_fixtures import make_box_model, make_cylinder_hole_model

HAS_OCP = importlib.util.find_spec("OCP") is not None
STATIC = Path(__file__).resolve().parents[1] / "src/parasolid_kit/interop/preview/static"


def test_preview_options_are_explicit_finite_and_immutable() -> None:
    options = PreviewOptions(linear_deflection=0.05, angular_deflection=0.25)

    assert options.to_dict() == {
        "linear_deflection": 0.05,
        "angular_deflection": 0.25,
        "include_edges": True,
        "allow_partial": False,
    }
    with pytest.raises(FrozenInstanceError):
        options.linear_deflection = 1.0  # type: ignore[misc]
    with pytest.raises(ValueError, match="finite positive"):
        PreviewOptions(linear_deflection=0.0)
    with pytest.raises(ValueError, match="pi"):
        PreviewOptions(angular_deflection=4.0)


def test_small_glb_writer_is_deterministic_and_structurally_valid() -> None:
    def build() -> bytes:
        builder = GlbBuilder(max_binary_bytes=4096)
        builder.add_triangles(
            target_key="occt:face:000001",
            pick_id=1,
            positions=[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals=[0.0, 0.0, 1.0] * 3,
            indices=[0, 1, 2],
        )
        return builder.build()

    first = build()
    second = build()
    report = validate_glb_bytes(first)

    assert first == second
    assert report.valid is True
    assert report.version == 2
    assert report.primitive_count == 1
    assert report.accessor_count == 3
    assert validate_glb_bytes(first[:-1]).valid is False


def test_bundled_assets_match_the_reviewed_exact_hash_allowlist() -> None:
    assert tuple(sorted(STATIC_ASSET_NAMES)) == ("index.html", "viewer.css", "viewer.js")
    for name in STATIC_ASSET_NAMES:
        assert (
            hashlib.sha256((STATIC / name).read_bytes()).hexdigest() == (STATIC_ASSET_SHA256[name])
        )
    html = (STATIC / "index.html").read_text(encoding="utf-8")
    javascript = (STATIC / "viewer.js").read_text(encoding="utf-8")
    assert 'content="1.0.0; license=MIT"' in html
    assert "https://" not in html
    assert "https://" not in javascript
    assert "innerHTML" not in javascript


def test_preview_server_rejects_external_binding_without_explicit_permission(
    tmp_path: Path,
) -> None:
    with pytest.raises(PreviewError) as captured:
        create_preview_server(tmp_path, host="0.0.0.0")

    assert captured.value.diagnostic.code == "preview.external_bind_forbidden"


def test_preview_server_only_serves_reviewed_routes_and_security_headers(
    tmp_path: Path,
) -> None:
    for name in STATIC_ASSET_NAMES:
        shutil.copyfile(STATIC / name, tmp_path / name)
    (tmp_path / "preview.glb").write_bytes(b"glb")
    (tmp_path / "preview.manifest.json").write_text("{}\n", encoding="ascii")
    (tmp_path / "secret.txt").write_text("secret", encoding="ascii")

    with create_preview_server(tmp_path) as server:
        server.start()
        connection = http.client.HTTPConnection("127.0.0.1", server.port, timeout=5)
        connection.request("GET", "/")
        response = connection.getresponse()
        body = response.read()
        assert response.status == 200
        assert body == (STATIC / "index.html").read_bytes()
        assert response.getheader("X-Content-Type-Options") == "nosniff"
        assert response.getheader("X-Frame-Options") == "DENY"
        assert "default-src 'self'" in (response.getheader("Content-Security-Policy") or "")
        assert "Python" not in (response.getheader("Server") or "")

        connection.request("GET", "/../secret.txt")
        rejected = connection.getresponse()
        rejected.read()
        assert rejected.status == 404
        assert rejected.getheader("X-Content-Type-Options") == "nosniff"

        connection.request("GET", "/secret.txt")
        rejected = connection.getresponse()
        rejected.read()
        assert rejected.status == 404
        connection.close()

        hostile = http.client.HTTPConnection("127.0.0.1", server.port, timeout=5)
        hostile.request("GET", "/", headers={"Host": "evil.example"})
        rejected = hostile.getresponse()
        rejected.read()
        assert rejected.status == 400
        hostile.close()


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
@pytest.mark.parametrize("factory", [make_box_model, make_cylinder_hole_model])
def test_preview_writes_valid_glb_and_source_reverse_mapping(
    factory: object,
    tmp_path: Path,
) -> None:
    model = factory()  # type: ignore[operator]
    converted = to_occt(
        model,
        source_unit="mm",
        source_identity=f"sha256:{'a' * 64}",
    )

    result = write_preview(converted, model, tmp_path / "preview")
    manifest = json.loads(result.manifest_path.read_text(encoding="ascii"))
    face = next(item for item in manifest["primitives"] if item["kind"] == "face")
    edge = next(item for item in manifest["primitives"] if item["kind"] == "edge")
    face_source = next(item for item in face["source_entities"] if item["kind"] == "face")

    assert result.report.status == "complete"
    assert result.report.glb_validation.valid is True
    assert result.report.face_primitive_count == len(model.faces)
    assert result.report.edge_primitive_count >= len(model.edges)
    assert face["parasolid_face_ids"]
    assert face_source["node_id"] is not None
    assert face_source["byte_range"]["end"] > face_source["byte_range"]["start"]
    assert edge["target_key"].startswith("occt:edge:")
    assert manifest["source"]["sha256"] == "a" * 64
    assert "path" not in json.dumps(manifest).lower()


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_preview_output_is_deterministic_for_one_conversion(
    tmp_path: Path,
) -> None:
    model = make_box_model()
    converted = to_occt(model, source_unit="mm")

    first = write_preview(converted, model, tmp_path / "first")
    second = write_preview(converted, model, tmp_path / "second")

    assert first.glb_path.read_bytes() == second.glb_path.read_bytes()
    assert first.manifest_path.read_bytes() == second.manifest_path.read_bytes()


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_preview_default_rejects_missing_face_mapping_and_partial_mode_lists_it(
    tmp_path: Path,
) -> None:
    model = make_box_model()
    converted = to_occt(model, source_unit="mm")
    source_map = SourceShapeMap(
        tuple(
            relation
            for relation in converted.source_map.relations
            if not (
                relation.source.kind is SourceEntityKind.FACE
                and relation.source.entity_id == model.faces[0].id
            )
        )
    )
    incomplete_mapping = replace(converted, source_map=source_map)

    with pytest.raises(PreviewError) as captured:
        write_preview(incomplete_mapping, model, tmp_path / "strict")
    assert captured.value.diagnostic.code == "preview.incomplete_mapping"

    result = write_preview(
        incomplete_mapping,
        model,
        tmp_path / "partial",
        options=PreviewOptions(allow_partial=True),
    )
    manifest = json.loads(result.manifest_path.read_text(encoding="ascii"))

    assert result.report.status == "partial"
    assert result.report.missing_face_count == 1
    assert manifest["preview"]["partial"] is True
    assert manifest["missing_entities"] == [
        {
            "entity_id": model.faces[0].id,
            "kind": "face",
            "reason": "no mapped render primitive",
            "source": {
                "byte_range": model.faces[0].source.byte_range.to_dict(),
                "entity_id": model.faces[0].id,
                "key": f"parasolid:face:{model.faces[0].id:06d}",
                "kind": "face",
                "node_id": model.faces[0].source.node_id,
                "node_index": model.faces[0].source.node_index,
                "node_type": model.faces[0].source.node_type,
                "type_name": model.faces[0].source.type_name,
            },
            "source_key": f"parasolid:face:{model.faces[0].id:06d}",
            "target_keys": [],
        }
    ]


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_preview_limits_fail_without_implicit_simplification(tmp_path: Path) -> None:
    model = make_box_model()
    converted = to_occt(model, source_unit="mm")

    with pytest.raises(PreviewError) as captured:
        write_preview(
            converted,
            model,
            tmp_path / "limited",
            limits=InteropLimits(max_triangles=1),
        )

    assert captured.value.diagnostic.code == "preview.limit_exceeded"
    assert captured.value.diagnostic.details == {
        "resource": "max_triangles",
        "observed": 2,
        "limit": 1,
    }
    assert not (tmp_path / "limited").exists()
