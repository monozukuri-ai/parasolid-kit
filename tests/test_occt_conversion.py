from __future__ import annotations

import importlib.util
import json
from dataclasses import FrozenInstanceError, replace
from math import isclose

import pytest

from parasolid_kit import BrepMetrics, CurveKind
from parasolid_kit.interop import InteropLimits, OcctConversionError
from parasolid_kit.interop.occt import (
    OcctConversionOptions,
    ShapeRelationKind,
    ValidationTolerances,
    to_occt,
)
from tests._occt_fixtures import make_box_model, make_cylinder_hole_model

HAS_OCP = importlib.util.find_spec("OCP") is not None


def test_occt_options_require_explicit_units_and_apply_exact_length_scale() -> None:
    options = OcctConversionOptions(source_unit="m", target_unit="mm")

    assert options.applied_scale == 1000.0
    assert options.to_dict()["source_to_metres"] == 1.0
    assert options.to_dict()["target_to_metres"] == 0.001
    with pytest.raises(FrozenInstanceError):
        options.target_unit = "cm"  # type: ignore[misc]
    with pytest.raises(ValueError, match="source_unit must be one of"):
        OcctConversionOptions(source_unit="yard")  # type: ignore[arg-type]


def test_validation_tolerances_scale_by_metric_dimension() -> None:
    tolerances = ValidationTolerances(linear_absolute=1.0e-3, relative=1.0e-4)

    assert tolerances.linear_threshold(20.0) == 2.0e-3
    assert tolerances.area_threshold(200.0) == 2.0e-2
    assert tolerances.volume_threshold(2000.0) == 0.2


def test_unsupported_geometry_fails_before_optional_runtime_import() -> None:
    model = make_box_model()
    unsupported = replace(model.curves[0], kind=CurveKind.ELLIPSE)
    model = replace(model, curves=(unsupported, *model.curves[1:]))

    with pytest.raises(OcctConversionError) as captured:
        to_occt(model, source_unit="mm")

    assert captured.value.diagnostic.code == "occt.unsupported_curve"
    report = captured.value.report
    assert report is not None
    assert report.conversion_complete is False
    assert report.ocp_version is None
    assert any(item.code == "occt.unsupported_curve" for item in report.diagnostics)


def test_entity_limit_fails_with_a_partial_conversion_report() -> None:
    model = make_box_model()

    with pytest.raises(OcctConversionError) as captured:
        to_occt(model, source_unit="mm", limits=InteropLimits(max_entities=1))

    assert captured.value.diagnostic.code == "occt.limit_exceeded"
    assert captured.value.report is not None
    assert captured.value.report.usage.entities > 1


def test_requested_healing_is_an_explicit_unsupported_operation() -> None:
    with pytest.raises(OcctConversionError) as captured:
        to_occt(make_box_model(), source_unit="mm", heal=True)

    assert captured.value.diagnostic.code == "occt.healing_unavailable"
    assert captured.value.report is not None
    assert captured.value.report.healing_requested is True
    assert captured.value.report.healing_performed is False


def test_strict_conversion_rejects_an_incomplete_source_before_occt_import() -> None:
    model = replace(make_box_model(), complete=False)

    with pytest.raises(OcctConversionError) as captured:
        to_occt(model, source_unit="mm")

    assert captured.value.diagnostic.code == "occt.source_incomplete"
    assert captured.value.report is not None
    assert captured.value.report.source_complete is False


def test_vertex_trimmed_circle_is_not_silently_treated_as_a_full_ring() -> None:
    model = make_cylinder_hole_model()
    trimmed = replace(model.edges[0], start_vertex=999)
    model = replace(model, edges=(trimmed, *model.edges[1:]))

    with pytest.raises(OcctConversionError) as captured:
        to_occt(model, source_unit="mm")

    assert captured.value.diagnostic.code == "occt.unsupported_curve"
    assert captured.value.diagnostic.details["geometry_kind"] == "trimmed_circle"


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_synthetic_box_converts_to_a_valid_shared_topology_solid() -> None:
    result = to_occt(make_box_model(), source_unit="mm")
    output = result.report.output_topology.to_dict()

    assert result.report.conversion_complete is True
    assert result.report.occt_valid is True
    assert result.report.healing_performed is False
    assert output["solids"] == 1
    assert output["faces"] == 6
    assert output["edges"] == 12
    assert output["vertices"] == 8
    assert isclose(result.report.metrics.surface_area or 0.0, 5200.0, rel_tol=1.0e-10)
    assert isclose(result.report.metrics.volume or 0.0, 24000.0, rel_tol=1.0e-10)
    json.dumps(result.to_dict())


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_periodic_cylinder_hole_preserves_many_to_many_provenance() -> None:
    result = to_occt(make_cylinder_hole_model(), source_unit="mm")
    output = result.report.output_topology.to_dict()
    relation_kinds = {item.relation for item in result.source_map.relations}

    assert result.report.occt_valid is True
    assert result.report.healing_performed is False
    assert output["solids"] == 1
    assert output["faces"] == 4
    assert output["edges"] == 6
    assert output["vertices"] == 4
    assert ShapeRelationKind.SPLIT in relation_kinds
    assert ShapeRelationKind.MERGED in relation_kinds
    assert ShapeRelationKind.GENERATED in relation_kinds
    assert result.report.generated_topology_count == 6
    generated_targets = {
        item.target_key
        for item in result.source_map.relations
        if item.relation is ShapeRelationKind.GENERATED
    }
    assert any(target.startswith("occt:edge:") for target in generated_targets)
    assert any(target.startswith("occt:vertex:") for target in generated_targets)


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_same_semantics_in_text_and_binary_models_have_matching_metrics() -> None:
    text = make_box_model()
    binary = replace(text, source_format="binary")

    text_result = to_occt(text, source_unit="mm")
    binary_result = to_occt(binary, source_unit="mm")

    assert text_result.report.metrics == binary_result.report.metrics


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_source_length_units_scale_coordinates_area_and_volume() -> None:
    result = to_occt(make_box_model(0.04, 0.03, 0.02), source_unit="m")

    assert result.report.options.applied_scale == 1000.0
    assert isclose(result.report.metrics.surface_area or 0.0, 5200.0, rel_tol=1.0e-10)
    assert isclose(result.report.metrics.volume or 0.0, 24000.0, rel_tol=1.0e-10)


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_source_metric_disagreement_fails_with_measured_partial_report() -> None:
    model = make_box_model()
    metrics = BrepMetrics(
        model.metrics.bounding_box,
        model.metrics.surface_area,
        (model.metrics.volume or 0.0) * 2.0,
    )

    with pytest.raises(OcctConversionError) as captured:
        to_occt(replace(model, metrics=metrics), source_unit="mm")

    assert captured.value.diagnostic.code == "occt.metric_mismatch"
    assert captured.value.report is not None
    assert captured.value.report.occt_valid is True
    assert captured.value.report.conversion_complete is False
    assert captured.value.report.metrics.volume == 24000.0
