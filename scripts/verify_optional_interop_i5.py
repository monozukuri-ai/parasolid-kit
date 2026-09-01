#!/usr/bin/env python3
"""Verify the I5 CadQuery adapter on synthetic and caller-enabled real models."""

from __future__ import annotations

import argparse
import json
import runpy
from math import isclose
from pathlib import Path
from typing import Any

import cadquery as cq

from parasolid_kit import read_brep
from parasolid_kit.interop.cadquery import load_runtime, to_cadquery, to_cadquery_shapes
from parasolid_kit.interop.occt import to_occt

ROOT = Path(__file__).resolve().parents[1]
SYNTHETIC_FIXTURES = ROOT / "tests" / "_occt_fixtures.py"


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--schema-dir",
        type=Path,
        help="caller-owned directory containing exact sch_30000.sch_txt",
    )
    parser.add_argument("--data-dir", type=Path, default=ROOT / "data")
    parser.add_argument(
        "--require-real",
        action="store_true",
        help="fail when the external schema or real fixture gate is unavailable",
    )
    return parser.parse_args()


def _bounds(shape: cq.Shape) -> tuple[float, float, float, float, float, float]:
    box = shape.BoundingBox()
    return (box.xmin, box.ymin, box.zmin, box.xmax, box.ymax, box.zmax)


def _verify_model(model: Any, *, source_unit: str) -> dict[str, object]:
    converted = to_occt(model, source_unit=source_unit)
    shape = to_cadquery(model, source_unit=source_unit)
    body_shapes = to_cadquery_shapes(model, source_unit=source_unit)
    expected = converted.report.metrics
    tolerances = converted.report.options.validation
    expected_bounds = expected.bounding_box
    actual_bounds = _bounds(shape)
    if expected_bounds is None:
        raise RuntimeError("I5 verifier requires an OCCT bounding box")
    for reference, actual in zip(expected_bounds, actual_bounds, strict=True):
        if not isclose(
            reference,
            actual,
            rel_tol=0.0,
            abs_tol=tolerances.linear_threshold(reference),
        ):
            raise RuntimeError("CadQuery and OCCT bounding boxes differ")
    expected_area = expected.surface_area
    expected_volume = expected.volume
    if expected_area is None or not isclose(
        shape.Area(),
        expected_area,
        rel_tol=0.0,
        abs_tol=tolerances.area_threshold(expected_area),
    ):
        raise RuntimeError("CadQuery and OCCT surface areas differ")
    if expected_volume is None or not isclose(
        shape.Volume(),
        expected_volume,
        rel_tol=0.0,
        abs_tol=tolerances.volume_threshold(expected_volume),
    ):
        raise RuntimeError("CadQuery and OCCT volumes differ")
    expected_faces = converted.report.output_topology.to_dict()["faces"]
    if len(shape.Faces()) != expected_faces:
        raise RuntimeError("CadQuery and OCCT face counts differ")
    if len(body_shapes) != len(model.bodies):
        raise RuntimeError("CadQuery per-body result count differs from the source")
    if isinstance(shape, (cq.Assembly, cq.Workplane)):
        raise RuntimeError("I5 inferred assembly or feature-history semantics")
    expected_root = cq.Compound if len(model.bodies) > 1 else cq.Shape
    if not isinstance(shape, expected_root):
        raise RuntimeError("I5 returned the wrong single/multi-body result type")
    if len(model.bodies) == 1 and isinstance(shape, cq.Compound):
        raise RuntimeError("I5 retained a one-body Compound instead of its body Shape")
    return {
        "source_bodies": len(model.bodies),
        "shape_type": type(shape).__name__,
        "body_shape_types": [type(item).__name__ for item in body_shapes],
        "face_count": len(shape.Faces()),
        "bounding_box": list(actual_bounds),
        "surface_area": shape.Area(),
        "volume": shape.Volume(),
        "matches_occt": True,
        "assembly_inferred": False,
        "feature_history_inferred": False,
    }


def _synthetic_gate() -> dict[str, object]:
    fixtures = runpy.run_path(str(SYNTHETIC_FIXTURES))
    return {
        "status": "passed",
        "single_box": _verify_model(fixtures["make_box_model"](), source_unit="mm"),
        "two_boxes": _verify_model(fixtures["make_two_box_model"](), source_unit="mm"),
    }


def _real_gate(schema_dir: Path | None, data_dir: Path) -> dict[str, object]:
    if schema_dir is None:
        return {
            "status": "blocked_external_catalog",
            "required_catalog": "sch_30000.sch_txt",
        }
    schema_dir = schema_dir.expanduser().resolve()
    catalog = schema_dir / "sch_30000.sch_txt"
    if not catalog.is_file():
        return {
            "status": "blocked_external_catalog",
            "required_catalog": str(catalog),
        }
    data_dir = data_dir.expanduser().resolve()
    inputs = (
        ("box-x-t", data_dir / "onshape-box-v30.x_t"),
        ("box-x-b", data_dir / "onshape-box-v30.x_b"),
        ("cylinder-hole-x-t", data_dir / "onshape-cylinder-hole-v30.x_t"),
        ("cylinder-hole-x-b", data_dir / "onshape-cylinder-hole-v30.x_b"),
    )
    reports: dict[str, object] = {}
    for name, source in inputs:
        reports[name] = _verify_model(
            read_brep(source, schema_dir=schema_dir).brep,
            source_unit="m",
        )
    return {"status": "passed", "inputs": reports}


def main() -> int:
    arguments = _arguments()
    try:
        runtime = load_runtime()
        synthetic = _synthetic_gate()
        real = _real_gate(arguments.schema_dir, arguments.data_dir)
        failed = real["status"] != "passed" and arguments.require_real
        report = {
            "status": "failed" if failed else "passed",
            "cadquery_version": getattr(runtime, "__version__", None),
            "synthetic": synthetic,
            "real": real,
        }
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
