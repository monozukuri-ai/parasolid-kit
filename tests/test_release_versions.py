"""Version bumps must propagate without editing Python source or test expectations."""

from __future__ import annotations

import sys
from types import ModuleType

import pytest

pytest.importorskip("tomllib", reason="maintainer release tooling requires Python 3.11+")

from scripts.bump_version import bump_version
from scripts.verify_release import python_version, runtime_version_check, source_versions, versions


@pytest.fixture
def source(tmp_path):
    (tmp_path / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.3-rc.4"\n')
    (tmp_path / "pyproject.toml").write_text('[project]\ndynamic = ["version"]\n')
    (tmp_path / "fuzz").mkdir()
    core = '[[package]]\nname = "parasolid-core"\nversion = "1.2.3-rc.4"\n'
    binding = '[[package]]\nname = "parasolid-python"\nversion = "1.2.3-rc.4"\n'
    (tmp_path / "Cargo.lock").write_text(core + binding)
    (tmp_path / "fuzz/Cargo.lock").write_text(core)
    (tmp_path / "uv.lock").write_text(
        '[[package]]\nname = "parasolid-kit"\nsource = { editable = "." }\n'
    )
    return tmp_path


@pytest.mark.parametrize(
    ("rust", "python"),
    [("1.2.3", "1.2.3"), ("1.2.3-rc.4", "1.2.3rc4"), ("1.2.3-dev0", "1.2.3.dev0")],
)
def test_cargo_version_maps_to_python(rust, python):
    assert python_version(rust) == python


def test_source_and_locks_need_no_installed_package(source):
    assert source_versions(source) == ("1.2.3rc4", "1.2.3-rc.4")
    assert versions(source, "v1.2.3rc4") == ("1.2.3rc4", "1.2.3-rc.4")
    with pytest.raises(ValueError, match="release tag differs"):
        versions(source, "v1.2.3-rc.4")


@pytest.mark.parametrize("project", ['version = "1.2.3rc4"', 'dynamic = ["description"]'])
def test_source_rejects_a_second_version_definition(source, project):
    (source / "pyproject.toml").write_text(f"[project]\n{project}\n")
    with pytest.raises(ValueError, match="must derive its version"):
        source_versions(source)


@pytest.mark.parametrize("filename", ["Cargo.lock", "fuzz/Cargo.lock", "uv.lock"])
def test_release_rejects_stale_locks(source, filename):
    path = source / filename
    text = path.read_text()
    if filename == "uv.lock":
        text += 'version = "1.2.3rc4"\n'
    else:
        text = text.replace("1.2.3-rc.4", "1.2.2")
    path.write_text(text)
    with pytest.raises(ValueError, match=filename):
        versions(source)


@pytest.mark.parametrize("version", ["v1.2.3", "1.2", "1.2.3rc4", "01.2.3", "1.2.3-rc.04"])
def test_invalid_bump_does_not_modify_files(source, version):
    before = {p: p.read_bytes() for p in source.rglob("*") if p.is_file()}
    with pytest.raises(ValueError, match="unsupported release version"):
        bump_version(version, source)
    assert {p: p.read_bytes() for p in before} == before


@pytest.mark.parametrize(
    ("facade", "metadata", "native", "accepted"),
    [
        ("1.2.3rc4", "1.2.3rc4", "1.2.3-rc.4", True),
        ("1.2.3rc4", "1.2.3rc4", "1.2.2", False),
        ("1.2.2", "1.2.2", "1.2.3-rc.4", False),
        ("1.2.2", "1.2.3rc4", "1.2.3-rc.4", False),
    ],
)
def test_cold_runtime_checks_source_metadata_and_native_version(
    source, monkeypatch, facade, metadata, native, accepted
):
    package = ModuleType("parasolid_kit")
    package.__version__ = facade
    package._core = ModuleType("parasolid_kit._core")
    package._core.CORE_VERSION = native
    monkeypatch.setitem(sys.modules, "parasolid_kit", package)
    monkeypatch.setattr("importlib.metadata.version", lambda name: metadata)
    code = runtime_version_check(source)
    if accepted:
        exec(code, {})
    else:
        with pytest.raises(AssertionError):
            exec(code, {})
