#!/usr/bin/env python3
"""Install wheel and sdist into separate cold environments and smoke the API/CLI."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Installed before package import in every runtime subprocess. Build/install may
# fetch ordinary dependencies; runtime may not read the checkout or catalogs.
RUNTIME_GUARD_CODE = """
import sys
from pathlib import Path
checkout = Path(sys.argv[1]).resolve()
installed_environment = Path(sys.argv[2]).resolve()
def runtime_guard(event, args):
    if event.startswith("socket."):
        raise RuntimeError("network operation during isolated parser runtime")
    if event == "open" and isinstance(args[0], (str, bytes)):
        import os
        path = Path(os.fsdecode(args[0])).resolve()
        if path.is_relative_to(checkout) or path.name.lower().endswith(".sch_txt"):
            raise RuntimeError("checkout/catalog read during isolated parser runtime")
sys.addaudithook(runtime_guard)
""".strip()

BUILTIN_CODE = """
from parasolid_kit import (
    InMemorySchemaProvider, SchemaError, compare_documents, write_xb,
)
expected_profile = {
    "kind": "builtin", "profile_id": "onshape-sch30000-r2", "profile_revision": 2,
    "schema_key": "SCH_3000000_30000", "coverage": "verified_subset",
    "profile_sha256": "adce41a88ebc4179212519144a5a627dba8d0b6572e3d77ac709f0b16840657f",
}
expected_profiles = {expected_profile["schema_key"]: expected_profile}
for key, profile_id, digest in (
    ("SCH_1300000_13006", "onshape-sch13006-r6",
     "2748e9f9c28fa32b59edb9e0b16fea7a976ad081f698dd3d098d9cbd17e08fbb"),
    ("SCH_3000310_30000_13006", "icad-sch30000-13006-r5",
     "1f090c87aef63e99af8dcb3aef9cc077a613af6749f177392989cca70ca3bfa5"),
):
    expected_profiles[key] = {
        "kind": "builtin", "profile_id": profile_id,
        "profile_revision": 6 if key == "SCH_1300000_13006" else 5,
        "schema_key": key, "coverage": "verified_subset", "profile_sha256": digest,
    }
""".strip()

FIXTURE_CODE = """
import hashlib
import json
import parasolid_kit
from parasolid_kit import parse_xt, parse_xb, read_brep
assert Path(parasolid_kit.__file__).resolve().is_relative_to(installed_environment)
paths = [Path(p) for p in sys.argv[3:]]
assert len(paths) in (1, 2)
documents = [(parse_xt if p.suffix == ".x_t" else parse_xb)(p) for p in paths]
if len(paths) == 2:
    assert compare_documents(*documents).equivalent
reports = []
for path, document in zip(paths, documents):
    if path.suffix == ".x_b":
        assert write_xb(document) == path.read_bytes()
    assert document.schema_resolution.to_dict() == expected_profiles[document.schema_key.raw]
    result = read_brep(path)
    assert result.brep.complete and result.brep.topology.valid
    assert not result.brep.diagnostics and not document.diagnostics
    assert result.summary.schema_resolution == document.schema_resolution
    reports.append({
        "input_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "records": len(document.nodes),
        "fields": sum(len(n.fields) for n in document.nodes),
        "summary": result.summary.to_dict(),
    })
assert "OCP" not in sys.modules and "cadquery" not in sys.modules
print(json.dumps({"equivalent": True if len(paths) == 2 else None,
                  "complete": True, "streams": reports}))
""".strip()

CLI_CODE = """
import runpy
sys.argv = ["parasolid-kit", *sys.argv[3:]]
runpy.run_module("parasolid_kit", run_name="__main__")
""".strip()

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
assert Path(parasolid_kit.__file__).resolve().is_relative_to(installed_environment)
assert parasolid_kit.__version__ == "0.1.0"
from parasolid_kit import _core
assert _core.CORE_VERSION == "0.1.0"
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
        assert error.diagnostic.details["required_extra"] == extra
        if extra == "cadquery" and sys.platform == "win32":
            assert error.diagnostic.code == "interop.unsupported_platform"
            assert error.diagnostic.details["alternative_extra"] == "occt"
        else:
            assert error.diagnostic.code == "interop.missing_dependency"
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
# Synthetic integer-array record (variable length precedes the compact index).
payload += (82).to_bytes(2, "big") + (2).to_bytes(4, "big")
payload += (2).to_bytes(2, "big") + (7).to_bytes(4, "big")
payload += (-32764).to_bytes(4, "big", signed=True) + bytes([0, 1, 0, 1])
text_payload = (f"T{len(modeller)} {modeller.decode()}{len(schema)} "
                f"{schema.decode()}0 82 2 1 7 ?1 0 ").encode()
text, binary = parse_xt(text_payload), parse_xb(payload)
assert compare_documents(text, binary).equivalent
assert [v.value for v in binary.nodes[0].fields[0].values] == [7, None]
for parsed in (text, binary):
    assert parsed.schema_resolution.to_dict() == expected_profile
for parser, data in ((parse_xt, text_payload), (parse_xb, payload)):
    try:
        parser(data, schema_provider=InMemorySchemaProvider())
    except SchemaError as error:
        assert error.diagnostic.code == "schema.missing_base_schema"
    else:
        raise AssertionError("explicit empty provider fell back")
    try:
        parser(data.replace(b"SCH_3000000_30000", b"SCH_3000001_30000"))
    except SchemaError as error:
        assert error.diagnostic.code == "schema.missing_base_schema"
    else:
        raise AssertionError("near key selected a profile")
Path("synthetic.x_t").write_bytes(text_payload)
Path("synthetic.x_b").write_bytes(payload)
assert "OCP" not in sys.modules and "cadquery" not in sys.modules
print(json.dumps({
    "version": parasolid_kit.__version__,
    "api": "imported",
    "interop_base": "missing_extras_actionable",
    "viewer_assets": sorted(preview.STATIC_ASSET_SHA256),
    "geometry_coverage_rows": len(occt.GEOMETRY_COVERAGE),
    "native_inspect_schema": header.schema_key,
    "schema_resolution": text.schema_resolution.to_dict(),
    "synthetic_pair_equivalent": True,
}))
""".strip()


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--sdist", type=Path, required=True)
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument(
        "--fixture",
        type=Path,
        action="append",
        default=[],
        help="separate local X_T or X_B fixture without a paired export (never bundled)",
    )
    parser.add_argument(
        "--fixture-pair",
        type=Path,
        nargs=2,
        action="append",
        default=[],
        metavar=("X_T", "X_B"),
        help="separate local real fixture pair; repeat for more pairs (never bundled)",
    )
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


def verify_install(
    artifact: Path,
    python: str,
    *,
    fixture_pairs: tuple[tuple[Path, Path], ...] = (),
    fixtures: tuple[Path, ...] = (),
) -> dict[str, object]:
    """Install one artifact with uv and run from outside the checkout."""

    artifact = artifact.resolve()
    fixture_reports = []
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
        runtime = [str(environment_python), "-I", "-c"]
        runtime_args = [str(ROOT), str(environment_path)]
        imported = _run(
            [*runtime, RUNTIME_GUARD_CODE + "\n" + BUILTIN_CODE + "\n" + SMOKE_CODE, *runtime_args],
            cwd=work_dir,
            environment=environment,
        )

        def runtime_cli(*args: str) -> dict[str, object]:
            return json.loads(
                _run(
                    [*runtime, RUNTIME_GUARD_CODE + "\n" + CLI_CODE, *runtime_args, *args],
                    cwd=work_dir,
                    environment=environment,
                )
            )

        for suffix in ("x_t", "x_b"):
            parsed = runtime_cli("parse", f"synthetic.{suffix}")
            assert (
                parsed["document"]["schema_resolution"] == json.loads(imported)["schema_resolution"]
            )
        assert runtime_cli("compare", "synthetic.x_t", "synthetic.x_b")["comparison"]["equivalent"]
        groups = [*fixture_pairs, *((p,) for p in fixtures)]
        for index, group in enumerate(groups):
            copied = []
            for source in group:
                if source.suffix.lower() not in (".x_t", ".x_b"):
                    raise ValueError("fixture must have an X_T or X_B extension")
                target = work_dir / f"fixture-{index}{source.suffix.lower()}"
                shutil.copyfile(source, target)
                copied.append(str(target))
            checked = json.loads(
                _run(
                    [
                        *runtime,
                        RUNTIME_GUARD_CODE + "\n" + BUILTIN_CODE + "\n" + FIXTURE_CODE,
                        *runtime_args,
                        *copied,
                    ],
                    cwd=work_dir,
                    environment=environment,
                )
            )
            for path, stream in zip(copied, checked["streams"], strict=True):
                assert runtime_cli("parse", path, "--brep")["brep"]["complete"]
                assert runtime_cli("check", path, "--json")["summary"] == stream["summary"]
            if len(copied) == 2:
                assert runtime_cli("compare", *copied)["comparison"]["equivalent"]
            fixture_reports.append(checked)
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
        "runtime_python_guards": ["network", "checkout_reads", "catalog_reads"],
        "real_fixture_pair_count": len(fixture_pairs),
        "real_single_fixture_count": len(fixtures),
        "real_fixtures": fixture_reports,
    }


def main() -> int:
    """Run isolated checks for both archive kinds."""

    arguments = _arguments()
    reports: list[dict[str, object]] = []
    try:
        for artifact in (arguments.wheel, arguments.sdist):
            reports.append(
                verify_install(
                    artifact,
                    arguments.python,
                    fixture_pairs=tuple(tuple(p) for p in arguments.fixture_pair),
                    fixtures=tuple(arguments.fixture),
                )
            )
    except (OSError, RuntimeError, AssertionError, json.JSONDecodeError) as error:
        report = {"status": "failed", "artifacts": reports, "error": str(error)}
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 1
    report = {"status": "passed", "artifacts": reports}
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
