#!/usr/bin/env python3
"""Bind a release to one successful candidate run and privately verified artifacts.

Maintainer tool (Python >= 3.11). The receipt is a maintainer attestation, not a
replacement for the private reports. Only hashes and gate status leave the host.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 cold-install test environments.
    import tomli as tomllib

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "monozukuri-ai/parasolid-kit"
GATES = frozenset({"private_wheel", "private_sdist", "downstream", "robustness"})
PLATFORMS = ("manylinux", "win_amd64", "macosx_x86_64", "macosx_arm64")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def python_version(rust: str) -> str:
    """Convert the supported Cargo release versions to Maturin's PEP 440 form."""
    number = r"(?:0|[1-9]\d*)"
    match = re.fullmatch(
        rf"({number}\.{number}\.{number})(?:-rc\.({number})|-dev({number}))?", rust
    )
    if not match:
        raise ValueError(f"unsupported release version: {rust}")
    python = match[1]
    if match[2]:
        python += f"rc{match[2]}"
    elif match[3]:
        python += f".dev{match[3]}"
    return python


def source_versions(root: Path = ROOT) -> tuple[str, str]:
    """Read the single version definition without importing the built package."""
    project = tomllib.loads((root / "pyproject.toml").read_text(encoding="utf-8"))["project"]
    if "version" in project or "version" not in project.get("dynamic", []):
        raise ValueError("pyproject.toml must derive its version from Cargo.toml")
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    rust = cargo["workspace"]["package"]["version"]
    return python_version(rust), rust


def runtime_version_check(root: Path = ROOT) -> str:
    """Embed source expectations before a cold runtime is isolated from the checkout."""
    python, rust = source_versions(root)
    return (
        "from importlib.metadata import version\n"
        "import parasolid_kit\n"
        "from parasolid_kit import _core\n"
        f"assert parasolid_kit.__version__ == version('parasolid-kit') == {python!r}\n"
        f"assert _core.CORE_VERSION == {rust!r}\n"
    )


def versions(root: Path = ROOT, tag: str | None = None) -> tuple[str, str]:
    python, rust = source_versions(root)
    for filename in ("Cargo.lock", "fuzz/Cargo.lock", "uv.lock"):
        lock = tomllib.loads((root / filename).read_text(encoding="utf-8"))
        packages = {p["name"]: p for p in lock["package"]}
        if filename == "uv.lock":
            package = packages.get("parasolid-kit", {})
            if package.get("source") != {"editable": "."} or "version" in package:
                raise ValueError("uv.lock: parasolid-kit must use dynamic local metadata")
            continue
        expected = {"parasolid-core": rust, "parasolid-python": rust}
        for name, value in expected.items():
            if name == "parasolid-python" and filename.startswith("fuzz/"):
                continue
            if packages.get(name, {}).get("version") != value:
                raise ValueError(f"{filename}: {name} version differs")
    if tag is not None and tag != f"v{python}":
        raise ValueError("release tag differs from package version")
    return python, rust


def validate_receipt(receipt: dict, run: dict, sha: str, tag: str) -> None:
    python, rust = versions(tag=tag)
    if (
        receipt.get("schema_version") != 1
        or receipt.get("repository") != REPOSITORY
        or receipt.get("source_sha") != sha
        or not re.fullmatch(r"[a-f0-9]{40}", sha)
        or receipt.get("python_version") != python
        or receipt.get("rust_version") != rust
    ):
        raise ValueError("receipt source or version differs from release")
    if (
        type(receipt.get("candidate_run_id")) is not int
        or receipt["candidate_run_id"] != run.get("id")
        or receipt.get("candidate_run_attempt") != run.get("run_attempt")
        or run.get("head_sha") != sha
        or run.get("status") != "completed"
        or run.get("conclusion") != "success"
        or run.get("path") != ".github/workflows/release.yml"
        or run.get("repository", {}).get("full_name") != REPOSITORY
        or run.get("head_repository", {}).get("full_name") != REPOSITORY
        or run.get("event") not in {"push", "workflow_dispatch"}
        or not run.get("head_branch", "").startswith("release/")
    ):
        raise ValueError("candidate must be a successful release-branch run at the exact commit")
    gates = receipt.get("gates", {})
    if set(gates) != GATES:
        raise ValueError("required private release gates are missing")
    for name, gate in gates.items():
        if (
            set(gate) != {"status", "report_sha256"}
            or gate["status"] != "passed"
            or not re.fullmatch(r"[a-f0-9]{64}", gate["report_sha256"])
        ):
            raise ValueError(f"private gate did not pass: {name}")


def artifact_hashes(directory: Path) -> dict[str, str]:
    files = sorted(p for p in directory.iterdir() if p.is_file())
    if any(p.is_symlink() or not p.name.endswith((".whl", ".tar.gz", ".crate")) for p in files):
        raise ValueError("unexpected release artifact")
    if any(p.is_dir() for p in directory.iterdir()):
        raise ValueError("release artifacts must be flat")
    python, rust = versions()
    wheels = [p.name for p in files if p.suffix == ".whl"]
    if len(wheels) != 4 or any(
        not n.startswith(f"parasolid_kit-{python}-cp310-abi3-") for n in wheels
    ):
        raise ValueError("expected four abi3 candidate wheels")
    for marker in PLATFORMS:
        if marker.startswith("macosx_"):
            matches = [n for n in wheels if "macosx_" in n and n.endswith(marker[7:] + ".whl")]
        else:
            matches = [n for n in wheels if marker in n]
        if len(matches) != 1:
            raise ValueError(f"missing or duplicated platform: {marker}")
    expected = {*wheels, f"parasolid_kit-{python}.tar.gz", f"parasolid-core-{rust}.crate"}
    if {p.name for p in files} != expected:
        raise ValueError("candidate sdist or core crate missing")
    return {p.name: digest(p) for p in files}


def verify_artifacts(receipt: dict, directory: Path) -> None:
    if receipt.get("artifacts") != artifact_hashes(directory):
        raise ValueError("artifacts differ from privately verified candidate hashes")


def gh(*args: str) -> str:
    return subprocess.check_output(["gh", *args], text=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag")
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--artifacts", type=Path)
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--require-core-published", action="store_true")
    args = parser.parse_args()
    python, rust = versions(tag=args.tag)
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    outputs = {"source_sha": sha, "python_version": python, "rust_version": rust}
    if args.receipt:
        receipt = json.loads(args.receipt.read_text())
        run_id = receipt["candidate_run_id"]
        if type(run_id) is not int or run_id <= 0:
            raise ValueError("invalid candidate run id")
        run = json.loads(gh("api", f"repos/{REPOSITORY}/actions/runs/{run_id}"))
        validate_receipt(receipt, run, sha, args.tag or f"v{python}")
        outputs["candidate_run_id"] = str(run_id)
        if args.artifacts:
            verify_artifacts(receipt, args.artifacts)
        if args.require_core_published:
            # Verify the registry checksum, not merely existence of the version.
            import urllib.request

            request = urllib.request.Request(
                f"https://crates.io/api/v1/crates/parasolid-core/{rust}",
                headers={"User-Agent": "parasolid-kit release verification"},
            )
            with urllib.request.urlopen(request, timeout=30) as response:
                version = json.load(response)["version"]
            if (
                version["yanked"]
                or version["checksum"] != receipt["artifacts"][f"parasolid-core-{rust}.crate"]
            ):
                raise ValueError("published Rust core differs from verified candidate")
    elif args.artifacts or args.require_core_published:
        parser.error("artifact and registry verification require --receipt")
    if args.github_output:
        with args.github_output.open("a") as stream:
            stream.writelines(f"{key}={value}\n" for key, value in outputs.items())
    print(json.dumps(outputs, indent=2))


if __name__ == "__main__":
    main()
