#!/usr/bin/env python3
"""Verify I6 bounded previews, provenance, partial warnings, and browser picking."""

from __future__ import annotations

import argparse
import json
import os
import re
import runpy
import shutil
import subprocess
import tempfile
from dataclasses import replace
from pathlib import Path
from typing import Any

from parasolid_kit.interop.occt import SourceEntityKind, SourceShapeMap, to_occt
from parasolid_kit.interop.preview import (
    STATIC_ASSET_NAMES,
    STATIC_ASSET_SHA256,
    PreviewOptions,
    create_preview_server,
    write_preview,
)

ROOT = Path(__file__).resolve().parents[1]
SYNTHETIC_FIXTURES = ROOT / "tests" / "_occt_fixtures.py"


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--chrome",
        type=Path,
        help="Chrome/Chromium executable; auto-detected when omitted",
    )
    parser.add_argument(
        "--skip-browser",
        action="store_true",
        help="run GLB, manifest, partial, and asset gates without the headless browser gate",
    )
    return parser.parse_args()


def _manifest(result: Any) -> dict[str, Any]:
    return json.loads(result.manifest_path.read_text(encoding="ascii"))


def _inspect(result: Any, expected_faces: int) -> dict[str, object]:
    manifest = _manifest(result)
    faces = [item for item in manifest["primitives"] if item["kind"] == "face"]
    edges = [item for item in manifest["primitives"] if item["kind"] == "edge"]
    if len(faces) != expected_faces:
        raise RuntimeError("preview face primitive count differs from the source")
    face = faces[0]
    source_faces = [item for item in face["source_entities"] if item["kind"] == "face"]
    if not face["parasolid_face_ids"] or not source_faces:
        raise RuntimeError("face primitive does not resolve to a Parasolid face")
    source = source_faces[0]
    if source["node_id"] is None or source["byte_range"]["end"] <= source["byte_range"]["start"]:
        raise RuntimeError("face primitive lacks source node and byte-range provenance")
    if not edges or not any(item["parasolid_edge_ids"] for item in edges):
        raise RuntimeError("preview lacks a pickable source edge")
    serialized = result.manifest_path.read_text(encoding="ascii")
    if "/private/not-for-preview" in serialized:
        raise RuntimeError("preview manifest leaked a source identity/path")
    if not result.report.glb_validation.valid:
        raise RuntimeError("preview GLB structural validator failed")
    if set(path.name for path in result.directory.iterdir()) != {
        *STATIC_ASSET_NAMES,
        "preview.glb",
        "preview.manifest.json",
    }:
        raise RuntimeError("preview output contains missing or unexpected resources")
    return {
        "status": result.report.status,
        "counts": manifest["preview"]["counts"],
        "glb_validation": result.report.glb_validation.to_dict(),
        "selected_face": {
            "target_key": face["target_key"],
            "parasolid_face_ids": face["parasolid_face_ids"],
            "node_id": source["node_id"],
            "byte_range": source["byte_range"],
        },
        "edge_mapping_present": True,
        "source_path_absent": True,
    }


def _partial_gate(model: Any, converted: Any, root: Path) -> dict[str, object]:
    missing_id = model.faces[0].id
    source_map = SourceShapeMap(
        tuple(
            relation
            for relation in converted.source_map.relations
            if not (
                relation.source.kind is SourceEntityKind.FACE
                and relation.source.entity_id == missing_id
            )
        )
    )
    partial_conversion = replace(converted, source_map=source_map)
    partial = write_preview(
        partial_conversion,
        model,
        root / "partial",
        options=PreviewOptions(allow_partial=True),
    )
    manifest = _manifest(partial)
    missing = manifest["missing_entities"]
    if partial.report.status != "partial" or manifest["preview"]["partial"] is not True:
        raise RuntimeError("partial preview is not explicitly marked")
    if len(missing) != 1 or missing[0]["entity_id"] != missing_id:
        raise RuntimeError("partial preview does not list its missing face")
    html = partial.index_path.read_text(encoding="utf-8")
    javascript = (partial.directory / "viewer.js").read_text(encoding="utf-8")
    if "partial-banner" not in html or "missing_entities" not in javascript:
        raise RuntimeError("bundled UI lacks the visual partial warning/list path")
    return {
        "status": "passed",
        "preview_status": partial.report.status,
        "missing_entities": missing,
        "visual_warning_present": True,
    }


def _asset_gate() -> dict[str, object]:
    from hashlib import sha256

    static = ROOT / "src/parasolid_kit/interop/preview/static"
    observed = {
        name: sha256((static / name).read_bytes()).hexdigest() for name in STATIC_ASSET_NAMES
    }
    if observed != STATIC_ASSET_SHA256:
        raise RuntimeError("bundled viewer asset hashes differ from the reviewed allowlist")
    combined = b"\n".join((static / name).read_bytes() for name in STATIC_ASSET_NAMES)
    if b"https://" in combined or b"http://" in combined:
        raise RuntimeError("bundled viewer assets contain an external URL")
    if b"innerHTML" in combined:
        raise RuntimeError("bundled viewer uses unsafe HTML insertion")
    return {
        "status": "passed",
        "version": "1.0.0",
        "license": "MIT",
        "sha256": observed,
        "cdn_required": False,
        "node_runtime_required": False,
        "vtk_runtime_required": False,
    }


def _chrome_path(requested: Path | None) -> Path | None:
    if requested is not None:
        return requested.expanduser().resolve()
    for name in ("google-chrome", "chromium", "chromium-browser"):
        candidate = shutil.which(name)
        if candidate is not None:
            return Path(candidate)
    return None


def _browser_gate(result: Any, chrome: Path | None, *, skip: bool) -> dict[str, object]:
    if skip:
        return {"status": "skipped_by_request"}
    if chrome is None or not chrome.is_file():
        raise FileNotFoundError("Chrome/Chromium is required unless --skip-browser is passed")
    with create_preview_server(result.directory) as server:
        server.start()
        command = [
            str(chrome),
            "--headless=new",
            "--disable-dev-shm-usage",
            "--enable-unsafe-swiftshader",
            "--use-angle=swiftshader-webgl",
            "--window-size=1280,900",
            "--virtual-time-budget=5000",
            "--dump-dom",
            f"{server.url}?self-test=1",
        ]
        if hasattr(os, "geteuid") and os.geteuid() == 0:
            command.insert(1, "--no-sandbox")
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
        )
    if completed.returncode != 0:
        raise RuntimeError(
            f"headless browser failed with exit {completed.returncode}: {completed.stderr}"
        )
    passed = re.search(
        r'<output[^>]*id="self-test"[^>]*data-status="passed"',
        completed.stdout,
    )
    if passed is None or 'data-viewer-ready="true"' not in completed.stdout:
        match = re.search(r'<output[^>]*id="self-test"[^>]*>.*?</output>', completed.stdout)
        detail = "self-test output absent" if match is None else match.group(0)
        raise RuntimeError(f"headless face-picking/source-map self-test failed: {detail}")
    return {
        "status": "passed",
        "browser": str(chrome),
        "real_canvas_click": True,
        "face_source_mapping": True,
        "edge_source_mapping": True,
    }


def main() -> int:
    arguments = _arguments()
    try:
        fixtures = runpy.run_path(str(SYNTHETIC_FIXTURES))
        box_model = fixtures["make_box_model"]()
        cylinder_model = fixtures["make_cylinder_hole_model"]()
        box = to_occt(
            box_model,
            source_unit="mm",
            source_identity="/private/not-for-preview/box.x_t",
        )
        cylinder = to_occt(
            cylinder_model,
            source_unit="mm",
            source_identity="/private/not-for-preview/cylinder-hole.x_b",
        )
        with tempfile.TemporaryDirectory(prefix="parasolid-kit-i6-") as temporary:
            root = Path(temporary)
            first_box = write_preview(box, box_model, root / "box")
            second_box = write_preview(box, box_model, root / "box-repeat")
            cylinder_preview = write_preview(cylinder, cylinder_model, root / "cylinder-hole")
            if first_box.glb_path.read_bytes() != second_box.glb_path.read_bytes():
                raise RuntimeError("repeated box GLB output is not deterministic")
            if first_box.manifest_path.read_bytes() != second_box.manifest_path.read_bytes():
                raise RuntimeError("repeated box manifest output is not deterministic")
            report = {
                "status": "passed",
                "box": _inspect(first_box, len(box_model.faces)),
                "cylinder_hole": _inspect(cylinder_preview, len(cylinder_model.faces)),
                "deterministic": True,
                "partial": _partial_gate(box_model, box, root),
                "assets": _asset_gate(),
                "browser": _browser_gate(
                    first_box,
                    _chrome_path(arguments.chrome),
                    skip=arguments.skip_browser,
                ),
            }
        failed = False
    except Exception as error:
        report = {
            "status": "failed",
            "error_type": type(error).__name__,
            "error": str(error),
        }
        failed = True
    print(json.dumps(report, indent=2, sort_keys=True))
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
