"""Isolated API/CLI worker for verify_release_corpus; no optional CAD imports."""

from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import sys
from pathlib import Path


def runtime_guard(event: str, args: tuple[object, ...]) -> None:
    if event.startswith("socket."):
        raise RuntimeError("release corpus runtime prohibits network access")
    if (
        event == "open"
        and isinstance(args[0], (str, bytes))
        and os.fsdecode(args[0]).lower().endswith(".sch_txt")
    ):
        raise RuntimeError("release corpus runtime prohibits catalog access")


def require(condition: object, message: str) -> None:
    if not condition:
        raise ValueError(message)


def cli(*args: str) -> tuple[int, dict[str, object]]:
    from parasolid_kit.cli import main

    stdout, stderr = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        exit_code = main(args)
    return exit_code, json.loads(stderr.getvalue() if exit_code == 2 else stdout.getvalue())


def probe(path: Path, encoding: str, stage: str, pair: Path | None = None) -> dict[str, object]:
    from parasolid_kit import (
        ParasolidError,
        compare_documents,
        inspect_xb,
        inspect_xt,
        map_brep,
        parse_xb,
        parse_xt,
    )
    from parasolid_kit.summary import BrepSummary

    data = path.read_bytes()
    parser = parse_xt if encoding == "text" else parse_xb
    try:
        header = (inspect_xt if encoding == "text" else inspect_xb)(data)
        document = parser(data)
    except ParasolidError as error:
        code = error.diagnostic.code
        location = error.diagnostic.location
        offset = None if location is None else location.byte_offset
        exits = {}
        for args in (("parse", str(path)), ("check", str(path), "--json")):
            exit_code, result = cli(*args)
            require(exit_code == 2, "CLI accepted an API parse failure")
            require(result["diagnostic"] == error.diagnostic.to_dict(), "CLI diagnostic differs")
            exits[args[0]] = exit_code
        return {"status": "diagnostic", "code": code, "offset": offset, "cli_exits": exits}

    require(document.terminator.byte_range.end == len(data), "input was not fully consumed")
    inspect_exit, inspected = cli("inspect", str(path))
    require(inspect_exit == 0 and inspected["header"] == header.to_dict(), "CLI inspect differs")
    nodes = []
    for node in document.nodes:
        fields = []
        require(0 <= node.byte_range.start < node.byte_range.end <= len(data), "invalid node range")
        for field in node.fields:
            require(
                node.byte_range.start
                <= field.byte_range.start
                <= field.byte_range.end
                <= node.byte_range.end,
                "field range lies outside its node",
            )
            fields.append(
                {
                    "name": field.definition.name,
                    "code": field.definition.field_type.value,
                    "range": [field.byte_range.start, field.byte_range.end],
                    "values": [value.value for value in field.values],
                }
            )
        nodes.append(
            {
                "node_type": node.node_type,
                "index": node.index,
                "variable_length": node.variable_length,
                "range": [node.byte_range.start, node.byte_range.end],
                "fields": fields,
            }
        )
    model = map_brep(document) if stage == "brep" else None
    parse_args = ("parse", str(path), "--brep") if model else ("parse", str(path))
    parse_exit, parsed = cli(*parse_args)
    require(parse_exit == 0, "CLI parse failed")
    profile = None if document.schema_resolution is None else document.schema_resolution.to_dict()
    for key, expected in {
        "format": encoding,
        "schema_key": document.schema_key.to_dict(),
        "node_count": len(nodes),
        "schema_resolution": profile,
        "termination": document.terminator.to_dict(),
        "schema_coverage": document.schema_coverage.to_dict(),
        "diagnostics": [d.to_dict() for d in document.diagnostics],
    }.items():
        require(parsed["document"][key] == expected, f"CLI document {key} differs")
    exits = {"inspect": inspect_exit, "parse": parse_exit}
    brep = None
    if model is not None:
        summary = BrepSummary.from_parsed(document, model)
        check_exit, checked = cli("check", str(path), "--json")
        require(
            check_exit == (0 if model.complete and model.topology.valid else 1),
            "CLI check exit differs from completeness",
        )
        require(checked["summary"] == summary.to_dict(), "CLI check summary differs")
        brep = {
            "complete": model.complete,
            "counts": summary.counts.to_dict(),
            "topology": model.to_dict()["topology"],
            "diagnostic_codes": [d.code for d in model.diagnostics],
        }
        for key in ("complete", "counts", "topology"):
            require(parsed["brep"][key] == brep[key], f"CLI B-Rep {key} differs")
        exits["check"] = check_exit
    if pair is not None:
        other = (parse_xb if encoding == "text" else parse_xt)(pair.read_bytes())
        require(compare_documents(document, other).equivalent, "X_T / X_B values differ")
        compare_exit, compared = cli("compare", str(path), str(pair))
        require(compare_exit == 0 and compared["comparison"]["equivalent"], "CLI pair differs")
        exits["compare"] = compare_exit
    require("OCP" not in sys.modules and "cadquery" not in sys.modules, "optional CAD import")
    return {
        "status": "parsed",
        "schema_key": document.schema_key.raw,
        "profile": profile,
        "nodes": nodes,
        "brep": brep,
        "termination": [document.terminator.byte_range.start, document.terminator.byte_range.end],
        "snapshot": {"document": document.to_dict(), "brep": model.to_dict() if model else None},
        "cli_exits": exits,
        "pair_equivalent": True if pair else None,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    parser.add_argument("encoding", choices=("text", "binary"))
    parser.add_argument("stage", choices=("raw", "brep"))
    parser.add_argument("--pair", type=Path)
    args = parser.parse_args()
    sys.addaudithook(runtime_guard)
    try:
        from parasolid_kit import _core

        result = probe(args.path, args.encoding, args.stage, args.pair)
        result["runtime"] = {
            "python": sys.version,
            "executable": sys.executable,
            "core_path": _core.__file__,
            "core_version": _core.CORE_VERSION,
        }
        print(json.dumps(result, allow_nan=False))
        return 0 if result["status"] == "parsed" else 1
    except (OSError, RuntimeError, TypeError, ValueError) as error:
        print(json.dumps({"status": "failed", "error": str(error)}))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
