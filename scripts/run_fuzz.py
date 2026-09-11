#!/usr/bin/env python3
"""Run one bounded sanitizer campaign and retain logs and reproducer inputs."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", choices=("inspect", "parse", "schema_catalog"))
    parser.add_argument("--seconds", type=int, default=600)
    parser.add_argument("--runs", type=int)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path)
    args = parser.parse_args()
    if not 1 <= args.seconds <= 3600 or (args.runs is not None and args.runs < 1):
        parser.error("seconds must be 1..3600 and runs must be positive")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    corpus, artifacts = output / "corpus", output / "artifacts"
    artifacts.mkdir()
    corpus.mkdir()
    if args.target in ("inspect", "parse"):
        from prepare_fuzz_corpus import build_seeds

        for name, data in build_seeds().items():
            (corpus / name).write_bytes(data)
    command = ["cargo", "+nightly", "fuzz", "run", args.target, str(corpus)]
    if args.target_dir:
        command.extend(["--target-dir", str(args.target_dir.resolve())])
    command.extend(
        [
            "--",
            "-max_len=65536",
            "-timeout=5",
            "-rss_limit_mb=1024",
            f"-max_total_time={args.seconds}",
            "-print_final_stats=1",
            "-seed=9401",
            f"-artifact_prefix={artifacts}/",
        ]
    )
    if args.runs is not None:
        command.append(f"-runs={args.runs}")
    report = {
        "status": "running",
        "target": args.target,
        "command": command,
        "max_input_bytes": 65536,
        "input_timeout_seconds": 5,
        "rss_limit_mb": 1024,
        "seconds": args.seconds,
        "runs": args.runs,
        "seed_sha256": {
            p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(corpus.iterdir())
        },
    }
    started = time.monotonic()
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    with (output / "run.log").open("w") as log:
        process = subprocess.Popen(
            command,
            cwd=ROOT / "fuzz",
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            report["exit_code"] = process.wait(timeout=args.seconds + 600)
            report["status"] = "passed" if process.returncode == 0 else "failed"
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            report["status"] = "process_timeout"
    report["wall_seconds"] = time.monotonic() - started
    report["artifacts"] = {
        p.name: hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted(artifacts.iterdir())
        if p.is_file()
    }
    if report["artifacts"]:
        report["status"] = "failed"
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps(
            {
                key: report[key]
                for key in ("status", "target", "wall_seconds", "exit_code")
                if key in report
            }
        )
    )
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
