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
import parasolid_kit
from parasolid_kit import inspect_xb, inspect_xt, parse_xb, parse_xt
assert parasolid_kit.__version__ == "0.1.0.dev0"
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
        check=True,
        capture_output=True,
        text=True,
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
            ["uv", "venv", "--python", python, str(environment_path)],
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
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        report = {"status": "failed", "artifacts": reports, "error": str(error)}
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 1
    report = {"status": "passed", "artifacts": reports}
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
