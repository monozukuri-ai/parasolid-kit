#!/usr/bin/env python3
"""Verify every required release case against an explicit, hashed baseline."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import subprocess
import sys
from pathlib import Path, PurePosixPath
from typing import Any

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = Path(__file__).with_name("release_corpus_runtime.py")
ORACLE = Path(__file__).with_name("release_corpus_oracle.py")


def require(condition: object, message: str) -> None:
    if not condition:
        raise ValueError(message)


def checked_path(root: Path, name: str) -> Path:
    relative = PurePosixPath(name)
    require(
        bool(relative.parts)
        and not relative.is_absolute()
        and ".." not in relative.parts
        and relative.as_posix() == name
        and "\\" not in name
        and ":" not in name,
        f"path is not normalized and relative: {name}",
    )
    path = root
    for part in relative.parts:
        path = path / part
        require(not path.is_symlink(), f"symlink is not an immutable input: {name}")
    require(path.resolve().is_relative_to(root), f"path escapes input root: {name}")
    require(path.is_file(), f"required file is missing: {name}")
    return path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def check_artifact(root: Path, artifact: dict[str, Any], max_file_size: int) -> Path:
    path = checked_path(root, artifact["path"])
    size = path.stat().st_size
    require(0 < size <= max_file_size, f"file exceeds size limit or is empty: {path}")
    require(size == artifact["bytes"], f"size mismatch: {path}")
    require(sha256(path) == artifact["sha256"], f"SHA-256 mismatch: {path}")
    return path


def equal_json(actual: Any, expected: Any, location: str = "result") -> None:
    """Compare fixed reports; only floating values use the existing pair tolerance."""
    if isinstance(actual, float) and isinstance(expected, (int, float)):
        require(
            math.isclose(actual, expected, abs_tol=1e-12, rel_tol=1e-12),
            f"{location}: {actual!r} != {expected!r}",
        )
    elif isinstance(actual, dict) and isinstance(expected, dict):
        require(actual.keys() == expected.keys(), f"{location}: object keys differ")
        for key in actual:
            equal_json(actual[key], expected[key], f"{location}.{key}")
    elif isinstance(actual, list) and isinstance(expected, list):
        require(len(actual) == len(expected), f"{location}: array lengths differ")
        for index, (left, right) in enumerate(zip(actual, expected, strict=True)):
            equal_json(left, right, f"{location}[{index}]")
    else:
        require(
            type(actual) is type(expected) and actual == expected,
            f"{location}: {actual!r} != {expected!r}",
        )


def run_json(command: list[str], timeout: float) -> tuple[int, dict[str, Any]]:
    try:
        result = subprocess.run(
            command, capture_output=True, text=True, check=False, timeout=timeout
        )
    except subprocess.TimeoutExpired as error:
        raise ValueError(
            f"timeout after {timeout}s: {command!r}; stderr: {error.stderr!r}"
        ) from error
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ValueError(
            f"invalid subprocess JSON (exit {result.returncode}): {command!r}; "
            f"stdout: {result.stdout[-2000:]}; stderr: {result.stderr[-2000:]}"
        ) from error
    require(isinstance(value, dict), f"subprocess report is not an object: {command!r}")
    require(
        result.returncode in (0, 1),
        f"subprocess failed (exit {result.returncode}): {value}; stderr: {result.stderr}",
    )
    return result.returncode, value


def load_cases(
    manifest: Path, checks: Path, root: Path, max_file_size: int
) -> list[dict[str, Any]]:
    provenance_schema = json.loads((ROOT / "corpus/manifest.schema.json").read_text())
    release_schema = json.loads((ROOT / "corpus/release-checks.schema.json").read_text())
    entries = [json.loads(line) for line in manifest.read_text().splitlines() if line.strip()]
    rules = json.loads(checks.read_text())
    Draft202012Validator(release_schema).validate(rules)
    validator = Draft202012Validator(provenance_schema, format_checker=FormatChecker())
    for entry in entries:
        validator.validate(entry)
    by_id = {entry["id"]: entry for entry in entries}
    require(len(by_id) == len(entries), "duplicate manifest case ID")
    identifiers = [case["id"] for case in rules["cases"]]
    require(len(set(identifiers)) == len(identifiers), "duplicate required case ID")
    require(set(identifiers) == set(by_id), "required cases and manifest IDs differ")
    paths: set[str] = set()
    for case in rules["cases"]:
        entry = by_id[case["id"]]
        require(entry["path"] not in paths, f"duplicate fixture path: {entry['path']}")
        paths.add(entry["path"])
        path = check_artifact(root, {**entry, "bytes": case["bytes"]}, max_file_size)
        suffixes = (".x_t", ".xt") if entry["encoding"] == "text" else (".x_b", ".xb")
        require(path.suffix.lower() in suffixes, f"encoding/path mismatch: {path}")
        if entry["source_kind"] != "synthetic":
            require("provenance" in case, "real case requires a hashed provenance record")
        for key in ("baseline", "container", "provenance"):
            if case.get(key):
                check_artifact(root, case[key], max_file_size)
        if case["oracle"]:
            require(case["stage"] == "brep", "geometry oracle requires a B-Rep case")
            require(
                entry["generator"]["document_version"] == case["oracle"]["microversion"],
                "case and native oracle saved states differ",
            )
            for key in ("step", "body_details", "mass_properties"):
                check_artifact(root, case["oracle"][key], max_file_size)
        if "pair_id" in case:
            require(case["pair_id"] in by_id, "required pair is missing")
            other = by_id[case["pair_id"]]
            require(other["encoding"] != entry["encoding"], "pair must contain X_T and X_B")
            require(
                other["lineage_id"] == entry["lineage_id"]
                and other["generator"]["document_version"]
                == entry["generator"]["document_version"],
                "pair lineage or saved version differs",
            )
            case["pair_path"] = str(checked_path(root, other["path"]))
        case["entry"] = entry
        case["input_path"] = str(path)
    return rules["cases"]


def verify_case(
    case: dict[str, Any],
    root: Path,
    rust_probe: Path,
    python: str,
    oracle_python: str | None,
    timeout: float,
) -> dict[str, Any]:
    entry = case["entry"]
    stage = "raw" if case["stage"] == "diagnostic" else case["stage"]
    command = [python, "-I", str(RUNTIME), case["input_path"], entry["encoding"], stage]
    if case.get("pair_path") and entry["encoding"] == "text" and case["stage"] != "diagnostic":
        command.extend(["--pair", case["pair_path"]])
    python_exit, parsed = run_json(command, timeout)
    rust_exit, native = run_json(
        [str(rust_probe), case["input_path"], entry["encoding"], stage],
        timeout,
    )
    require(
        native["core_version"] == parsed["runtime"]["core_version"], "Rust/Python versions differ"
    )
    row = {
        "id": case["id"],
        "usage": case["usage"],
        "runtime": parsed["runtime"],
        "cli_exits": parsed["cli_exits"],
        "oracle": "not_requested",
    }
    if case["stage"] == "diagnostic":
        require(python_exit == rust_exit == 1, "expected diagnostic was not produced")
        for value in (parsed, native):
            require(value["status"] == "diagnostic", f"unexpected failure: {value}")
            equal_json({k: value[k] for k in ("code", "offset")}, case["diagnostic"], "diagnostic")
        return {**row, "status": "expected_diagnostic", "diagnostic": case["diagnostic"]}
    require(
        python_exit == rust_exit == 0, f"unexpected parser failure: {parsed.get('code')}; {native}"
    )
    require(parsed["status"] == native["status"] == "parsed", "unexpected parser status")
    require(parsed["schema_key"] == entry["expected"]["schema"], "internal schema key differs")
    for key in ("schema_key", "profile", "nodes", "termination", "brep"):
        equal_json(native[key], parsed[key], f"Rust/Python.{key}")
    equal_json(parsed["profile"], case["profile"], "profile")
    require(native["value_reencoding"] is True, "independent value reencoding was not measured")
    node_types = {node["node_type"] for node in parsed["nodes"]}
    require(
        set(entry["expected"]["required_node_types"]) <= node_types, "required node type missing"
    )
    if "node_count" in entry["expected"]:
        require(len(parsed["nodes"]) == entry["expected"]["node_count"], "node count differs")
    baseline = json.loads((root / case["baseline"]["path"]).read_text())
    equal_json(parsed["snapshot"], baseline, "regression baseline")
    if case["stage"] == "brep":
        require(
            parsed["brep"]["complete"] and parsed["brep"]["topology"]["valid"],
            "required complete B-Rep or valid topology is missing",
        )
        if "body_count" in entry["expected"]:
            require(
                parsed["brep"]["counts"]["bodies"] == entry["expected"]["body_count"],
                "body count differs",
            )
    if case["oracle"]:
        require(oracle_python is not None, "required geometry oracle interpreter is missing")
        oracle_exit, oracle = run_json(
            [
                oracle_python,
                "-I",
                str(ORACLE),
                str(root),
                case["input_path"],
                json.dumps(case["oracle"]),
            ],
            timeout,
        )
        require(
            oracle_exit == 0 and oracle["status"] == "passed", f"geometry oracle failed: {oracle}"
        )
        row["oracle"] = oracle
    return {
        **row,
        "status": "passed",
        "stage": case["stage"],
        "nodes": len(parsed["nodes"]),
        "pair_equivalent": parsed["pair_equivalent"],
        "value_reencoding": True,
        "schema_blobs_replayed": native["schema_blobs_replayed"],
    }


def verify_release_corpus(
    manifest: Path,
    checks: Path,
    *,
    root: Path,
    rust_probe: Path,
    python: str = sys.executable,
    oracle_python: str | None = None,
    timeout: float = 30,
    max_file_size: int = 256 * 1024 * 1024,
) -> dict[str, Any]:
    report: dict[str, Any] = {"status": "failed", "cases": [], "errors": []}
    try:
        require(math.isfinite(timeout) and timeout > 0, "timeout must be finite and positive")
        require(max_file_size > 0, "maximum file size must be positive")
        root = root.resolve()
        cases = load_cases(manifest, checks, root, max_file_size)
        require(rust_probe.is_file(), "required Rust probe is missing")
        require(
            oracle_python is not None or not any(c["oracle"] for c in cases),
            "required geometry oracle interpreter is missing",
        )
        report.update(
            manifest_sha256=sha256(manifest),
            checks_sha256=sha256(checks),
            rust_probe_sha256=sha256(rust_probe),
        )
        for case in cases:
            try:
                row = verify_case(case, root, rust_probe.resolve(), python, oracle_python, timeout)
            except (OSError, ValueError, KeyError, TypeError) as error:
                row = {"id": case["id"], "status": "failed", "error": str(error)}
            report["cases"].append(row)
        report["status"] = (
            "passed" if all(c["status"] != "failed" for c in report["cases"]) else "failed"
        )
    except (OSError, ValueError, KeyError, TypeError) as error:
        report["errors"].append(str(error))
    except Exception as error:
        # Includes JSON-schema validation failures; never count malformed input as a skip.
        report["errors"].append(f"{type(error).__name__}: {error}")
    report["counts"] = {
        "required": len(report["cases"]),
        "parsed": sum(c["status"] == "passed" for c in report["cases"]),
        "expected_diagnostics": sum(c["status"] == "expected_diagnostic" for c in report["cases"]),
        "failed": sum(c["status"] == "failed" for c in report["cases"]),
    }
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--checks", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--rust-probe", type=Path, required=True)
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument("--oracle-python")
    parser.add_argument("--timeout", type=float, default=30)
    args = parser.parse_args()
    report = verify_release_corpus(**vars(args))
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
