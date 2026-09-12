#!/usr/bin/env python3
"""Verify I6 bounded previews, provenance, partial warnings, and browser picking."""

from __future__ import annotations

import argparse
import json
import os
import runpy
import shutil
import subprocess
import tempfile
from contextlib import ExitStack
from dataclasses import replace
from importlib import resources
from pathlib import Path
from typing import Any

from parasolid_kit.interop.occt import SourceEntityKind, SourceShapeMap, to_occt
from parasolid_kit.interop.preview import (
    ASSET_BUNDLE_VERSION,
    ASSET_LICENSE,
    STATIC_ASSET_NAMES,
    STATIC_ASSET_SHA256,
    PreviewOptions,
    write_preview,
)

ROOT = Path(__file__).resolve().parents[1]
SYNTHETIC_FIXTURES = ROOT / "tests" / "_occt_fixtures.py"


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--chrome",
        type=Path,
        help="override the Chromium supplied by the pinned Playwright installation",
    )
    parser.add_argument(
        "--skip-browser",
        action="store_true",
        help="run GLB, manifest, partial, and asset gates without the headless browser gate",
    )
    parser.add_argument(
        "--browser-output",
        type=Path,
        default=ROOT / "viewer/test-results/i6",
        help="directory for Playwright JSON and screenshots",
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

    static = resources.files("parasolid_kit.interop.preview").joinpath("static")
    observed = {
        name: sha256(static.joinpath(name).read_bytes()).hexdigest() for name in STATIC_ASSET_NAMES
    }
    if observed != STATIC_ASSET_SHA256:
        raise RuntimeError("bundled viewer asset hashes differ from the reviewed allowlist")
    # The exact hashes cover upstream's fixed HTML templates and embedded notices.
    # URL/innerHTML substrings in those bytes do not establish network or DOM safety;
    # those require browser request/CSP monitoring and source-string injection tests.
    return {
        "status": "passed",
        "version": ASSET_BUNDLE_VERSION,
        "license": ASSET_LICENSE,
        "sha256": observed,
        "cdn_required": False,
        "node_runtime_required": False,
        "vtk_runtime_required": False,
    }


def _browser_gate(
    root: Path,
    chrome: Path | None,
    *,
    skip: bool,
    output: Path,
) -> dict[str, object]:
    if skip:
        return {"status": "skipped_by_request"}
    node = shutil.which("node")
    if node is None:
        raise FileNotFoundError(
            "Node is required for the development browser gate; "
            "use --skip-browser for Python-only checks"
        )
    driver = ROOT / "viewer/tests/browser.mjs"
    if not driver.is_file():
        raise FileNotFoundError(
            "viewer test sources are required (use a checkout or unpacked sdist)"
        )
    helper = runpy.run_path(str(ROOT / "viewer/tests/serve_previews.py"))
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    if chrome is not None:
        if not chrome.is_file():
            raise FileNotFoundError(f"Chrome/Chromium not found: {chrome}")
        environment["VIEWER_CHROME"] = str(chrome)
    with ExitStack() as stack:
        config = helper["serve_previews"](root / "browser", stack)
        config_path = root / "browser-config.json"
        config_path.write_text(json.dumps(config), encoding="ascii")
        completed = subprocess.run(
            [node, str(driver), "--config", str(config_path), "--report-dir", str(output)],
            check=False,
            capture_output=True,
            text=True,
            env=environment,
            timeout=240,
        )
    if completed.returncode != 0:
        raise RuntimeError(f"Playwright viewer gate failed: {completed.stdout}\n{completed.stderr}")
    report = json.loads((output / "browser.json").read_text(encoding="utf-8"))
    if report["status"] != "passed":
        raise RuntimeError("Playwright viewer report did not pass")
    return report


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
                    root,
                    None if arguments.chrome is None else arguments.chrome.expanduser().resolve(),
                    skip=arguments.skip_browser,
                    output=arguments.browser_output,
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
