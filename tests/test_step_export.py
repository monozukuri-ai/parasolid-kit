from __future__ import annotations

import importlib.util
import json
import sys
from dataclasses import replace
from pathlib import Path

import pytest

from parasolid_kit.interop import InteropDependencyError, InteropLimits, StepExportError
from parasolid_kit.interop.occt import (
    ConversionReport,
    InteropUsage,
    NamedCounts,
    OcctConversionOptions,
    OcctConversionResult,
    OcctMetrics,
    SourceShapeMap,
    to_occt,
    write_step,
)
from tests._occt_fixtures import make_box_model, make_cylinder_hole_model

HAS_OCP = importlib.util.find_spec("OCP") is not None


def _fake_result(*, complete: bool) -> OcctConversionResult:
    counts = NamedCounts.from_dict({"faces": 6, "shells": 1, "solids": 1})
    report = ConversionReport(
        schema_version=1,
        producer="test",
        parser_version="test",
        ocp_distribution=None,
        ocp_version=None,
        source_identity="/private/source/model.x_t",
        source_format="text",
        schema_key="synthetic",
        options=OcctConversionOptions(source_unit="mm"),
        source_complete=complete,
        conversion_complete=complete,
        occt_valid=complete,
        input_topology=NamedCounts.from_dict({"bodies": 1, "faces": 6}),
        input_curve_kinds=NamedCounts.from_dict({"line": 12}),
        input_surface_kinds=NamedCounts.from_dict({"plane": 6}),
        output_topology=counts,
        metrics=OcctMetrics((0.0, 0.0, 0.0, 40.0, 30.0, 20.0), 5200.0, 24000.0),
        mapping_relation_count=0,
        generated_topology_count=0,
        diagnostics=(),
        limits=InteropLimits(),
        usage=InteropUsage(),
    )
    return OcctConversionResult(
        shape=object(),
        subshapes=(),
        source_map=SourceShapeMap(()),
        report=report,
    )


def test_step_api_imports_without_loading_optional_runtime() -> None:
    assert callable(write_step)
    assert write_step.__module__ == "parasolid_kit.interop.occt.step"


def test_step_export_rejects_incomplete_results_before_runtime_or_output(
    tmp_path: Path,
) -> None:
    output = tmp_path / "incomplete.step"
    ocp_modules_before = {name for name in sys.modules if name == "OCP" or name.startswith("OCP.")}

    with pytest.raises(StepExportError) as captured:
        write_step(_fake_result(complete=False), output)

    assert captured.value.diagnostic.code == "step.incomplete_conversion"
    assert captured.value.report is None
    assert not output.exists()
    assert not Path(f"{output}.conversion.json").exists()
    assert {
        name for name in sys.modules if name == "OCP" or name.startswith("OCP.")
    } == ocp_modules_before


def test_step_export_rejects_an_existing_output_before_optional_runtime(
    tmp_path: Path,
) -> None:
    output = tmp_path / "existing.step"
    output.write_text("caller-owned", encoding="utf-8")
    ocp_modules_before = {name for name in sys.modules if name == "OCP" or name.startswith("OCP.")}

    with pytest.raises(StepExportError) as captured:
        write_step(_fake_result(complete=True), output)

    assert captured.value.diagnostic.code == "step.output_exists"
    assert output.read_text(encoding="utf-8") == "caller-owned"
    assert {
        name for name in sys.modules if name == "OCP" or name.startswith("OCP.")
    } == ocp_modules_before


@pytest.mark.skipif(HAS_OCP, reason="exercises the parser-only installation")
def test_step_export_preserves_the_actionable_missing_profile_error(tmp_path: Path) -> None:
    output = tmp_path / "missing-runtime.step"

    with pytest.raises(InteropDependencyError) as captured:
        write_step(_fake_result(complete=True), output)

    assert captured.value.diagnostic.code == "interop.missing_dependency"
    assert captured.value.diagnostic.details["required_extra"] == "occt"
    assert not output.exists()
    assert not tuple(tmp_path.glob(".*.tmp"))


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_box_step_export_is_ap242_and_cold_reimported_with_a_private_sidecar(
    tmp_path: Path,
) -> None:
    result = to_occt(
        make_box_model(),
        source_unit="mm",
        source_identity="/private/customer/box.x_t",
    )
    output = tmp_path / "box.step"

    exported = write_step(result, output)
    sidecar_text = exported.sidecar_path.read_text(encoding="utf-8")
    sidecar = json.loads(sidecar_text)

    assert exported.report.status == "validated"
    assert exported.report.validation is not None
    assert exported.report.validation.passed is True
    assert exported.report.validation.process_isolated is True
    assert exported.report.validation.expected_topology.to_dict() == {
        "bodies": 1,
        "faces": 6,
    }
    assert exported.report.validation.actual_topology.to_dict() == {
        "bodies": 1,
        "faces": 6,
    }
    assert exported.report.validation.metrics_match is True
    assert sidecar["step"]["schema"] == "AP242"
    assert sidecar["step"]["output_unit"] == "mm"
    assert sidecar["conversion"]["source_identity"] is None
    assert "/private/customer" not in sidecar_text
    assert sidecar["source_map"]["relations"]
    assert sidecar["conversion_effects"]["source_map_relation_counts"] == {
        "direct": 85,
        "generated": 0,
        "merged": 0,
        "split": 0,
    }
    assert sidecar["validation"]["reader"]["done"] is True
    assert "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING" in output.read_text(encoding="utf-8")


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
@pytest.mark.parametrize(
    ("output_unit", "declared_unit"),
    [
        ("m", "metre"),
        ("cm", "centimetre"),
        ("mm", "millimetre"),
        ("in", "INCH"),
        ("ft", "FOOT"),
    ],
)
def test_step_output_unit_does_not_change_physical_size(
    tmp_path: Path,
    output_unit: str,
    declared_unit: str,
) -> None:
    result = to_occt(
        make_box_model(0.04, 0.03, 0.02),
        source_unit="m",
        target_unit="m",
        source_identity=f"sha256:{'a' * 64}",
    )

    exported = write_step(
        result,
        tmp_path / f"box-{output_unit}.step",
        output_unit=output_unit,  # type: ignore[arg-type]
    )
    validation = exported.report.validation
    assert validation is not None

    assert exported.report.geometry_scale_to_mm == 1000.0
    assert validation.passed is True
    assert validation.declared_length_units == (declared_unit,)
    assert validation.metrics_mm.bounding_box is not None
    assert validation.metrics_mm.bounding_box[3:] == pytest.approx(
        (40.0000001, 30.0000001, 20.0000001), abs=2.0e-7
    )
    sidecar = json.loads(exported.sidecar_path.read_text(encoding="utf-8"))
    assert sidecar["source"]["sha256"] == "a" * 64
    assert sidecar["step"]["conversion_target_unit"] == "m"
    assert sidecar["step"]["output_unit"] == output_unit


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_cylinder_step_preserves_body_face_and_metrics_without_edge_identity(
    tmp_path: Path,
) -> None:
    converted = to_occt(make_cylinder_hole_model(), source_unit="mm")

    exported = write_step(converted, tmp_path / "cylinder.step", output_unit="m")
    validation = exported.report.validation
    assert validation is not None

    assert validation.passed is True
    assert validation.expected_topology.to_dict() == {"bodies": 1, "faces": 4}
    assert validation.actual_topology.to_dict() == {"bodies": 1, "faces": 4}
    assert "edges" not in validation.expected_topology.to_dict()
    assert {item.metric for item in validation.metric_comparisons} == {
        "bounding_box",
        "surface_area",
        "volume",
    }
    sidecar = json.loads(exported.sidecar_path.read_text(encoding="utf-8"))
    relation_counts = sidecar["conversion_effects"]["source_map_relation_counts"]
    assert relation_counts["split"] > 0
    assert relation_counts["merged"] > 0
    assert relation_counts["generated"] > 0


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_validation_mismatch_keeps_only_a_partial_report_and_no_final_files(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from parasolid_kit.interop.occt import step

    converted = to_occt(make_box_model(), source_unit="mm")
    output = tmp_path / "mismatch.step"

    monkeypatch.setattr(
        step,
        "_run_cold_reimport",
        lambda _path: {
            "reader_status": "RetDone",
            "reader_done": True,
            "candidate_roots": 1,
            "transferred_roots": 1,
            "shape_count": 1,
            "nonempty": True,
            "occt_valid": True,
            "topology": {"solids": 1, "shells": 1, "faces": 6},
            "metrics": {
                "bounding_box": [0.0, 0.0, 0.0, 400.0, 300.0, 200.0],
                "surface_area": 520000.0,
                "volume": 24000000.0,
            },
        },
    )

    with pytest.raises(StepExportError) as captured:
        write_step(converted, output)

    assert captured.value.diagnostic.code == "step.reimport_mismatch"
    assert captured.value.report is not None
    assert captured.value.report.status == "validation_failed"
    assert captured.value.report.validation is not None
    assert captured.value.report.validation.passed is False
    assert not output.exists()
    assert not Path(f"{output}.conversion.json").exists()


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_reader_failure_is_distinct_from_a_reimport_mismatch(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from parasolid_kit.interop.occt import step

    converted = to_occt(make_box_model(), source_unit="mm")
    output = tmp_path / "unreadable.step"
    monkeypatch.setattr(
        step,
        "_run_cold_reimport",
        lambda _path: {
            "reader_status": "RetFail",
            "reader_done": False,
            "candidate_roots": 0,
            "transferred_roots": 0,
            "shape_count": 0,
            "nonempty": False,
            "occt_valid": False,
            "declared_length_units": [],
            "topology": {},
            "metrics": {},
        },
    )

    with pytest.raises(StepExportError) as captured:
        write_step(converted, output)

    assert captured.value.diagnostic.code == "step.reimport_failed"
    assert captured.value.report is not None
    assert captured.value.report.status == "validation_failed"
    assert not output.exists()


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_validation_can_only_be_skipped_explicitly(tmp_path: Path) -> None:
    converted = to_occt(make_box_model(), source_unit="mm")

    exported = write_step(converted, tmp_path / "unvalidated.step", validate=False)
    sidecar = json.loads(exported.sidecar_path.read_text(encoding="utf-8"))

    assert exported.report.status == "written_unvalidated"
    assert exported.report.validation is None
    assert sidecar["validation_requested"] is False
    assert sidecar["validation"] is None


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_output_limit_and_overwrite_contract_leave_no_partial_artifacts(
    tmp_path: Path,
) -> None:
    converted = to_occt(make_box_model(), source_unit="mm")
    limited_output = tmp_path / "limited.step"

    with pytest.raises(StepExportError) as captured:
        write_step(
            converted,
            limited_output,
            limits=replace(converted.report.limits, max_output_bytes=100),
        )
    assert captured.value.diagnostic.code == "step.limit_exceeded"
    assert not limited_output.exists()
    assert not Path(f"{limited_output}.conversion.json").exists()

    output = tmp_path / "replace.step"
    output.write_text("old-step", encoding="utf-8")
    sidecar = Path(f"{output}.conversion.json")
    sidecar.write_text("old-sidecar", encoding="utf-8")
    with pytest.raises(StepExportError) as existing:
        write_step(converted, output)
    assert existing.value.diagnostic.code == "step.output_exists"
    assert output.read_text(encoding="utf-8") == "old-step"
    assert sidecar.read_text(encoding="utf-8") == "old-sidecar"

    exported = write_step(converted, output, overwrite=True)
    assert exported.report.status == "validated"
    assert output.read_text(encoding="utf-8").startswith("ISO-10303-21;")
    assert json.loads(sidecar.read_text(encoding="utf-8"))["status"] == "validated"


@pytest.mark.skipif(not HAS_OCP, reason="requires the optional OCCT profile")
def test_repeated_export_is_byte_reproducible(tmp_path: Path) -> None:
    converted = to_occt(make_box_model(), source_unit="mm")
    output = tmp_path / "reproducible.step"
    sidecar = Path(f"{output}.conversion.json")
    output.write_text("placeholder", encoding="utf-8")
    sidecar.write_text("placeholder", encoding="utf-8")

    first = write_step(converted, output, overwrite=True)
    first_step = output.read_bytes()
    first_sidecar = sidecar.read_bytes()
    second = write_step(converted, output, overwrite=True)

    assert output.read_bytes() == first_step
    assert sidecar.read_bytes() == first_sidecar
    assert first.report.artifact.sha256 == second.report.artifact.sha256
