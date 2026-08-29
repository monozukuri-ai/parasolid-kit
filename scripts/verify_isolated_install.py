#!/usr/bin/env python3
"""Install wheel and sdist into separate cold environments and smoke the API/CLI."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

SMOKE_CODE = """
import json
import hashlib
import sys
from importlib import resources
import parasolid_kit
import parasolid_kit.interop as interop
import parasolid_kit.interop.cadquery as cadquery_interop
import parasolid_kit.interop.occt as occt
import parasolid_kit.interop.preview as preview
from parasolid_kit import (
    BrepSummary,
    DirectorySchemaProvider,
    ParsedBrep,
    inspect_xb,
    inspect_xt,
    parse_xb,
    parse_xt,
    read_brep,
)
assert parasolid_kit.__version__ == "0.1.0.dev0"
assert callable(read_brep)
assert BrepSummary.__module__ == "parasolid_kit.summary"
assert ParsedBrep.__module__ == "parasolid_kit.summary"
assert DirectorySchemaProvider.__module__ == "parasolid_kit.schema.provider"
assert callable(occt.to_occt)
assert callable(occt.write_step)
assert len(occt.geometry_coverage()) == 20
assert occt.geometry_coverage() is occt.GEOMETRY_COVERAGE
assert "surface_parametric" in occt.render_geometry_coverage_markdown()
assert callable(cadquery_interop.to_cadquery)
assert callable(cadquery_interop.to_cadquery_shapes)
assert callable(preview.write_preview)
assert callable(preview.create_preview_server)
assert occt.OcctConversionOptions(source_unit="m").applied_scale == 1000.0
static = resources.files("parasolid_kit.interop.preview").joinpath("static")
for name, expected in preview.STATIC_ASSET_SHA256.items():
    assert hashlib.sha256(static.joinpath(name).read_bytes()).hexdigest() == expected
assert "OCP" not in sys.modules
assert "cadquery" not in sys.modules
for guard, extra in (
    (interop.require_occt, "occt"),
    (interop.require_cadquery, "cadquery"),
):
    try:
        guard()
    except interop.InteropDependencyError as error:
        assert error.diagnostic.code == "interop.missing_dependency"
        assert error.diagnostic.details["required_extra"] == extra
        assert error.diagnostic.details["install_command"] == (
            f'python -m pip install "parasolid-kit[{extra}]"'
        )
    else:
        raise AssertionError(f"base install unexpectedly provided {extra}")
assert "OCP" not in sys.modules
assert "cadquery" not in sys.modules
modeller = b": TRANSMIT FILE created by modeller version 3000000"
schema = b"SCH_3000000_30000"
payload = b"PS\\0\\0" + len(modeller).to_bytes(2, "big") + modeller
payload += len(schema).to_bytes(4, "big", signed=True) + schema
payload += (0).to_bytes(4, "big", signed=True)
header = inspect_xb(payload)
assert header.schema_key == schema.decode("ascii")
print(json.dumps({
    "version": parasolid_kit.__version__,
    "api": "imported",
    "interop_base": "missing_extras_actionable",
    "viewer_assets": sorted(preview.STATIC_ASSET_SHA256),
    "geometry_coverage_rows": len(occt.GEOMETRY_COVERAGE),
    "native_inspect_schema": header.schema_key,
}))
""".strip()


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--sdist", type=Path, required=True)
    parser.add_argument("--python", default=sys.executable)
    return parser.parse_args()


def _environment_python(environment: Path) -> Path:
    windows = environment / "Scripts" / "python.exe"
    return windows if windows.exists() else environment / "bin" / "python"


def _console_script(environment: Path) -> Path:
    windows = environment / "Scripts" / "parasolid-kit.exe"
    return windows if windows.exists() else environment / "bin" / "parasolid-kit"


def _run(command: list[str], *, cwd: Path, environment: dict[str, str]) -> str:
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed with exit {completed.returncode}: {command!r}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return completed.stdout.strip()


def verify_install(artifact: Path, python: str) -> dict[str, object]:
    """Install one artifact with uv and run from outside the checkout."""

    artifact = artifact.resolve()
    with tempfile.TemporaryDirectory(prefix="parasolid-kit-cold-") as temporary:
        root = Path(temporary)
        environment_path = root / "environment"
        work_dir = root / "work"
        work_dir.mkdir()
        environment = os.environ.copy()
        environment.pop("PYTHONPATH", None)
        _run(
            [
                "uv",
                "venv",
                "--no-project",
                "--no-cache",
                "--python",
                python,
                str(environment_path),
            ],
            cwd=work_dir,
            environment=environment,
        )
        environment_python = _environment_python(environment_path)
        _run(
            [
                "uv",
                "pip",
                "install",
                "--python",
                str(environment_python),
                "--no-cache",
                str(artifact),
            ],
            cwd=work_dir,
            environment=environment,
        )
        imported = _run(
            [str(environment_python), "-I", "-c", SMOKE_CODE],
            cwd=work_dir,
            environment=environment,
        )
        module_version = _run(
            [str(environment_python), "-I", "-m", "parasolid_kit", "--version"],
            cwd=work_dir,
            environment=environment,
        )
        console_version = _run(
            [str(_console_script(environment_path)), "--version"],
            cwd=work_dir,
            environment=environment,
        )
    return {
        "artifact": str(artifact),
        "status": "passed",
        "import": json.loads(imported),
        "module_cli": module_version,
        "console_cli": console_version,
    }


def main() -> int:
    """Run isolated checks for both archive kinds."""

    arguments = _arguments()
    reports: list[dict[str, object]] = []
    try:
        for artifact in (arguments.wheel, arguments.sdist):
            reports.append(verify_install(artifact, arguments.python))
    except (OSError, RuntimeError, json.JSONDecodeError) as error:
        report = {"status": "failed", "artifacts": reports, "error": str(error)}
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 1
    report = {"status": "passed", "artifacts": reports}
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
