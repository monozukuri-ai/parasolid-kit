from __future__ import annotations

import sys
from dataclasses import FrozenInstanceError
from pathlib import Path
from types import ModuleType

import pytest

from parasolid_kit import (
    Diagnostic,
    DiagnosticKind,
    DiagnosticSeverity,
    ParasolidError,
    ParseLimits,
    interop,
)
from parasolid_kit.interop import dependency

ROOT = Path(__file__).resolve().parents[1]


def _versions(values: dict[str, str]):
    return lambda name: values.get(name)


def test_i2_optional_profiles_are_exact_and_mutually_exclusive() -> None:
    project_file = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
    optional = project_file.split("[project.optional-dependencies]\n", 1)[1].split("\n[", 1)[0]
    uv_settings = project_file.split("[tool.uv]\n", 1)[1].split("\n[", 1)[0]

    assert "dependencies = []" in project_file
    assert '"cadquery-ocp-novtk>=7.9.3.1,<7.10"' in optional
    assert '"cadquery>=2.8,<2.9"' in optional
    assert "all =" not in optional
    assert "\"python_version >= '3.11'\"" in uv_settings
    assert '{ extra = "occt" }' in uv_settings
    assert '{ extra = "cadquery" }' in uv_settings


def test_interop_import_is_dependency_free_and_exports_the_i2_contract() -> None:
    assert "OCP" not in sys.modules
    assert "cadquery" not in sys.modules
    assert callable(interop.require_occt)
    assert callable(interop.require_cadquery)
    assert issubclass(interop.InteropDependencyError, interop.InteropError)
    assert issubclass(interop.InteropError, ParasolidError)
    assert issubclass(interop.OcctConversionError, interop.InteropError)
    assert issubclass(interop.CadQueryConversionError, interop.InteropError)
    assert issubclass(interop.StepExportError, interop.InteropError)
    assert issubclass(interop.PreviewError, interop.InteropError)


def test_missing_occt_profile_has_an_actionable_structured_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    imported: list[str] = []
    monkeypatch.setattr(dependency, "_distribution_version", _versions({}))
    monkeypatch.setattr(
        dependency.importlib,
        "import_module",
        lambda name: imported.append(name),
    )

    with pytest.raises(interop.InteropDependencyError) as captured:
        interop.require_occt()

    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "interop.missing_dependency"
    assert diagnostic.kind is DiagnosticKind.INCOMPLETE
    assert diagnostic.fatal is True
    assert diagnostic.details["required_extra"] == "occt"
    assert diagnostic.details["missing_distributions"] == "cadquery-ocp-novtk"
    assert diagnostic.details["install_command"] == ('python -m pip install "parasolid-kit[occt]"')
    assert imported == []


def test_missing_cadquery_profile_reports_every_required_distribution(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        dependency,
        "_distribution_version",
        _versions({"cadquery": "2.8.0"}),
    )

    with pytest.raises(interop.InteropDependencyError) as captured:
        interop.require_cadquery()

    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "interop.missing_dependency"
    assert diagnostic.details["required_extra"] == "cadquery"
    assert diagnostic.details["missing_distributions"] == "cadquery-ocp"
    assert diagnostic.details["detected_distributions"] == "cadquery==2.8.0"
    assert diagnostic.details["minimum_python"] == "3.11"
    assert "Python 3.11 or newer" in str(captured.value)


def test_conflicting_profiles_stop_before_the_ocp_namespace_is_imported(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    imported: list[str] = []
    monkeypatch.setattr(
        dependency,
        "_distribution_version",
        _versions(
            {
                "cadquery-ocp-novtk": "7.9.3.1.1",
                "cadquery-ocp": "7.9.3.1.1",
                "cadquery": "2.8.0",
            }
        ),
    )
    monkeypatch.setattr(
        dependency.importlib,
        "import_module",
        lambda name: imported.append(name),
    )

    with pytest.raises(interop.InteropDependencyError) as captured:
        interop.require_occt()

    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "interop.conflicting_profiles"
    assert diagnostic.kind is DiagnosticKind.INVALID
    assert diagnostic.details["detected_distributions"] == (
        "cadquery-ocp-novtk==7.9.3.1.1, cadquery-ocp==7.9.3.1.1"
    )
    assert "pip uninstall cadquery cadquery-ocp" in diagnostic.details["keep_occt_command"]
    assert "pip uninstall cadquery-ocp-novtk" in diagnostic.details["keep_cadquery_command"]
    assert imported == []


@pytest.mark.parametrize(
    ("profile", "versions", "expected_module"),
    [
        ("occt", {"cadquery-ocp-novtk": "7.9.3.1.1"}, "OCP"),
        (
            "cadquery",
            {"cadquery": "2.8.0", "cadquery-ocp": "7.9.3.1.1"},
            "cadquery",
        ),
    ],
)
def test_valid_profile_imports_only_after_distribution_validation(
    profile: str,
    versions: dict[str, str],
    expected_module: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    module = ModuleType(expected_module)
    imported: list[str] = []

    def import_module(name: str) -> ModuleType:
        imported.append(name)
        return module

    monkeypatch.setattr(dependency, "_distribution_version", _versions(versions))
    monkeypatch.setattr(dependency.importlib, "import_module", import_module)

    loaded = interop.require_occt() if profile == "occt" else interop.require_cadquery()

    assert loaded is module
    assert imported == [expected_module]
    assert dependency.installed_interop_distributions() == versions


def test_broken_optional_import_becomes_a_structured_dependency_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        dependency,
        "_distribution_version",
        _versions({"cadquery-ocp-novtk": "7.9.3.1.1"}),
    )

    def broken_import(_name: str) -> ModuleType:
        raise ModuleNotFoundError("native OCP module is incomplete")

    monkeypatch.setattr(dependency.importlib, "import_module", broken_import)

    with pytest.raises(interop.InteropDependencyError) as captured:
        interop.require_occt()

    assert captured.value.diagnostic.code == "interop.missing_dependency"
    assert captured.value.diagnostic.details["import_error"] == ("native OCP module is incomplete")
    assert isinstance(captured.value.__cause__, ModuleNotFoundError)


def test_interop_error_rejects_a_diagnostic_owned_by_another_layer() -> None:
    diagnostic = Diagnostic(
        code="document.invalid_fixture",
        severity=DiagnosticSeverity.ERROR,
        kind=DiagnosticKind.INVALID,
        message="not an interop diagnostic",
    )

    with pytest.raises(ValueError, match="interop-owned prefix"):
        interop.InteropError(diagnostic)


def test_interop_limits_are_immutable_positive_and_separate_from_parse_limits() -> None:
    limits = interop.InteropLimits(max_entities=42, max_triangles=84)

    assert limits.max_entities == 42
    assert limits.limit_for("max_triangles") == 84
    assert limits.to_dict()["max_output_bytes"] == 512 * 1024 * 1024
    assert not hasattr(ParseLimits(), "max_triangles")
    with pytest.raises(FrozenInstanceError):
        limits.max_entities = 43  # type: ignore[misc]


@pytest.mark.parametrize("value", [0, -1, True, 1.5, "10"])
def test_interop_limits_reject_non_positive_or_non_integer_values(value: object) -> None:
    with pytest.raises(ValueError, match="positive integer"):
        interop.InteropLimits(max_vertices=value)  # type: ignore[arg-type]


def test_interop_limits_reject_an_unknown_resource_name() -> None:
    limits = interop.InteropLimits()

    with pytest.raises(ValueError, match="unknown interop limit"):
        limits.limit_for("max_nodes")
