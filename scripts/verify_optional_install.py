#!/usr/bin/env python3
"""Cold-install one optional wheel profile and exercise its guarded runtime."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
I0_SPIKE = ROOT / "scripts" / "verify_optional_interop_i0.py"
I5_ADAPTER = ROOT / "scripts" / "verify_optional_interop_i5.py"
I6_PREVIEW = ROOT / "scripts" / "verify_optional_interop_i6.py"
I7_GEOMETRY = ROOT / "scripts" / "verify_optional_interop_i7.py"
I3_FIXTURES = ROOT / "tests" / "_occt_fixtures.py"

PROFILE_SMOKE_CODE = r"""
import json
import math
import runpy
import sys
import tempfile
from pathlib import Path

profile = sys.argv[1]
import parasolid_kit
import parasolid_kit.interop as interop

assert parasolid_kit.__version__ == "0.1.0"
from parasolid_kit import _core
assert _core.CORE_VERSION == "0.1.0"
assert "OCP" not in sys.modules
assert "cadquery" not in sys.modules
before = interop.installed_interop_distributions()
if profile == "occt":
    runtime = interop.require_occt()
    assert runtime.__name__ == "OCP"
    assert "OCP" in sys.modules
    assert "cadquery" not in sys.modules
    assert "cadquery-ocp-novtk" in before
    assert "cadquery-ocp" not in before
elif profile == "cadquery":
    runtime = interop.require_cadquery()
    assert runtime.__name__ == "cadquery"
    assert "cadquery" in sys.modules
    assert "OCP" in sys.modules
    assert "cadquery" in before
    assert "cadquery-ocp" in before
    assert "cadquery-ocp-novtk" not in before
else:
    raise AssertionError(f"unexpected profile: {profile}")

fixtures = runpy.run_path(sys.argv[2])
from parasolid_kit.interop.occt import ShapeRelationKind, to_occt, write_step

box = to_occt(fixtures["make_box_model"](), source_unit="mm")
box_counts = box.report.output_topology.to_dict()
assert box.report.conversion_complete and box.report.occt_valid
assert not box.report.healing_performed
assert box_counts["solids"] == 1
assert box_counts["faces"] == 6
assert box_counts["edges"] == 12
assert box_counts["vertices"] == 8
assert math.isclose(box.report.metrics.surface_area, 5200.0, rel_tol=1.0e-10)
assert math.isclose(box.report.metrics.volume, 24000.0, rel_tol=1.0e-10)

cylinder_hole = to_occt(fixtures["make_cylinder_hole_model"](), source_unit="mm")
cylinder_counts = cylinder_hole.report.output_topology.to_dict()
assert cylinder_hole.report.conversion_complete and cylinder_hole.report.occt_valid
assert not cylinder_hole.report.healing_performed
assert cylinder_counts["solids"] == 1
assert cylinder_counts["faces"] == 4
assert cylinder_counts["edges"] == 6
assert cylinder_counts["vertices"] == 4
relation_kinds = {item.relation for item in cylinder_hole.source_map.relations}
assert ShapeRelationKind.SPLIT in relation_kinds
assert ShapeRelationKind.MERGED in relation_kinds
assert ShapeRelationKind.GENERATED in relation_kinds
assert cylinder_hole.report.generated_topology_count == 6
with tempfile.TemporaryDirectory(prefix="parasolid-kit-i4-") as temporary:
    root = Path(temporary)
    box_step = write_step(box, root / "box.step", output_unit="mm")
    cylinder_step = write_step(
        cylinder_hole,
        root / "cylinder-hole.step",
        output_unit="m",
    )
    assert box_step.report.status == "validated"
    assert box_step.report.validation is not None
    assert box_step.report.validation.passed
    assert box_step.report.validation.process_isolated
    assert cylinder_step.report.status == "validated"
    assert cylinder_step.report.validation is not None
    assert cylinder_step.report.validation.passed
    assert box_step.sidecar_path.is_file()
    assert cylinder_step.sidecar_path.is_file()

    def compact_step(exported, conversion):
        sidecar = json.loads(exported.sidecar_path.read_text(encoding="utf-8"))
        assert len(sidecar["source_map"]["relations"]) == (
            conversion.report.mapping_relation_count
        )
        return {
            "status": exported.report.status,
            "artifact": exported.report.artifact.to_dict(),
            "step_schema": exported.report.step_schema,
            "output_unit": exported.report.output_unit,
            "mapping_relation_count": conversion.report.mapping_relation_count,
            "validation": exported.report.validation.to_dict(),
        }

    i4_export = {
        "box": compact_step(box_step, box),
        "cylinder_hole": compact_step(cylinder_step, cylinder_hole),
    }
print(json.dumps({
    "profile": profile,
    "runtime_module": runtime.__name__,
    "distributions": before,
    "i3_conversion": {
        "box": box.report.to_dict(),
        "cylinder_hole": cylinder_hole.report.to_dict(),
    },
    "i4_step_export": i4_export,
}, sort_keys=True))
""".strip()

CONFLICT_SMOKE_CODE = r"""
import json
import sys

import parasolid_kit.interop as interop

assert "OCP" not in sys.modules
assert "cadquery" not in sys.modules
try:
    interop.require_occt()
except interop.InteropDependencyError as error:
    assert error.diagnostic.code == "interop.conflicting_profiles"
    detected = error.diagnostic.details["detected_distributions"]
    assert "cadquery-ocp-novtk==" in detected
    assert "cadquery-ocp==" in detected
    assert "OCP" not in sys.modules
    assert "cadquery" not in sys.modules
    print(json.dumps({
        "status": "rejected_before_import",
        "diagnostic": error.diagnostic.to_dict(),
    }, sort_keys=True))
else:
    raise AssertionError("conflicting OCP distributions were accepted")
""".strip()

UNSUPPORTED_PLATFORM_SMOKE_CODE = r"""
import json
import sys

import parasolid_kit
from parasolid_kit import interop

assert parasolid_kit.__version__ == "0.1.0"
from parasolid_kit import _core
assert _core.CORE_VERSION == "0.1.0"
assert sys.platform == "win32"
assert "OCP" not in sys.modules
assert "cadquery" not in sys.modules
try:
    interop.require_cadquery()
except interop.InteropDependencyError as error:
    assert error.diagnostic.code == "interop.unsupported_platform"
    assert error.diagnostic.details["alternative_extra"] == "occt"
    assert "OCP" not in sys.modules
    assert "cadquery" not in sys.modules
    print(json.dumps(error.diagnostic.to_dict(), sort_keys=True))
else:
    raise AssertionError("the unsupported Windows CadQuery runtime was accepted")
""".strip()


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--profile", choices=("occt", "cadquery"), required=True)
    parser.add_argument("--python", default=sys.executable)
    expectation = parser.add_mutually_exclusive_group()
    expectation.add_argument(
        "--expect-unsupported-python",
        action="store_true",
        help="require dependency resolution to reject this Python/profile pair",
    )
    expectation.add_argument(
        "--expect-unsupported-platform",
        action="store_true",
        help="install the base wheel and require Windows CadQuery rejection before import",
    )
    return parser.parse_args()


def _environment_python(environment: Path) -> Path:
    windows = environment / "Scripts" / "python.exe"
    return windows if windows.exists() else environment / "bin" / "python"


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


def verify_profile(
    wheel: Path,
    profile: str,
    python: str,
    *,
    expect_unsupported_platform: bool = False,
) -> dict[str, object]:
    """Install one extra into a fresh environment and run guarded runtime checks."""

    if expect_unsupported_platform and profile != "cadquery":
        raise ValueError("only the cadquery profile has an unsupported platform gate")
    wheel = wheel.resolve()
    if not wheel.is_file():
        raise FileNotFoundError(f"wheel does not exist: {wheel}")
    with tempfile.TemporaryDirectory(prefix=f"parasolid-kit-{profile}-") as temporary:
        root = Path(temporary)
        environment_path = root / "environment"
        work_dir = root / "work"
        work_dir.mkdir()
        environment = os.environ.copy()
        for name in ("PYTHONPATH", "VIRTUAL_ENV", "UV_PROJECT_ENVIRONMENT"):
            environment.pop(name, None)
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
        extra = "" if expect_unsupported_platform else f"[{profile}]"
        requirement = f"parasolid-kit{extra} @ {wheel.as_uri()}"
        _run(
            [
                "uv",
                "pip",
                "install",
                "--python",
                str(environment_python),
                "--no-cache",
                requirement,
            ],
            cwd=work_dir,
            environment=environment,
        )
        if expect_unsupported_platform:
            diagnostic = json.loads(
                _run(
                    [str(environment_python), "-I", "-c", UNSUPPORTED_PLATFORM_SMOKE_CODE],
                    cwd=work_dir,
                    environment=environment,
                )
            )
            return {
                "status": "rejected_as_expected",
                "profile": profile,
                "python": python,
                "wheel": str(wheel),
                "diagnostic": diagnostic,
            }
        runtime = json.loads(
            _run(
                [
                    str(environment_python),
                    "-I",
                    "-c",
                    PROFILE_SMOKE_CODE,
                    profile,
                    str(I3_FIXTURES),
                ],
                cwd=work_dir,
                environment=environment,
            )
        )
        i0_spike = json.loads(
            _run(
                [
                    str(environment_python),
                    "-I",
                    str(I0_SPIKE),
                    "--expect-profile",
                    profile,
                ],
                cwd=work_dir,
                environment=environment,
            )
        )
        i5_adapter: dict[str, object] | None = None
        i6_preview = json.loads(
            _run(
                [
                    str(environment_python),
                    "-I",
                    str(I6_PREVIEW),
                    "--skip-browser",
                ],
                cwd=work_dir,
                environment=environment,
            )
        )
        if i6_preview["status"] != "passed":
            raise RuntimeError("the cold I6 preview gate did not pass")
        i7_geometry = json.loads(
            _run(
                [str(environment_python), "-I", str(I7_GEOMETRY)],
                cwd=work_dir,
                environment=environment,
            )
        )
        if i7_geometry["status"] != "passed":
            raise RuntimeError("the cold I7 geometry coverage gate did not pass")
        if profile == "cadquery":
            i5_adapter = json.loads(
                _run(
                    [str(environment_python), "-I", str(I5_ADAPTER)],
                    cwd=work_dir,
                    environment=environment,
                )
            )
            if i5_adapter["status"] != "passed":
                raise RuntimeError("the cold I5 CadQuery adapter gate did not pass")
        _run(
            [
                "uv",
                "pip",
                "install",
                "--python",
                str(environment_python),
                "--no-cache",
                "pytest>=8.3,<9",
                "jsonschema>=4.23,<5",
            ],
            cwd=work_dir,
            environment=environment,
        )
        test_suite = _run(
            [str(environment_python), "-I", "-m", "pytest", "-q", str(ROOT / "tests")],
            cwd=work_dir,
            environment=environment,
        )
        conflict: dict[str, object] | None = None
        if profile == "cadquery":
            _run(
                [
                    "uv",
                    "pip",
                    "install",
                    "--python",
                    str(environment_python),
                    "--no-cache",
                    "cadquery-ocp-novtk>=7.9.3.1,<7.10",
                ],
                cwd=work_dir,
                environment=environment,
            )
            conflict = json.loads(
                _run(
                    [str(environment_python), "-I", "-c", CONFLICT_SMOKE_CODE],
                    cwd=work_dir,
                    environment=environment,
                )
            )
    return {
        "status": "passed",
        "profile": profile,
        "python": python,
        "wheel": str(wheel),
        "runtime": runtime,
        "i0_spike": i0_spike,
        "i5_adapter": i5_adapter,
        "i6_preview": i6_preview,
        "i7_geometry": i7_geometry,
        "test_suite": test_suite,
        "conflict": conflict,
    }


def verify_unsupported_python(wheel: Path, profile: str, python: str) -> dict[str, object]:
    """Prove that an unsupported profile is rejected instead of silently omitted."""

    if profile != "cadquery":
        raise ValueError("only the cadquery profile has an unsupported Python gate")
    wheel = wheel.resolve()
    if not wheel.is_file():
        raise FileNotFoundError(f"wheel does not exist: {wheel}")
    with tempfile.TemporaryDirectory(prefix="parasolid-kit-cadquery-rejection-") as temporary:
        root = Path(temporary)
        environment_path = root / "environment"
        work_dir = root / "work"
        work_dir.mkdir()
        environment = os.environ.copy()
        for name in ("PYTHONPATH", "VIRTUAL_ENV", "UV_PROJECT_ENVIRONMENT"):
            environment.pop(name, None)
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
        requirement = f"parasolid-kit[{profile}] @ {wheel.as_uri()}"
        completed = subprocess.run(
            [
                "uv",
                "pip",
                "install",
                "--python",
                str(environment_python),
                "--no-cache",
                requirement,
            ],
            cwd=work_dir,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )
        evidence = f"{completed.stdout}\n{completed.stderr}".strip()
        normalized = evidence.lower()
        if completed.returncode == 0:
            raise RuntimeError(
                "the unsupported CadQuery profile installed successfully; its dependency "
                "must not be silently omitted"
            )
        if "cadquery" not in normalized or not any(
            marker in normalized for marker in ("3.11", "requires-python", "requires python")
        ):
            raise RuntimeError(
                "installation failed without identifying CadQuery's Python requirement:\n"
                f"{evidence}"
            )
    return {
        "status": "rejected_as_expected",
        "profile": profile,
        "python": python,
        "wheel": str(wheel),
        "resolver_evidence": evidence,
    }


def main() -> int:
    """Run one profile check and emit one deterministic JSON report."""

    arguments = _arguments()
    try:
        if arguments.expect_unsupported_python:
            report = verify_unsupported_python(
                arguments.wheel,
                arguments.profile,
                arguments.python,
            )
        else:
            report = verify_profile(
                arguments.wheel,
                arguments.profile,
                arguments.python,
                expect_unsupported_platform=arguments.expect_unsupported_platform,
            )
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        report = {
            "status": "failed",
            "profile": arguments.profile,
            "error": str(error),
        }
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
        return 1
    print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
