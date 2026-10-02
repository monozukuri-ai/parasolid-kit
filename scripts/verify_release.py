#!/usr/bin/env python3
"""Verify source versions and the distribution hashes produced by release CI."""

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


def artifact_hashes(directory: Path, root: Path = ROOT) -> dict[str, str]:
    files = sorted(p for p in directory.iterdir() if p.is_file())
    if any(p.is_symlink() or not p.name.endswith((".whl", ".tar.gz", ".crate")) for p in files):
        raise ValueError("unexpected release artifact")
    if any(p.is_dir() for p in directory.iterdir()):
        raise ValueError("release artifacts must be flat")
    python, rust = versions(root)
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


def verify_artifacts(expected: dict[str, str], directory: Path, root: Path = ROOT) -> None:
    if expected != artifact_hashes(directory, root):
        raise ValueError("artifacts differ from the distributions verified by CI")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag")
    parser.add_argument("--artifacts", type=Path)
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--expected-hashes", help="JSON hashes from this run's distribution job")
    parser.add_argument("--source-root", type=Path, default=ROOT)
    args = parser.parse_args()
    root = args.source_root.resolve()
    python, rust = versions(root, tag=args.tag)
    sha = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    outputs = {"source_sha": sha, "python_version": python, "rust_version": rust}
    if args.expected_hashes is not None and args.artifacts is None:
        parser.error("expected hashes require --artifacts")
    if args.artifacts:
        if args.expected_hashes is not None:
            verify_artifacts(json.loads(args.expected_hashes), args.artifacts, root)
        outputs["artifact_hashes"] = json.dumps(
            artifact_hashes(args.artifacts, root), sort_keys=True
        )
    if args.github_output:
        with args.github_output.open("a") as stream:
            stream.writelines(f"{key}={value}\n" for key, value in outputs.items())
    print(json.dumps(outputs, indent=2))


if __name__ == "__main__":
    main()
