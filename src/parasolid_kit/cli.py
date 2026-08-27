"""Dependency-free command line interface for inspection and validation."""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from collections.abc import Sequence
from pathlib import Path
from typing import Literal, TextIO

from . import __version__
from .api import compare_documents, inspect_xb, inspect_xt, map_brep, parse_xb, parse_xt
from .binary.document import ParasolidDocument
from .brep import BrepModel
from .errors import ParasolidError
from .schema import InMemorySchemaProvider, SchemaKey, load_schema_catalog

SourceFormat = Literal["binary", "text"]


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="parasolid-kit",
        description="Inspect and validate Parasolid X_T/X_B files without format guessing.",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    commands = parser.add_subparsers(dest="command", required=True)

    inspect_command = commands.add_parser("inspect", help="inspect one header at L0")
    inspect_command.add_argument("path", type=Path)
    _add_format_argument(inspect_command)

    parse_command = commands.add_parser("parse", help="parse one complete document at L1/L4")
    parse_command.add_argument("path", type=Path)
    parse_command.add_argument("--schema-dir", type=Path, required=True)
    parse_command.add_argument(
        "--brep",
        action="store_true",
        help="also map and summarize the strict Parasolid-native B-Rep model",
    )
    _add_format_argument(parse_command)

    compare_command = commands.add_parser(
        "compare",
        help="compare two complete documents after pointer-index remapping",
    )
    compare_command.add_argument("left", type=Path)
    compare_command.add_argument("right", type=Path)
    compare_command.add_argument("--schema-dir", type=Path, required=True)
    compare_command.add_argument("--absolute-tolerance", type=float, default=1.0e-12)
    compare_command.add_argument("--relative-tolerance", type=float, default=1.0e-12)
    return parser


def _add_format_argument(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--format",
        choices=("auto", "x-b", "x-t"),
        default="auto",
        help="source encoding; auto accepts only known suffixes or signatures",
    )


def _source_format(path: Path, requested: str = "auto") -> SourceFormat:
    if requested == "x-b":
        return "binary"
    if requested == "x-t":
        return "text"
    suffix = path.suffix.lower()
    if suffix in {".x_b", ".xb"}:
        return "binary"
    if suffix in {".x_t", ".xt"}:
        return "text"
    with path.open("rb") as stream:
        prefix = stream.read(128)
    if prefix.startswith(b"PS\0\0") or b"\nPS\0\0" in prefix:
        return "binary"
    if prefix.startswith(b"T") or prefix.startswith(b"**PART"):
        return "text"
    raise ValueError("cannot identify source encoding; pass --format x-b or --format x-t")


def _header(path: Path, source_format: SourceFormat) -> object:
    return inspect_xb(path) if source_format == "binary" else inspect_xt(path)


def _provider(path: Path, source_format: SourceFormat, schema_dir: Path) -> InMemorySchemaProvider:
    header = _header(path, source_format)
    schema_key = SchemaKey.parse(header.schema_key)  # type: ignore[attr-defined]
    schema_path = schema_dir.expanduser().resolve() / f"sch_{schema_key.provider_schema}.sch_txt"
    if not schema_path.is_file():
        raise FileNotFoundError(f"required exact schema catalog does not exist: {schema_path}")
    catalog = load_schema_catalog(schema_path, expected_schema_id=schema_key.provider_schema)
    return InMemorySchemaProvider((catalog,))


def _document(
    path: Path,
    source_format: SourceFormat,
    schema_dir: Path,
) -> ParasolidDocument:
    provider = _provider(path, source_format, schema_dir)
    parser = parse_xb if source_format == "binary" else parse_xt
    return parser(path, schema_provider=provider)


def _document_summary(document: ParasolidDocument) -> dict[str, object]:
    type_counts = Counter(node.node_type for node in document.nodes)
    return {
        "format": document.format,
        "schema_key": document.schema_key.to_dict(),
        "node_count": len(document.nodes),
        "node_type_counts": {str(key): value for key, value in sorted(type_counts.items())},
        "schema_coverage": document.schema_coverage.to_dict(),
        "termination": document.terminator.to_dict(),
        "diagnostics": [item.to_dict() for item in document.diagnostics],
    }


def _brep_summary(model: BrepModel) -> dict[str, object]:
    serialized = model.to_dict()
    names = (
        "bodies",
        "regions",
        "shells",
        "faces",
        "loops",
        "half_edges",
        "edges",
        "vertices",
        "points",
        "curves",
        "surfaces",
    )
    return {
        "complete": model.complete,
        "counts": {name: len(getattr(model, name)) for name in names},
        "topology": serialized["topology"],
        "metrics": serialized["metrics"],
        "diagnostics": serialized["diagnostics"],
    }


def _write_json(value: object, *, stream: TextIO | None = None) -> None:
    print(
        json.dumps(value, ensure_ascii=False, indent=2),
        file=sys.stdout if stream is None else stream,
    )


def _run(arguments: argparse.Namespace) -> int:
    if arguments.command == "inspect":
        source_format = _source_format(arguments.path, arguments.format)
        header = _header(arguments.path, source_format)
        _write_json(
            {
                "status": "header_valid",
                "format": source_format,
                "path": str(arguments.path),
                "header": header.to_dict(),  # type: ignore[attr-defined]
            }
        )
        return 0
    if arguments.command == "parse":
        source_format = _source_format(arguments.path, arguments.format)
        document = _document(arguments.path, source_format, arguments.schema_dir)
        report: dict[str, object] = {
            "status": "parsed",
            "path": str(arguments.path),
            "document": _document_summary(document),
        }
        if arguments.brep:
            report["brep"] = _brep_summary(map_brep(document))
        _write_json(report)
        return 0
    if arguments.command == "compare":
        left_format = _source_format(arguments.left)
        right_format = _source_format(arguments.right)
        left = _document(arguments.left, left_format, arguments.schema_dir)
        right = _document(arguments.right, right_format, arguments.schema_dir)
        comparison = compare_documents(
            left,
            right,
            absolute_tolerance=arguments.absolute_tolerance,
            relative_tolerance=arguments.relative_tolerance,
        )
        _write_json(
            {
                "status": "equivalent" if comparison.equivalent else "different",
                "left": str(arguments.left),
                "right": str(arguments.right),
                "comparison": comparison.to_dict(),
            }
        )
        return 0 if comparison.equivalent else 1
    raise RuntimeError("unreachable CLI command")


def main(argv: Sequence[str] | None = None) -> int:
    """Run the CLI and return 0 for success, 1 for differences, or 2 for errors."""

    arguments = _parser().parse_args(argv)
    try:
        return _run(arguments)
    except (ParasolidError, OSError, TypeError, ValueError) as error:
        report: dict[str, object] = {
            "status": "error",
            "error_type": type(error).__name__,
            "message": str(error),
        }
        if isinstance(error, ParasolidError) and hasattr(error, "diagnostic"):
            report["diagnostic"] = error.diagnostic.to_dict()
        _write_json(report, stream=sys.stderr)
        return 2
