#!/usr/bin/env python3
"""Set the Cargo version and refresh release lockfiles (Python 3.11+)."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path

if __package__:
    from .verify_release import ROOT, python_version, source_versions, versions
else:
    from verify_release import ROOT, python_version, source_versions, versions


def bump_version(version: str, root: Path = ROOT) -> tuple[str, str]:
    python_version(version)
    source_versions(root)
    manifest = root / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8")
    updated, count = re.subn(
        r'(\[workspace\.package\]\s*\n(?:[^\[\n]*\n)*?version\s*=\s*)"[^"]+"',
        lambda match: f'{match[1]}"{version}"',
        text,
    )
    if count != 1:
        raise ValueError("expected one workspace.package.version in Cargo.toml")
    manifest.write_text(updated, encoding="utf-8")
    for command in (
        ["cargo", "update", "--workspace", "--offline"],
        ["cargo", "update", "--workspace", "--offline", "--manifest-path", "fuzz/Cargo.toml"],
        ["uv", "lock"],
    ):
        subprocess.run(command, cwd=root, check=True)
    return versions(root)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="Cargo version, e.g. 0.4.0, 0.4.0-rc.1 or 0.4.0-dev1")
    args = parser.parse_args()
    try:
        python, rust = bump_version(args.version)
    except ValueError as error:
        parser.error(str(error))
    print(f"Rust {rust}; Python {python}; release tag v{python}")


if __name__ == "__main__":
    main()
