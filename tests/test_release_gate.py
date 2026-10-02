"""Only the complete, unchanged set of CI distributions may be published."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

import pytest

pytest.importorskip("tomllib", reason="maintainer release tooling requires Python 3.11+")

from scripts.verify_release import artifact_hashes, verify_artifacts, versions


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


def test_ci_artifact_hashes_match_the_verified_distributions(tmp_path):
    directory = artifacts(tmp_path)
    expected = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in directory.iterdir()}
    assert artifact_hashes(directory) == expected
    verify_artifacts(expected, directory)


def test_publication_rejects_replaced_artifact(tmp_path):
    expected = artifact_hashes(artifacts(tmp_path))
    next(tmp_path.glob("*.whl")).write_bytes(b"rebuilt after CI verification")
    with pytest.raises(ValueError, match="differ"):
        verify_artifacts(expected, tmp_path)


@pytest.mark.parametrize("suffix", [".whl", ".tar.gz", ".crate"])
def test_publication_rejects_missing_distribution(tmp_path, suffix):
    expected = artifact_hashes(artifacts(tmp_path))
    next(p for p in tmp_path.iterdir() if p.name.endswith(suffix)).unlink()
    with pytest.raises(ValueError):
        verify_artifacts(expected, tmp_path)


def test_publication_rejects_private_input_in_release_directory(tmp_path):
    artifacts(tmp_path)
    (tmp_path / "private.x_b").write_bytes(b"private input")
    with pytest.raises(ValueError, match="unexpected release artifact"):
        artifact_hashes(tmp_path)


def test_publication_rejects_duplicate_platform(tmp_path):
    artifacts(tmp_path)
    intel = next(tmp_path.glob("*macosx*x86_64.whl"))
    intel.rename(tmp_path / intel.name.replace("x86_64", "arm64"))
    with pytest.raises(ValueError, match="platform"):
        artifact_hashes(tmp_path)


@pytest.mark.parametrize("changed", [False, True])
def test_publication_cli_checks_ci_hashes_without_receipt(tmp_path, monkeypatch, capsys, changed):
    from scripts import verify_release

    expected = artifact_hashes(artifacts(tmp_path))
    if changed:
        next(tmp_path.glob("*.whl")).write_bytes(b"changed")
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "verify_release.py",
            "--tag",
            f"v{versions()[0]}",
            "--artifacts",
            str(tmp_path),
            "--expected-hashes",
            json.dumps(expected),
        ],
    )
    if changed:
        with pytest.raises(ValueError, match="differ"):
            verify_release.main()
    else:
        verify_release.main()
        assert json.loads(json.loads(capsys.readouterr().out)["artifact_hashes"]) == expected


def test_expected_hashes_require_artifacts(monkeypatch, capsys):
    from scripts import verify_release

    monkeypatch.setattr(sys, "argv", ["verify_release.py", "--expected-hashes", "{}"])
    with pytest.raises(SystemExit, match="2"):
        verify_release.main()
    assert "expected hashes require --artifacts" in capsys.readouterr().err
