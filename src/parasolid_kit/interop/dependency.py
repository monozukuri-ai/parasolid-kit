"""Import optional CAD runtimes only after validating their distributions."""

from __future__ import annotations

import importlib
from importlib import metadata
from types import ModuleType
from typing import Final, Literal, TypeAlias

from ..diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity
from .errors import InteropDependencyError

InteropProfile: TypeAlias = Literal["occt", "cadquery"]

_DISTRIBUTIONS: Final = (
    "cadquery-ocp-novtk",
    "cadquery-ocp",
    "cadquery-ocp-proxy",
    "cadquery",
)


def installed_interop_distributions() -> dict[str, str]:
    """Return detected optional distribution versions without importing their modules."""

    result: dict[str, str] = {}
    for name in _DISTRIBUTIONS:
        version = _distribution_version(name)
        if version is not None:
            result[name] = version
    return result


def require_occt() -> ModuleType:
    """Validate one OCP distribution profile, then import and return ``OCP``."""

    installed = installed_interop_distributions()
    _reject_conflicting_profiles(installed)
    if not ({"cadquery-ocp-novtk", "cadquery-ocp"} & installed.keys()):
        raise _missing_dependency(
            profile="occt",
            missing="cadquery-ocp-novtk",
            installed=installed,
        )
    return _import_runtime("OCP", profile="occt", installed=installed)


def require_cadquery() -> ModuleType:
    """Validate the full CadQuery profile, then import and return ``cadquery``."""

    installed = installed_interop_distributions()
    _reject_conflicting_profiles(installed)
    missing = [name for name in ("cadquery", "cadquery-ocp") if name not in installed]
    if missing:
        raise _missing_dependency(
            profile="cadquery",
            missing=", ".join(missing),
            installed=installed,
        )
    return _import_runtime("cadquery", profile="cadquery", installed=installed)


def _distribution_version(name: str) -> str | None:
    try:
        return metadata.version(name)
    except metadata.PackageNotFoundError:
        return None


def _reject_conflicting_profiles(installed: dict[str, str]) -> None:
    novtk = installed.get("cadquery-ocp-novtk")
    full = installed.get("cadquery-ocp")
    if novtk is None or full is None:
        return
    detected = f"cadquery-ocp-novtk=={novtk}, cadquery-ocp=={full}"
    raise InteropDependencyError(
        Diagnostic(
            code="interop.conflicting_profiles",
            severity=DiagnosticSeverity.ERROR,
            kind=DiagnosticKind.INVALID,
            message=(
                "conflicting distributions provide the same OCP import namespace: "
                f"{detected}. Uninstall one profile before using interop"
            ),
            fatal=True,
            details={
                "detected_distributions": detected,
                "keep_occt_command": (
                    "python -m pip uninstall cadquery cadquery-ocp && "
                    'python -m pip install "parasolid-kit[occt]"'
                ),
                "keep_cadquery_command": (
                    "python -m pip uninstall cadquery-ocp-novtk && "
                    'python -m pip install "parasolid-kit[cadquery]"'
                ),
            },
        )
    )


def _missing_dependency(
    *,
    profile: InteropProfile,
    missing: str,
    installed: dict[str, str],
    import_error: str | None = None,
) -> InteropDependencyError:
    install_command = f'python -m pip install "parasolid-kit[{profile}]"'
    detected = ", ".join(f"{name}=={version}" for name, version in installed.items())
    details = {
        "required_extra": profile,
        "missing_distributions": missing,
        "detected_distributions": detected or "none",
        "install_command": install_command,
    }
    profile_note = ""
    if profile == "cadquery":
        details["minimum_python"] = "3.11"
        profile_note = " The CadQuery profile requires Python 3.11 or newer."
    if import_error is not None:
        details["import_error"] = import_error
    return InteropDependencyError(
        Diagnostic(
            code="interop.missing_dependency",
            severity=DiagnosticSeverity.ERROR,
            kind=DiagnosticKind.INCOMPLETE,
            message=(
                f"the {profile!r} interop profile is unavailable; install it with "
                f"`{install_command}`.{profile_note}"
            ),
            fatal=True,
            details=details,
        )
    )


def _import_runtime(
    module_name: str,
    *,
    profile: InteropProfile,
    installed: dict[str, str],
) -> ModuleType:
    try:
        return importlib.import_module(module_name)
    except ImportError as error:
        dependency_error = _missing_dependency(
            profile=profile,
            missing=f"import {module_name}",
            installed=installed,
            import_error=str(error),
        )
        raise dependency_error from error
