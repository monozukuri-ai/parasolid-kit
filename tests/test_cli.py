from __future__ import annotations

import json
from pathlib import Path

import pytest

from parasolid_kit import cli
from parasolid_kit.cli import main
from tests.support.parasolid_binary import SyntheticXbBuilder


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
