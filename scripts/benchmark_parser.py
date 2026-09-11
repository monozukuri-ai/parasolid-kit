#!/usr/bin/env python3
"""Measure bounded parser stages on Linux; input fixtures remain outside the package."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import math
import os
import platform
import resource
import signal
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STRICT_LIMITS = {
    "max_file_size": 65536,
    "max_nodes": 2048,
    "max_schema_types": 1024,
    "max_fields_per_type": 128,
    "max_string_bytes": 8192,
    "max_variable_elements": 4096,
    "max_diagnostics": 256,
}


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def python_worker(mode: str, encoding: str, filename: str) -> None:
    from parasolid_kit import ParseLimits, _core, map_brep, parse_xb, parse_xt

    limits = ParseLimits(**STRICT_LIMITS)
    path = Path(filename)
    limits.ensure_file_size(path.stat().st_size)
    data = path.read_bytes()
    parser = parse_xb if encoding == "binary" else parse_xt
    document = parser(data, limits=limits)
    key, nodes = document.header.schema_key, len(document.nodes)
    if mode == "parse":
        del document

        def operation():
            return parser(data, limits=limits)
    else:
        del_result = map_brep(document, limits=limits)
        del del_result

        def operation():
            return map_brep(document, limits=limits)

    gc.collect()
    started = time.perf_counter()
    result = operation()
    elapsed = time.perf_counter() - started
    if mode == "brep":
        assert result.complete
    print(
        json.dumps(
            {
                "status": "passed",
                "mode": mode,
                "encoding": encoding,
                "seconds": elapsed,
                "nodes": nodes,
                "schema_key": key,
                "bodies": len(result.bodies) if mode == "brep" else None,
                "warmup": 1,
                "python": sys.version,
                "executable": sys.executable,
                "native_extension": _core.__file__,
                "native_sha256": sha(Path(_core.__file__)),
            }
        )
    )


def child_limits() -> None:
    resource.setrlimit(resource.RLIMIT_AS, (256 * 1024 * 1024,) * 2)
    resource.setrlimit(resource.RLIMIT_CPU, (10, 10))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def sample(command: list[str], output: Path) -> dict:
    rss = output.with_suffix(".rss-kib")
    process = subprocess.Popen(
        ["/usr/bin/time", "-f", "%M", "-o", str(rss), *command],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
        preexec_fn=child_limits,
    )
    try:
        stdout, stderr = process.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        stdout, stderr = process.communicate()
        output.with_suffix(".stderr").write_text(stderr)
        raise RuntimeError(f"20 second process timeout: {command}") from None
    output.with_suffix(".stderr").write_text(stderr)
    if process.returncode != 0:
        raise RuntimeError(f"exit {process.returncode}: {command}: {stderr[-2000:]}")
    result = json.loads(stdout)
    if result.get("status") != "passed":
        raise RuntimeError(f"worker did not pass: {result}")
    if not math.isfinite(result["seconds"]) or result["seconds"] < 0:
        raise ValueError("worker returned an invalid elapsed time")
    result["peak_rss_kib"] = int(rss.read_text().strip())
    output.write_text(json.dumps(result, indent=2) + "\n")
    return result


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "--worker":
        python_worker(*sys.argv[2:])
        return 0
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--rust-probe", required=True, type=Path)
    parser.add_argument("--consumer-probe", type=Path)
    parser.add_argument("--python", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--baseline", type=Path)
    args = parser.parse_args()
    if args.repeats < 5:
        parser.error("at least five samples are required")
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = json.loads(args.manifest.read_text())
    report = {
        "status": "passed",
        "cases": [],
        "errors": [],
        "repeat_count": args.repeats,
        "manifest_sha256": sha(args.manifest),
        "strict_limits": STRICT_LIMITS,
        "process_limits": {
            "address_space_bytes": 256 * 1024 * 1024,
            "cpu_seconds": 10,
            "wall_seconds": 20,
        },
        "platform": platform.platform(),
        "machine": platform.machine(),
        "cpu": next(
            line.split(":", 1)[1].strip()
            for line in Path("/proc/cpuinfo").read_text().splitlines()
            if line.startswith("model name")
        ),
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "working_diff_sha256": hashlib.sha256(
            subprocess.check_output(["git", "diff", "--binary", "HEAD"], cwd=ROOT)
        ).hexdigest(),
        "build_profile": "release",
        "warmup_per_process": 1,
        "timing_scope": (
            "stage only, preloaded input; B-Rep excludes parse; "
            "process RSS includes startup and warmup"
        ),
        "probe_sha256": {str(p): sha(p) for p in [args.rust_probe, args.consumer_probe] if p},
    }
    for case in manifest["cases"]:
        try:
            path = Path(case["path"]).resolve()
            if sha(path) != case["sha256"]:
                raise ValueError(f"fixture hash changed: {path}")
            modes = (
                ["partial_integration"]
                if case["kind"] == "partial"
                else ["rust_parse", "rust_brep", "python_parse", "python_brep"]
            )
            for mode in modes:
                if mode == "partial_integration":
                    if args.consumer_probe is None:
                        raise ValueError("partial cases require --consumer-probe")
                    command = [str(args.consumer_probe.resolve()), str(path)]
                else:
                    language, stage = mode.split("_")
                    command = (
                        [str(args.rust_probe.resolve())]
                        if language == "rust"
                        else [
                            str(args.python.absolute()),
                            "-I",
                            str(Path(__file__).resolve()),
                            "--worker",
                        ]
                    )
                    command += [stage, case["encoding"], str(path)]
                samples = [
                    sample(command, args.output / f"{case['id']}-{mode}-{i}.json")
                    for i in range(args.repeats)
                ]
                for result in samples:
                    for key in [
                        "nodes",
                        "bodies",
                        "schema_key",
                        "consumer_status",
                        "geometry_transferred",
                    ]:
                        if (
                            key in case["expected"]
                            and key in result
                            and result[key] is not None
                            and result[key] != case["expected"][key]
                        ):
                            raise ValueError(f"unexpected {key}: {case['id']} {mode}")
                report["cases"].append(
                    {
                        "id": case["id"],
                        "mode": mode,
                        "input_bytes": path.stat().st_size,
                        "sha256": case["sha256"],
                        "size_class": case["size_class"],
                        "median_seconds": statistics.median(s["seconds"] for s in samples),
                        "peak_rss_kib": max(s["peak_rss_kib"] for s in samples),
                        "median_rss_kib": statistics.median(s["peak_rss_kib"] for s in samples),
                        "sample_count": len(samples),
                        "first_result": samples[0],
                    }
                )
        except (OSError, ValueError, RuntimeError) as error:
            report["status"] = "failed"
            report["errors"].append(str(error))
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        for key in (
            "manifest_sha256",
            "platform",
            "machine",
            "cpu",
            "rustc",
            "build_profile",
            "strict_limits",
            "process_limits",
            "repeat_count",
        ):
            if baseline[key] != report[key]:
                raise ValueError(f"comparison requires the same {key}")
        previous = {(r["id"], r["mode"]): r for r in baseline["cases"]}
        report["review_regressions"] = []
        for row in report["cases"]:
            old = previous[(row["id"], row["mode"])]
            for metric in ["median_seconds", "peak_rss_kib"]:
                if row[metric] > old[metric] * 1.2:
                    report["review_regressions"].append(
                        {
                            "id": row["id"],
                            "mode": row["mode"],
                            "metric": metric,
                            "ratio": row[metric] / old[metric],
                        }
                    )
        if report["review_regressions"] and report["status"] == "passed":
            report["status"] = "needs_review"
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps(
            {
                "status": report["status"],
                "measurements": len(report["cases"]),
                "errors": report["errors"],
            }
        )
    )
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
