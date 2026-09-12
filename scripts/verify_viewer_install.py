#!/usr/bin/env python3
"""Cold-install wheel/sdist with OCCT and check their bundled UI outside the source tree.

Node/Playwright drive a browser in the parent harness only. The installed Python
runtime has an empty PATH and denies process launches, source reads and external
networking. Input geometry is a public synthetic B-Rep, not a parser qualification.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], cwd: Path, env: dict[str, str], log: Path) -> None:
    with log.open("w", encoding="utf-8") as output:
        result = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
            check=False,
            timeout=600,
        )
    if result.returncode:
        raise RuntimeError(f"command exited {result.returncode}; see {log}: {command!r}")


def check_install(
    artifact: Path,
    source: Path,
    python: str,
    node: str,
    uv: str,
    root: Path,
    output: Path,
    environment: dict[str, str],
) -> dict[str, Any]:
    root.mkdir()
    output.mkdir(parents=True, exist_ok=True)
    venv = root / "environment"
    run(
        [uv, "venv", "--no-project", "--no-cache", "--python", python, str(venv)],
        root,
        environment,
        output / "venv.log",
    )
    executable = venv / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    run(
        [
            uv,
            "pip",
            "install",
            "--python",
            str(executable),
            "--no-cache",
            f"parasolid-kit[occt] @ {artifact.as_uri()}",
        ],
        root,
        environment,
        output / "install.log",
    )
    # Only the probe and its public pre-conversion geometry travel into the runtime tree.
    probe = root / "probe"
    for name in ("viewer/tests/serve_previews.py", "tests/_occt_fixtures.py"):
        target = probe / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / name, target)
    runtime_env = {**environment, "PATH": "", "PYTHONDONTWRITEBYTECODE": "1"}
    command = [
        str(executable),
        "-I",
        str(probe / "viewer/tests/serve_previews.py"),
        "--directory",
        str(root / "previews"),
        "--forbid-root",
        str(ROOT),
        "--forbid-root",
        str(source),
        "--environment",
        str(venv),
    ]
    with (output / "runtime.log").open("w", encoding="utf-8") as log:
        child = subprocess.Popen(
            command,
            cwd=root,
            env=runtime_env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log,
            text=True,
        )
        try:
            ready: queue.Queue[str] = queue.Queue()
            threading.Thread(target=lambda: ready.put(child.stdout.readline()), daemon=True).start()
            try:
                line = ready.get(timeout=60)
            except queue.Empty as error:
                raise RuntimeError(
                    f"viewer runtime startup timed out; see {output / 'runtime.log'}"
                ) from error
            if not line:
                raise RuntimeError(
                    f"viewer runtime failed before startup; see {output / 'runtime.log'}"
                )
            config = json.loads(line)
            assert config["runtime_guard"] is True
            assert Path(config["package"]).is_relative_to(venv)
            config_path = root / "browser-config.json"
            config_path.write_text(json.dumps(config), encoding="ascii")
            run(
                [
                    node,
                    str(ROOT / "viewer/tests/browser.mjs"),
                    "--config",
                    str(config_path),
                    "--report-dir",
                    str(output),
                ],
                root,
                environment,
                output / "browser.log",
            )
            browser = json.loads((output / "browser.json").read_text(encoding="utf-8"))
            assert browser["status"] == "passed"
            child.stdin.close()
            if child.wait(timeout=15) != 0:
                raise RuntimeError(
                    f"viewer runtime failed on shutdown; see {output / 'runtime.log'}"
                )
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=15)
            child.stdout.close()
            if not child.stdin.closed:
                child.stdin.close()
    return {
        "status": "passed",
        "artifact": str(artifact),
        "artifact_sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
        "python": config["python"],
        "installed_package": config["package"],
        "runtime_guard": config["runtime_guard"],
        "runtime_path": "",
        "source_oracle": "public BrepModel before conversion",
        "generated_files_sha256": config["files"],
        "browser": browser,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--sdist", type=Path, required=True)
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument("--chrome", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report: dict[str, Any] = {"status": "running", "platform": sys.platform, "installs": {}}
    try:
        wheel, sdist = args.wheel.resolve(), args.sdist.resolve()
        env = os.environ.copy()
        for name in ("PYTHONPATH", "VIRTUAL_ENV", "UV_PROJECT_ENVIRONMENT"):
            env.pop(name, None)
        if args.chrome:
            env["VIEWER_CHROME"] = str(args.chrome.resolve())
        node, uv, npm = shutil.which("node"), shutil.which("uv"), shutil.which("npm")
        if node is None or uv is None or npm is None:
            raise FileNotFoundError("uv, Node/npm and Playwright are required by the test harness")
        run(
            [
                sys.executable,
                str(ROOT / "scripts/verify_artifacts.py"),
                "--wheel",
                str(wheel),
                "--sdist",
                str(sdist),
                "--require-license",
            ],
            ROOT,
            env,
            output / "archives.json",
        )
        # Archive gate rejects traversal, links, special files and unapproved members first.
        with tempfile.TemporaryDirectory(prefix="parasolid-viewer-cold-") as temporary:
            root = Path(temporary)
            extracted = root / "source"
            with tarfile.open(sdist) as archive:
                for member in archive.getmembers():
                    if member.isfile():
                        target = extracted / member.name
                        target.parent.mkdir(parents=True, exist_ok=True)
                        with archive.extractfile(member) as src, target.open("wb") as dst:
                            shutil.copyfileobj(src, dst)
            (source,) = extracted.iterdir()
            # Reproduce bundled bytes using only the frontend sources in the sdist.
            # Node/npm are development tools here; neither is available in the runtime below.
            run([npm, "ci"], source / "viewer", env, output / "frontend-install.log")
            run([npm, "run", "build"], source / "viewer", env, output / "frontend-build.log")
            report["sdist_frontend_rebuild"] = "passed"
            for name, artifact in (("wheel", wheel), ("sdist", sdist)):
                report["installs"][name] = check_install(
                    artifact,
                    source,
                    args.python,
                    node,
                    uv,
                    root / name,
                    output / name,
                    env,
                )
        report["status"] = "passed"
    except Exception as error:
        report.update(status="failed", error_type=type(error).__name__, error=str(error))
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "report": str(output / "report.json")}))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
