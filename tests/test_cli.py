from __future__ import annotations

import json
from hashlib import sha256
from pathlib import Path
from types import SimpleNamespace

import pytest

from parasolid_kit import (
    BrepEntityCounts,
    BrepMetrics,
    BrepSummary,
    Diagnostic,
    DiagnosticKind,
    DiagnosticSeverity,
    SchemaKey,
    TopologyValidation,
    cli,
)
from parasolid_kit.cli import main
from tests.support.parasolid_binary import SyntheticXbBuilder


def _check_summary(*, complete: bool) -> BrepSummary:
    diagnostics = (
        ()
        if complete
        else (
            Diagnostic(
                code="geometry.unsupported_curve",
                severity=DiagnosticSeverity.WARNING,
                kind=DiagnosticKind.UNSUPPORTED,
                message="synthetic unsupported curve",
                node_type=75,
                node_id=301,
            ),
        )
    )
    return BrepSummary(
        source_format="binary",
        modeller_version=": TRANSMIT FILE created by modeller version 3000000",
        schema_key=SchemaKey.parse("SCH_3000000_30000"),
        file_size=123,
        node_count=9,
        resolved_schema_type_count=5,
        resolved_schema_field_count=12,
        complete=complete,
        counts=BrepEntityCounts(
            bodies=1,
            regions=1,
            shells=1,
            faces=6,
            loops=6,
            half_edges=24,
            edges=12,
            vertices=8,
            points=8,
            curves=12,
            surfaces=6,
        ),
        body_kind_counts=(("solid", 1),),
        curve_kind_counts=(("line", 12),),
        surface_kind_counts=(("plane", 6),),
        topology=TopologyValidation(
            valid=True,
            closed_loop_count=6,
            closed_edge_ring_count=12,
            euler_characteristic=2,
        ),
        metrics=BrepMetrics(bounding_box=None, surface_area=52.0, volume=24.0),
        document_diagnostics=(),
        brep_diagnostics=diagnostics,
    )


def test_cli_inspect_reports_a_machine_readable_header(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    path = tmp_path / "fixture.x_b"
    path.write_bytes(
        SyntheticXbBuilder(schema_name="SCH_3000000_30000", schema_max_type=None).build()
    )

    assert main(["inspect", str(path)]) == 0
    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert report["status"] == "header_valid"
    assert report["format"] == "binary"
    assert report["header"]["schema_key"] == "SCH_3000000_30000"
    assert captured.err == ""


def test_cli_rejects_ambiguous_input_without_guessing(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    path = tmp_path / "unknown.dat"
    path.write_bytes(b"not parasolid")

    assert main(["inspect", str(path)]) == 2
    captured = capsys.readouterr()
    report = json.loads(captured.err)

    assert report["status"] == "error"
    assert report["error_type"] == "ValueError"
    assert "pass --format" in report["message"]
    assert captured.out == ""


def test_cli_requires_the_exact_catalog_path(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    path = tmp_path / "fixture.x_b"
    path.write_bytes(
        SyntheticXbBuilder(schema_name="SCH_3000000_30000", schema_max_type=None).build()
    )

    assert main(["parse", str(path), "--schema-dir", str(tmp_path)]) == 2
    captured = capsys.readouterr()
    report = json.loads(captured.err)

    assert report["status"] == "error"
    assert report["error_type"] == "FileNotFoundError"
    assert report["message"].endswith("sch_30000.sch_txt")


def test_cli_compare_returns_one_for_a_valid_difference(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class Different:
        equivalent = False

        @staticmethod
        def to_dict() -> dict[str, object]:
            return {"equivalent": False, "difference_count": 1}

    monkeypatch.setattr(cli, "_document", lambda *_args: object())
    monkeypatch.setattr(cli, "compare_documents", lambda *_args, **_kwargs: Different())

    assert (
        main(
            [
                "compare",
                str(tmp_path / "left.x_t"),
                str(tmp_path / "right.x_b"),
                "--schema-dir",
                str(tmp_path),
            ]
        )
        == 1
    )
    report = json.loads(capsys.readouterr().out)

    assert report["status"] == "different"
    assert report["comparison"] == {"equivalent": False, "difference_count": 1}


def test_cli_check_is_human_readable_by_default(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    summary = _check_summary(complete=True)
    monkeypatch.setattr(
        cli,
        "read_brep",
        lambda *_args, **_kwargs: SimpleNamespace(summary=summary),
    )

    assert (
        main(
            [
                "check",
                str(tmp_path / "fixture.x_b"),
                "--schema-dir",
                str(tmp_path),
            ]
        )
        == 0
    )
    captured = capsys.readouterr()

    assert "Parasolid B-Rep check: complete" in captured.out
    assert "Format: X_B" in captured.out
    assert "bodies: 1" in captured.out
    assert "curves: line=12" in captured.out
    assert "physical length unit unknown" in captured.out
    assert "Diagnostics (0):" in captured.out
    assert captured.err == ""


def test_cli_check_json_returns_one_for_an_incomplete_mapping(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    summary = _check_summary(complete=False)
    monkeypatch.setattr(
        cli,
        "read_brep",
        lambda *_args, **_kwargs: SimpleNamespace(summary=summary),
    )

    assert (
        main(
            [
                "check",
                str(tmp_path / "fixture.x_b"),
                "--schema-dir",
                str(tmp_path),
                "--json",
            ]
        )
        == 1
    )
    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert report["status"] == "incomplete"
    assert report["summary"]["complete"] is False
    assert report["summary"]["diagnostic_count"] == 1
    assert report["summary"]["diagnostics"]["brep"][0]["code"] == ("geometry.unsupported_curve")
    assert captured.err == ""


def test_cli_check_human_reports_diagnostic_layer_and_entity(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    summary = _check_summary(complete=False)
    monkeypatch.setattr(
        cli,
        "read_brep",
        lambda *_args, **_kwargs: SimpleNamespace(summary=summary),
    )

    assert (
        main(
            [
                "check",
                str(tmp_path / "fixture.x_b"),
                "--schema-dir",
                str(tmp_path),
            ]
        )
        == 1
    )
    captured = capsys.readouterr()

    assert "[brep] warning geometry.unsupported_curve" in captured.out
    assert "node_id=301, node_type=75" in captured.out
    assert "synthetic unsupported curve" in captured.out
    assert captured.err == ""


def test_cli_export_step_keeps_conversion_and_writer_lazy(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from parasolid_kit.interop import occt
    from parasolid_kit.interop.occt import step

    source = tmp_path / "fixture.x_t"
    source.write_bytes(b"synthetic parasolid")
    output = tmp_path / "fixture.step"
    calls: dict[str, object] = {}
    output_topology = SimpleNamespace(to_dict=lambda: {"solids": 1, "faces": 6})
    metrics = SimpleNamespace(
        to_dict=lambda: {
            "bounding_box": [0, 0, 0, 40, 30, 20],
            "surface_area": 5200,
            "volume": 24000,
        }
    )
    conversion_report = SimpleNamespace(
        source_complete=True,
        conversion_complete=True,
        occt_valid=True,
        output_topology=output_topology,
        metrics=metrics,
        options=SimpleNamespace(target_unit="mm"),
    )
    converted = SimpleNamespace(report=conversion_report)
    export_report = SimpleNamespace(
        status="validated",
        step_schema="AP242",
        output_unit="ft",
        geometry_scale_to_mm=1.0,
        transfer_status="RetDone",
        write_status="RetDone",
        artifact=SimpleNamespace(to_dict=lambda: {"filename": output.name, "sha256": "a" * 64}),
        validation=SimpleNamespace(to_dict=lambda: {"passed": True, "process_isolated": True}),
    )

    monkeypatch.setattr(
        cli,
        "read_brep",
        lambda *_args, **_kwargs: SimpleNamespace(
            brep=object(),
            document=SimpleNamespace(raw_bytes=source.read_bytes()),
        ),
    )

    def fake_to_occt(brep: object, **options: object) -> object:
        calls["brep"] = brep
        calls["conversion_options"] = options
        return converted

    def fake_write_step(result: object, destination: Path, **options: object) -> object:
        calls["result"] = result
        calls["destination"] = destination
        calls["write_options"] = options
        return SimpleNamespace(
            path=destination,
            sidecar_path=Path(f"{destination}.conversion.json"),
            report=export_report,
        )

    monkeypatch.setattr(occt, "to_occt", fake_to_occt)
    monkeypatch.setattr(step, "write_step", fake_write_step)

    assert (
        main(
            [
                "export-step",
                str(source),
                str(output),
                "--schema-dir",
                str(tmp_path),
                "--source-unit",
                "m",
                "--output-unit",
                "ft",
                "--overwrite",
            ]
        )
        == 0
    )
    captured = capsys.readouterr()
    report = json.loads(captured.out)

    expected_hash = sha256(source.read_bytes()).hexdigest()
    assert calls["conversion_options"] == {
        "source_unit": "m",
        "target_unit": "mm",
        "source_identity": f"sha256:{expected_hash}",
    }
    assert calls["write_options"] == {
        "output_unit": "ft",
        "validate": True,
        "overwrite": True,
    }
    assert report["status"] == "validated"
    assert report["source_sha256"] == expected_hash
    assert report["validation"]["process_isolated"] is True
    assert captured.err == ""


def test_cli_view_writes_a_persistent_preview_without_starting_a_server(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from parasolid_kit.interop import occt, preview

    source = tmp_path / "fixture.x_t"
    source.write_bytes(b"synthetic parasolid")
    output = tmp_path / "review"
    brep = object()
    converted = object()
    calls: dict[str, object] = {}
    monkeypatch.setattr(
        cli,
        "read_brep",
        lambda *_args, **_kwargs: SimpleNamespace(
            brep=brep,
            document=SimpleNamespace(raw_bytes=source.read_bytes()),
        ),
    )

    def fake_to_occt(model: object, **options: object) -> object:
        calls["model"] = model
        calls["conversion"] = options
        return converted

    def fake_write_preview(
        result: object,
        model: object,
        destination: Path,
        **options: object,
    ) -> object:
        calls["result"] = result
        calls["preview_model"] = model
        calls["destination"] = destination
        calls["preview"] = options
        return SimpleNamespace(
            directory=destination,
            index_path=destination / "index.html",
            glb_path=destination / "preview.glb",
            manifest_path=destination / "preview.manifest.json",
            report=SimpleNamespace(to_dict=lambda: {"status": "complete"}),
        )

    monkeypatch.setattr(occt, "to_occt", fake_to_occt)
    monkeypatch.setattr(preview, "write_preview", fake_write_preview)
    monkeypatch.setattr(
        preview,
        "create_preview_server",
        lambda *_args, **_kwargs: pytest.fail("write-only must not start a server"),
    )

    assert (
        main(
            [
                "view",
                str(source),
                "--schema-dir",
                str(tmp_path),
                "--source-unit",
                "m",
                "--target-unit",
                "cm",
                "--output",
                str(output),
                "--linear-deflection",
                "0.025",
                "--angular-deflection",
                "0.2",
                "--no-edges",
                "--write-only",
                "--overwrite",
            ]
        )
        == 0
    )
    captured = capsys.readouterr()
    report = json.loads(captured.out)
    source_hash = sha256(source.read_bytes()).hexdigest()
    options = calls["preview"]["options"]  # type: ignore[index]

    assert calls["model"] is brep
    assert calls["conversion"] == {
        "source_unit": "m",
        "target_unit": "cm",
        "require_complete": True,
        "source_identity": f"sha256:{source_hash}",
    }
    assert calls["result"] is converted
    assert calls["preview_model"] is brep
    assert calls["destination"] == output
    assert options.linear_deflection == 0.025
    assert options.angular_deflection == 0.2
    assert options.include_edges is False
    assert calls["preview"]["overwrite"] is True  # type: ignore[index]
    assert report["status"] == "generated"
    assert report["source_sha256"] == source_hash
    assert report["preview"]["status"] == "complete"
    assert captured.err == ""


def test_cli_check_human_errors_do_not_change_existing_json_error_contract(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def missing(*_args: object, **_kwargs: object) -> object:
        raise FileNotFoundError("missing exact schema")

    monkeypatch.setattr(cli, "read_brep", missing)
    arguments = [
        "check",
        str(tmp_path / "fixture.x_b"),
        "--schema-dir",
        str(tmp_path),
    ]

    assert main(arguments) == 2
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err == "error: missing exact schema\n"

    assert main([*arguments, "--json"]) == 2
    captured = capsys.readouterr()
    assert captured.out == ""
    assert json.loads(captured.err)["status"] == "error"
