from __future__ import annotations

import importlib.util
import sys
from dataclasses import replace
from functools import partial
from pathlib import Path
from typing import Any

import pytest

from parasolid_kit import (
    CurveKind,
    IntersectionCurve,
    NurbsCurve,
    NurbsSurface,
    SurfaceKind,
    SurfaceParametricCurve,
    TrimmedCurve,
)
from parasolid_kit.interop import InteropDependencyError, OcctConversionError
from parasolid_kit.interop.occt import (
    GEOMETRY_COVERAGE,
    geometry_coverage,
    render_geometry_coverage_markdown,
    to_occt,
    write_step,
)
from parasolid_kit.interop.preview import write_preview
from tests._occt_fixtures import (
    make_analytic_curve_sheet_model,
    make_closed_analytic_surface_model,
    make_cone_frustum_model,
    make_nurbs_surface_model,
)

HAS_OCP = importlib.util.find_spec("OCP") is not None
ROOT = Path(__file__).resolve().parents[1]
DOC_BEGIN = "<!-- BEGIN GENERATED I7 GEOMETRY COVERAGE -->"
DOC_END = "<!-- END GENERATED I7 GEOMETRY COVERAGE -->"

I7_CASES: tuple[tuple[str, Any, str, str], ...] = (
    (
        "ellipse",
        partial(make_analytic_curve_sheet_model, CurveKind.ELLIPSE),
        "curve",
        CurveKind.ELLIPSE.value,
    ),
    (
        "parabola",
        partial(make_analytic_curve_sheet_model, CurveKind.PARABOLA),
        "curve",
        CurveKind.PARABOLA.value,
    ),
    (
        "hyperbola",
        partial(make_analytic_curve_sheet_model, CurveKind.HYPERBOLA),
        "curve",
        CurveKind.HYPERBOLA.value,
    ),
    (
        "trimmed-curve",
        partial(make_analytic_curve_sheet_model, CurveKind.TRIMMED),
        "curve",
        CurveKind.TRIMMED.value,
    ),
    (
        "nurbs-curve",
        partial(make_analytic_curve_sheet_model, CurveKind.NURBS),
        "curve",
        CurveKind.NURBS.value,
    ),
    ("cone", make_cone_frustum_model, "surface", SurfaceKind.CONE.value),
    (
        "sphere",
        partial(make_closed_analytic_surface_model, SurfaceKind.SPHERE),
        "surface",
        SurfaceKind.SPHERE.value,
    ),
    (
        "torus",
        partial(make_closed_analytic_surface_model, SurfaceKind.TORUS),
        "surface",
        SurfaceKind.TORUS.value,
    ),
    ("nurbs-surface", make_nurbs_surface_model, "surface", SurfaceKind.NURBS.value),
    (
        "offset-surface",
        partial(make_nurbs_surface_model, offset=2.0),
        "surface",
        SurfaceKind.OFFSET.value,
    ),
)


def test_geometry_coverage_is_enum_complete_immutable_and_dependency_free() -> None:
    optional_before = {
        name for name in sys.modules if name == "OCP" or name.startswith(("OCP.", "cadquery"))
    }

    assert geometry_coverage() is GEOMETRY_COVERAGE
    assert {item.kind for item in GEOMETRY_COVERAGE if item.category == "curve"} == {
        item.value for item in CurveKind
    }
    assert {item.kind for item in GEOMETRY_COVERAGE if item.category == "surface"} == {
        item.value for item in SurfaceKind
    }
    assert all(
        item.step == "unsupported" or item.occt != "unsupported" for item in GEOMETRY_COVERAGE
    )
    assert all(
        item.parser != "unsupported" for item in GEOMETRY_COVERAGE if item.kind != "unsupported"
    )
    assert {
        name for name in sys.modules if name == "OCP" or name.startswith(("OCP.", "cadquery"))
    } == optional_before


def test_format_support_embeds_the_machine_rendered_coverage_table() -> None:
    text = (ROOT / "docs" / "format-support.md").read_text(encoding="utf-8")
    assert text.count(DOC_BEGIN) == 1
    assert text.count(DOC_END) == 1
    embedded = text.split(DOC_BEGIN, 1)[1].split(DOC_END, 1)[0].strip()

    assert embedded == render_geometry_coverage_markdown()


@pytest.mark.skipif(HAS_OCP, reason="exercises dispatch before the optional runtime is installed")
@pytest.mark.parametrize(("_name", "factory", "_category", "_kind"), I7_CASES)
def test_i7_supported_geometry_reaches_the_guarded_optional_runtime(
    _name: str,
    factory: Any,
    _category: str,
    _kind: str,
) -> None:
    with pytest.raises(InteropDependencyError) as captured:
        to_occt(factory(), source_unit="mm")

    assert captured.value.diagnostic.code == "interop.missing_dependency"


def test_i7_keeps_pcurve_intersection_and_rational_nurbs_explicitly_unsupported() -> None:
    model = make_analytic_curve_sheet_model(CurveKind.NURBS)
    primary = model.curves[0]
    pcurve = replace(
        primary,
        kind=CurveKind.SURFACE_PARAMETRIC,
        definition=SurfaceParametricCurve(
            surface=1,
            parameter_curve=2,
            original_curve=None,
            tolerance_to_original=None,
        ),
    )
    with pytest.raises(OcctConversionError) as pcurve_error:
        to_occt(replace(model, curves=(pcurve, *model.curves[1:])), source_unit="mm")
    assert pcurve_error.value.diagnostic.code == "occt.unsupported_curve"
    assert pcurve_error.value.diagnostic.details["geometry_kind"] == "surface_parametric"

    intersection = replace(
        primary,
        kind=CurveKind.INTERSECTION,
        definition=IntersectionCurve(
            surfaces=(1, 1),
            chart=primary.source,
            start=primary.source,
            end=primary.source,
            intersection_data=None,
        ),
    )
    with pytest.raises(OcctConversionError) as intersection_error:
        to_occt(replace(model, curves=(intersection, *model.curves[1:])), source_unit="mm")
    assert intersection_error.value.diagnostic.code == "occt.unsupported_curve"
    assert intersection_error.value.diagnostic.details["geometry_kind"] == "intersection"

    definition = primary.definition
    assert isinstance(definition, NurbsCurve)
    rational = replace(
        primary,
        definition=replace(
            definition,
            rational=True,
            vertex_dimension=4,
            control_vertices=tuple((*values, 1.0) for values in definition.control_vertices),
        ),
    )
    with pytest.raises(OcctConversionError) as rational_error:
        to_occt(replace(model, curves=(rational, *model.curves[1:])), source_unit="mm")
    assert rational_error.value.diagnostic.code == "occt.unsupported_curve"
    assert rational_error.value.diagnostic.details["rational"] is True


def test_trimmed_basis_missing_and_cycles_are_rejected_before_occt_import() -> None:
    model = make_analytic_curve_sheet_model(CurveKind.TRIMMED)
    primary = model.curves[0]
    definition = primary.definition
    assert isinstance(definition, TrimmedCurve)
    missing = replace(primary, definition=replace(definition, basis_curve=999))

    with pytest.raises(OcctConversionError) as missing_error:
        to_occt(replace(model, curves=(missing, *model.curves[1:])), source_unit="mm")
    assert missing_error.value.diagnostic.code == "occt.invalid_reference"
    assert missing_error.value.diagnostic.details["target_id"] == 999

    basis = model.curves[-1]
    cycle_basis = replace(
        basis,
        kind=CurveKind.TRIMMED,
        definition=replace(definition, basis_curve=1),
    )
    with pytest.raises(OcctConversionError) as cycle_error:
        to_occt(replace(model, curves=(*model.curves[:-1], cycle_basis)), source_unit="mm")
    assert cycle_error.value.diagnostic.code == "occt.invalid_reference"
    assert "cycle" in cycle_error.value.diagnostic.message


def test_malformed_nurbs_is_rejected_before_occt_import() -> None:
    curve_model = make_analytic_curve_sheet_model(CurveKind.NURBS)
    curve = curve_model.curves[0]
    curve_definition = curve.definition
    assert isinstance(curve_definition, NurbsCurve)
    malformed_curve = replace(
        curve,
        definition=replace(curve_definition, knot_multiplicities=(3, 2)),
    )

    with pytest.raises(OcctConversionError) as curve_error:
        to_occt(
            replace(curve_model, curves=(malformed_curve, *curve_model.curves[1:])),
            source_unit="mm",
        )
    assert curve_error.value.diagnostic.code == "occt.invalid_geometry"
    assert "expanded knot count" in curve_error.value.diagnostic.details["reason"]

    surface_model = make_nurbs_surface_model()
    surface = surface_model.surfaces[0]
    surface_definition = surface.definition
    assert isinstance(surface_definition, NurbsSurface)
    malformed_surface = replace(
        surface,
        definition=replace(surface_definition, u_knots=(0.0, 0.0)),
    )

    with pytest.raises(OcctConversionError) as surface_error:
        to_occt(replace(surface_model, surfaces=(malformed_surface,)), source_unit="mm")
    assert surface_error.value.diagnostic.code == "occt.invalid_geometry"
    assert "strictly increasing" in surface_error.value.diagnostic.details["reason"]


def test_periodic_nurbs_is_an_explicit_conditional_coverage_failure() -> None:
    model = make_analytic_curve_sheet_model(CurveKind.NURBS)
    curve = model.curves[0]
    definition = curve.definition
    assert isinstance(definition, NurbsCurve)
    periodic = replace(curve, definition=replace(definition, periodic=True, closed=True))

    with pytest.raises(OcctConversionError) as captured:
        to_occt(replace(model, curves=(periodic, *model.curves[1:])), source_unit="mm")

    assert captured.value.diagnostic.code == "occt.unsupported_curve"
    assert captured.value.diagnostic.details["periodic"] is True
    assert captured.value.diagnostic.details["closed"] is True


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
@pytest.mark.parametrize(("_name", "factory", "category", "kind"), I7_CASES)
def test_i7_public_fixtures_convert_as_valid_exact_shapes(
    _name: str,
    factory: Any,
    category: str,
    kind: str,
) -> None:
    converted = to_occt(factory(), source_unit="mm")
    counts = (
        converted.report.input_curve_kinds
        if category == "curve"
        else converted.report.input_surface_kinds
    ).to_dict()

    assert converted.report.conversion_complete is True
    assert converted.report.occt_valid is True
    assert counts[kind] >= 1
    assert converted.report.output_topology.to_dict()["faces"] >= 1
    assert converted.report.mapping_relation_count > 0


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
@pytest.mark.parametrize(("name", "factory", "_category", "_kind"), I7_CASES)
def test_i7_public_fixtures_pass_ap242_cold_reimport(
    tmp_path: Path,
    name: str,
    factory: Any,
    _category: str,
    _kind: str,
) -> None:
    converted = to_occt(factory(), source_unit="mm")
    exported = write_step(converted, tmp_path / f"{name}.step")

    assert exported.report.status == "validated"
    assert exported.report.validation is not None
    assert exported.report.validation.passed is True
    assert exported.report.validation.process_isolated is True
    assert exported.report.validation.topology_matches is True
    assert exported.report.validation.metrics_match is True


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
@pytest.mark.parametrize(("name", "factory", "_category", "_kind"), I7_CASES)
def test_i7_public_fixtures_remain_visible_in_the_bundled_viewer(
    tmp_path: Path,
    name: str,
    factory: Any,
    _category: str,
    _kind: str,
) -> None:
    model = factory()
    converted = to_occt(model, source_unit="mm")
    preview = write_preview(converted, model, tmp_path / name)

    assert preview.report.status == "complete"
    assert preview.report.glb_validation.valid is True
    assert preview.report.face_primitive_count == len(model.faces)
    assert preview.report.edge_primitive_count >= len(model.edges)
