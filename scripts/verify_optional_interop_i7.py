#!/usr/bin/env python3
"""Verify I7 parse/coverage, exact OCCT geometry, and AP242 cold-reimport gates."""

from __future__ import annotations

import json
import runpy
import sys
import tempfile
from dataclasses import replace
from pathlib import Path
from typing import Any

from parasolid_kit import CurveKind, NurbsCurve, SurfaceKind
from parasolid_kit.interop import (
    InteropError,
    OcctConversionError,
    installed_interop_distributions,
)
from parasolid_kit.interop.occt import geometry_coverage, to_occt, write_step
from parasolid_kit.interop.preview import write_preview

ROOT = Path(__file__).resolve().parents[1]
SYNTHETIC_FIXTURES = ROOT / "tests" / "_occt_fixtures.py"


def _cases(fixtures: dict[str, Any]) -> tuple[tuple[str, Any, str, str], ...]:
    curves = tuple(
        (
            f"curve-{kind.value}",
            fixtures["make_analytic_curve_sheet_model"](kind),
            "curve",
            kind.value,
        )
        for kind in (
            CurveKind.ELLIPSE,
            CurveKind.PARABOLA,
            CurveKind.HYPERBOLA,
            CurveKind.TRIMMED,
            CurveKind.NURBS,
        )
    )
    surfaces = (
        ("surface-cone", fixtures["make_cone_frustum_model"](), "surface", "cone"),
        (
            "surface-sphere",
            fixtures["make_closed_analytic_surface_model"](SurfaceKind.SPHERE),
            "surface",
            "sphere",
        ),
        (
            "surface-torus",
            fixtures["make_closed_analytic_surface_model"](SurfaceKind.TORUS),
            "surface",
            "torus",
        ),
        (
            "surface-nurbs",
            fixtures["make_nurbs_surface_model"](),
            "surface",
            "nurbs",
        ),
        (
            "surface-offset",
            fixtures["make_nurbs_surface_model"](offset=2.0),
            "surface",
            "offset",
        ),
    )
    return (*curves, *surfaces)


def _coverage_gate() -> dict[str, object]:
    rows = geometry_coverage()
    if "OCP" in sys.modules or "cadquery" in sys.modules:
        raise RuntimeError("coverage import eagerly loaded an optional runtime")
    unsupported = {(item.category, item.kind) for item in rows if item.occt == "unsupported"}
    required_unsupported = {
        ("surface", "blended_edge"),
        ("surface", "blend_boundary"),
    }
    if not required_unsupported <= unsupported:
        raise RuntimeError("geometry coverage inferred unsupported blend construction")
    conditional = {(item.category, item.kind) for item in rows if item.occt == "conditional"}
    if not {("curve", "surface_parametric"), ("curve", "intersection")} <= conditional:
        raise RuntimeError("parametric geometry must retain its conditional conversion gates")
    return {
        "status": "passed",
        "row_count": len(rows),
        "unsupported_without_inference": sorted(
            f"{category}:{kind}" for category, kind in required_unsupported
        ),
    }


def _rational_gate(fixtures: dict[str, Any]) -> dict[str, object]:
    model = fixtures["make_analytic_curve_sheet_model"](CurveKind.NURBS)
    primary = model.curves[0]
    definition = primary.definition
    if not isinstance(definition, NurbsCurve):
        raise RuntimeError("NURBS fixture did not expose a NurbsCurve")
    rational = replace(
        primary,
        definition=replace(
            definition,
            rational=True,
            vertex_dimension=4,
            control_vertices=tuple((*values, 1.0) for values in definition.control_vertices),
        ),
    )
    try:
        to_occt(replace(model, curves=(rational, *model.curves[1:])), source_unit="mm")
    except OcctConversionError as error:
        if error.diagnostic.code != "occt.unsupported_curve":
            raise RuntimeError("rational NURBS failed with the wrong diagnostic") from error
        if error.diagnostic.details.get("rational") is not True:
            raise RuntimeError("rational NURBS diagnostic omitted its exact reason") from error
        return {"status": "rejected_before_construction", "diagnostic": error.diagnostic.to_dict()}
    raise RuntimeError("rational NURBS was accepted without an established storage contract")


def main() -> int:
    try:
        fixtures = runpy.run_path(str(SYNTHETIC_FIXTURES))
        coverage = _coverage_gate()
        reports: dict[str, object] = {}
        with tempfile.TemporaryDirectory(prefix="parasolid-kit-i7-") as temporary:
            root = Path(temporary)
            for name, model, category, kind in _cases(fixtures):
                converted = to_occt(model, source_unit="mm")
                counts = (
                    converted.report.input_curve_kinds
                    if category == "curve"
                    else converted.report.input_surface_kinds
                ).to_dict()
                if counts.get(kind, 0) < 1:
                    raise RuntimeError(f"{name} conversion report omitted its source kind")
                exported = write_step(converted, root / f"{name}.step")
                validation = exported.report.validation
                if validation is None or not validation.passed or not validation.process_isolated:
                    raise RuntimeError(f"{name} did not pass isolated AP242 reimport")
                preview = write_preview(converted, model, root / f"{name}.preview")
                if preview.report.status != "complete" or not preview.report.glb_validation.valid:
                    raise RuntimeError(f"{name} did not pass the bundled viewer gate")
                reports[name] = {
                    "geometry_kind": kind,
                    "conversion_complete": converted.report.conversion_complete,
                    "occt_valid": converted.report.occt_valid,
                    "output_topology": converted.report.output_topology.to_dict(),
                    "metrics": converted.report.metrics.to_dict(),
                    "mapping_relation_count": converted.report.mapping_relation_count,
                    "step_status": exported.report.status,
                    "step_topology": validation.actual_topology.to_dict(),
                    "step_metrics_match": validation.metrics_match,
                    "preview_status": preview.report.status,
                    "preview_faces": preview.report.face_primitive_count,
                    "preview_edges": preview.report.edge_primitive_count,
                }
        cadquery_reports: dict[str, object] | None = None
        if "cadquery" in installed_interop_distributions():
            from parasolid_kit.interop.cadquery import to_cadquery

            cadquery_reports = {}
            for name, model, _category, _kind in _cases(fixtures):
                shape = to_cadquery(model, source_unit="mm")
                if not shape.isValid():
                    raise RuntimeError(f"{name} CadQuery wrapper is invalid")
                cadquery_reports[name] = {
                    "shape_type": type(shape).__name__,
                    "faces": len(shape.Faces()),
                    "area": float(shape.Area()),
                    "solid_volume": sum(float(solid.Volume()) for solid in shape.Solids()),
                }
        rational = _rational_gate(fixtures)
        report = {
            "status": "passed",
            "coverage": coverage,
            "geometry": reports,
            "cadquery": cadquery_reports,
            "rational_nurbs": rational,
        }
    except (OSError, RuntimeError, ValueError, InteropError) as error:
        report = {"status": "failed", "error": str(error)}
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
        return 1
    print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
