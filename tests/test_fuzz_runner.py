"""The campaign entry point must retain and fail on reproducer artifacts."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.skipif(os.name != "posix", reason="sanitizer runner uses POSIX process groups")
@pytest.mark.parametrize("exit_code", [0, 77])
def test_campaign_reproducer_is_retained_and_never_reported_as_passed(tmp_path, exit_code):
    cargo = tmp_path / "cargo"
    cargo.write_text(
        f"#!{sys.executable}\n"
        "import pathlib, sys\n"
        "prefix = next(a.split('=', 1)[1] for a in sys.argv if a.startswith('-artifact_prefix='))\n"
        "pathlib.Path(prefix, 'crash-fixture').write_bytes(b'reproduce')\n"
        "print('synthetic fuzzer failure for runner test')\n"
        f"sys.exit({exit_code})\n"
    )
    cargo.chmod(0o755)
    output = tmp_path / "campaign"
    result = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/run_fuzz.py"),
            "parse",
            "--runs",
            "1",
            "--output",
            str(output),
        ],
        env={**os.environ, "PATH": str(tmp_path) + os.pathsep + os.environ["PATH"]},
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    report = json.loads((output / "report.json").read_text())
    assert report["status"] == "failed"
    assert report["exit_code"] == exit_code
    assert "crash-fixture" in report["artifacts"]
    assert (output / "artifacts/crash-fixture").read_bytes() == b"reproduce"
    assert "synthetic fuzzer failure" in (output / "run.log").read_text()
