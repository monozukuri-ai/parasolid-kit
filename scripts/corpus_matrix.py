"""Run every Parasolid file of one or more directories through parse, B-Rep mapping and OCCT.

Each file runs in its own process with a timeout, so one failure or hang does
not stop the run. The result is one JSON object per file, printed as a table
and written to ``--json``. Nothing is downloaded and no file is copied: point
the script at a local corpus such as the public files named in
``docs/format-support.md``.

    python scripts/corpus_matrix.py DIR [DIR ...] --json matrix.json

With ``--probe FILE`` the script runs the probe of one file in-process and
prints its JSON line; the matrix uses that mode for its subprocesses. The exit
status is 1 when any file fails to parse, map completely or convert strictly,
which ``tests/test_public_corpus.py`` turns into a regression check against an
optional ``expected.json`` in the corpus directory.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path

SUFFIXES = {".x_t", ".x_b", ".xmt_txt", ".xmt_bin"}


def probe(path: Path) -> dict[str, object]:
    """Parse, map and convert one file; never raise."""
    from parasolid_kit import ParasolidError, read_brep

    row: dict[str, object] = {"path": path.name}
    lower = path.name.lower()
    source_format = "x-b" if lower.endswith((".x_b", ".xmt_bin")) else "x-t"
    try:
        from parasolid_kit import inspect_xb, inspect_xt

        header = (inspect_xb if source_format == "x-b" else inspect_xt)(path)
        row["key"] = header.schema_key
    except ParasolidError as error:
        row["key"] = f"inspect failed: {type(error).__name__}"
    started = time.perf_counter()
    try:
        parsed = read_brep(path, source_format=source_format)
    except ParasolidError as error:
        row["parse"] = f"{type(error).__name__}: {str(error)[:200]}"
        details = getattr(error, "diagnostic", None)
        row["parse_details"] = dict(details.details) if details is not None else {}
        row["parse_s"] = round(time.perf_counter() - started, 2)
        return row
    row["parse_s"] = round(time.perf_counter() - started, 2)
    summary = parsed.summary
    row["key"] = parsed.brep.schema_key
    row["profile"] = getattr(summary.schema_resolution, "profile_id", None)
    row["complete"] = parsed.complete
    row["bodies"] = summary.counts.bodies
    row["faces"] = summary.counts.faces
    row["attributes"] = len(parsed.brep.attributes)
    row["brep_diagnostics"] = dict(Counter(item.code for item in parsed.brep.diagnostics))
    try:
        from parasolid_kit.interop.errors import OcctConversionError
        from parasolid_kit.interop.occt import to_occt
    except ImportError:
        row["occt"] = "skipped: OCP is not installed"
        return row
    for strict in (True, False):
        started = time.perf_counter()
        try:
            result = to_occt(
                parsed.brep,
                source_unit="m",
                target_unit="mm",
                require_complete=strict,
                partial=not strict,
            )
        except OcctConversionError as error:
            row["occt"] = f"fail(strict={strict}): {str(error)[:200]}"
            if strict:
                row["occt_strict"] = row["occt"]
                row["occt_strict_details"] = dict(error.diagnostic.details)
            row["occt_s"] = round(time.perf_counter() - started, 2)
            continue
        report = result.report
        row["occt"] = f"ok(strict={strict})"
        row["occt_s"] = round(time.perf_counter() - started, 2)
        row["occt_valid"] = report.occt_valid
        row["faces_dropped"] = len(report.faces_dropped)
        row["bodies_failed"] = len(report.bodies_failed)
        row["occt_diagnostics"] = dict(Counter(item.code for item in report.diagnostics))
        break
    return row


def files_of(directories: list[Path]) -> list[Path]:
    result = []
    for directory in directories:
        for path in sorted(directory.iterdir()):
            if path.is_file() and path.suffix.lower() in SUFFIXES:
                result.append(path)
    return result


def run_matrix(directories: list[Path], python: str, timeout: float) -> list[dict[str, object]]:
    rows = []
    for path in files_of(directories):
        command = [python, "-I", str(Path(__file__).resolve()), "--probe", str(path)]
        try:
            completed = subprocess.run(
                command, capture_output=True, text=True, timeout=timeout, check=False
            )
        except subprocess.TimeoutExpired:
            rows.append({"path": path.name, "fatal": f"timeout after {timeout:g} s"})
            continue
        line = completed.stdout.strip().splitlines()[-1] if completed.stdout.strip() else ""
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            row = {
                "path": path.name,
                "fatal": f"exit {completed.returncode}: {completed.stderr.strip()[-300:]}",
            }
        rows.append(row)
    return rows


def is_success(row: dict[str, object]) -> bool:
    return (
        "fatal" not in row
        and "parse" not in row
        and bool(row.get("complete"))
        and str(row.get("occt", "")).startswith(("ok(strict=True)", "skipped"))
    )


def print_table(rows: list[dict[str, object]]) -> None:
    for row in rows:
        status = row.get("fatal") or row.get("parse") or row.get("occt") or "?"
        print(
            f"{str(row['path'])[:48]:<48} {str(row.get('key', ''))[:28]:<28} "
            f"complete={row.get('complete', '')!s:<5} faces={row.get('faces', ''):<6} {status}"
        )
    print(f"{sum(is_success(row) for row in rows)} of {len(rows)} files parse, map and convert")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("directories", nargs="*", type=Path)
    parser.add_argument("--probe", type=Path, help="probe one file in-process")
    parser.add_argument("--json", type=Path, help="write the rows as JSON")
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument("--timeout", type=float, default=600.0)
    arguments = parser.parse_args(argv)
    if arguments.probe is not None:
        print(json.dumps(probe(arguments.probe), ensure_ascii=False, default=str))
        return 0
    if not arguments.directories:
        parser.error("a corpus directory is required")
    rows = run_matrix(arguments.directories, arguments.python, arguments.timeout)
    print_table(rows)
    if arguments.json is not None:
        arguments.json.write_text(
            json.dumps(rows, ensure_ascii=False, indent=1, default=str) + "\n", encoding="utf-8"
        )
    return 0 if all(is_success(row) for row in rows) else 1


if __name__ == "__main__":
    os.environ.setdefault("PYTHONUNBUFFERED", "1")
    sys.exit(main())
