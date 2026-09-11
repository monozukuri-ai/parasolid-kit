"""A stale run, missing private evidence or changed artifact must block publishing."""

from __future__ import annotations

import copy
from pathlib import Path

import pytest

pytest.importorskip("tomllib", reason="maintainer release tooling requires Python 3.11+")

from scripts.verify_release import (
    GATES,
    REPOSITORY,
    artifact_hashes,
    validate_receipt,
    verify_artifacts,
    versions,
)

SHA = "a" * 40


def candidate():
    python, rust = versions()
    receipt = {
        "schema_version": 1,
        "repository": REPOSITORY,
        "source_sha": SHA,
        "candidate_run_id": 123,
        "candidate_run_attempt": 1,
        "python_version": python,
        "rust_version": rust,
        "gates": {name: {"status": "passed", "report_sha256": "b" * 64} for name in GATES},
    }
    run = {
        "id": 123,
        "run_attempt": 1,
        "head_sha": SHA,
        "status": "completed",
        "conclusion": "success",
        "path": ".github/workflows/release.yml",
        "repository": {"full_name": REPOSITORY},
        "head_repository": {"full_name": REPOSITORY},
        "event": "push",
        "head_branch": "release/0.1.0rc1",
    }
    return receipt, run


def artifacts(directory: Path) -> Path:
    python, rust = versions()
    names = [
        f"parasolid_kit-{python}-cp310-abi3-{platform}.whl"
        for platform in (
            "manylinux_2_17_x86_64.manylinux2014_x86_64",
            "win_amd64",
            "macosx_10_12_x86_64",
            "macosx_11_0_arm64",
        )
    ] + [f"parasolid_kit-{python}.tar.gz", f"parasolid-core-{rust}.crate"]
    for name in names:
        (directory / name).write_bytes(name.encode())
    return directory


def test_candidate_receipt_accepts_exact_successful_run(tmp_path):
    receipt, run = candidate()
    validate_receipt(receipt, run, SHA, f"v{versions()[0]}")
    receipt["artifacts"] = artifact_hashes(artifacts(tmp_path))
    verify_artifacts(receipt, tmp_path)


@pytest.mark.parametrize(
    "change",
    [
        {"head_sha": "c" * 40},
        {"id": 456},
        {"run_attempt": 2},
        {"status": "in_progress"},
        {"conclusion": "failure"},
        {"path": ".github/workflows/ci.yml"},
        {"event": "release"},
        {"event": "pull_request"},
        {"head_branch": "main"},
        {"repository": {"full_name": "fork/parasolid-kit"}},
        {"head_repository": {"full_name": "fork/parasolid-kit"}},
    ],
)
def test_candidate_receipt_rejects_unverified_run(change):
    receipt, run = candidate()
    with pytest.raises(ValueError, match="successful release-branch run"):
        validate_receipt(receipt, run | change, SHA, f"v{versions()[0]}")


@pytest.mark.parametrize("gate", sorted(GATES))
@pytest.mark.parametrize("change", [None, {"status": "failed"}, {"report_sha256": "unknown"}])
def test_every_private_gate_is_required(gate, change):
    receipt, run = candidate()
    if change is None:
        del receipt["gates"][gate]
    else:
        receipt["gates"][gate].update(change)
    with pytest.raises(ValueError, match="private"):
        validate_receipt(receipt, run, SHA, f"v{versions()[0]}")


def test_candidate_receipt_rejects_wrong_version_or_commit():
    receipt, run = candidate()
    for field in ("source_sha", "python_version", "rust_version", "repository"):
        invalid = copy.deepcopy(receipt)
        invalid[field] = "wrong"
        with pytest.raises(ValueError, match="source or version"):
            validate_receipt(invalid, run, SHA, f"v{versions()[0]}")
    with pytest.raises(ValueError, match="tag differs"):
        validate_receipt(receipt, run, SHA, "v9.9.9")


def test_candidate_receipt_rejects_replaced_artifact(tmp_path):
    receipt, _ = candidate()
    receipt["artifacts"] = artifact_hashes(artifacts(tmp_path))
    next(tmp_path.glob("*.whl")).write_bytes(b"rebuilt after private verification")
    with pytest.raises(ValueError, match="hashes"):
        verify_artifacts(receipt, tmp_path)


@pytest.mark.parametrize("suffix", [".whl", ".tar.gz", ".crate"])
def test_candidate_receipt_rejects_missing_distribution(tmp_path, suffix):
    receipt, _ = candidate()
    receipt["artifacts"] = artifact_hashes(artifacts(tmp_path))
    next(p for p in tmp_path.iterdir() if p.name.endswith(suffix)).unlink()
    with pytest.raises(ValueError):
        verify_artifacts(receipt, tmp_path)


def test_candidate_receipt_rejects_private_input_in_release_directory(tmp_path):
    artifacts(tmp_path)
    (tmp_path / "private.x_b").write_bytes(b"private input")
    with pytest.raises(ValueError, match="unexpected release artifact"):
        artifact_hashes(tmp_path)


def test_candidate_receipt_rejects_duplicate_platform(tmp_path):
    artifacts(tmp_path)
    intel = next(tmp_path.glob("*macosx*x86_64.whl"))
    intel.rename(tmp_path / intel.name.replace("x86_64", "arm64"))
    with pytest.raises(ValueError, match="platform"):
        artifact_hashes(tmp_path)
