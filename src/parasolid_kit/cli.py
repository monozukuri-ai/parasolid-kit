"""Dependency-free command line interface for inspection and validation."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import Counter
from collections.abc import Sequence
from contextlib import suppress
from pathlib import Path
from typing import Literal, TextIO

from . import __version__
from .api import compare_documents, inspect_xb, inspect_xt, map_brep, parse_xb, parse_xt, read_brep
from .binary.document import ParasolidDocument
from .brep import BrepModel, Vector3
from .diagnostics import Diagnostic
from .errors import ParasolidError
from .schema import DirectorySchemaProvider, SchemaKey
from .summary import BrepSummary

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

    check_command = commands.add_parser(
        "check",
        help="parse, map, and show a compact B-Rep completeness report",
    )
    check_command.add_argument("path", type=Path)
    check_command.add_argument("--schema-dir", type=Path, required=True)
    check_command.add_argument(
        "--json",
        action="store_true",
        help="write the compact report as JSON instead of human-readable text",
    )
    _add_format_argument(check_command)

    compare_command = commands.add_parser(
        "compare",
        help="compare two complete documents after pointer-index remapping",
    )
    compare_command.add_argument("left", type=Path)
    compare_command.add_argument("right", type=Path)
    compare_command.add_argument("--schema-dir", type=Path, required=True)
    compare_command.add_argument("--absolute-tolerance", type=float, default=1.0e-12)
    compare_command.add_argument("--relative-tolerance", type=float, default=1.0e-12)

    export_step_command = commands.add_parser(
        "export-step",
        help="convert a complete B-Rep and export validated AP242 plus a JSON sidecar",
    )
    export_step_command.add_argument("path", type=Path)
    export_step_command.add_argument("output", type=Path)
    export_step_command.add_argument("--schema-dir", type=Path, required=True)
    export_step_command.add_argument(
        "--source-unit",
        choices=("m", "cm", "mm", "in", "ft"),
        required=True,
        help="physical length unit of Parasolid coordinates; never inferred",
    )
    export_step_command.add_argument(
        "--output-unit",
        choices=("m", "cm", "mm", "in", "ft"),
        default="mm",
        help="length unit declared by the AP242 file (default: mm)",
    )
    export_step_command.add_argument(
        "--overwrite",
        action="store_true",
        help="replace an existing STEP and conversion sidecar",
    )
    export_step_command.add_argument(
        "--no-validate",
        action="store_true",
        help="skip the default separate-process STEP reimport check",
    )
    _add_format_argument(export_step_command)

    view_command = commands.add_parser(
        "view",
        help="write a bounded GLB preview and serve its bundled UI on localhost",
    )
    view_command.add_argument("path", type=Path)
    view_command.add_argument("--schema-dir", type=Path, required=True)
    view_command.add_argument(
        "--source-unit",
        choices=("m", "cm", "mm", "in", "ft"),
        required=True,
        help="physical length unit of Parasolid coordinates; never inferred",
    )
    view_command.add_argument(
        "--target-unit",
        choices=("m", "cm", "mm", "in", "ft"),
        default="mm",
        help="coordinate unit used by the preview GLB (default: mm)",
    )
    view_command.add_argument(
        "--output",
        type=Path,
        help="self-contained output directory (default: <input-stem>.parasolid-preview)",
    )
    view_command.add_argument(
        "--linear-deflection",
        type=float,
        default=0.1,
        help="OCCT mesh deflection in target units (default: 0.1)",
    )
    view_command.add_argument(
        "--angular-deflection",
        type=float,
        default=0.5,
        help="OCCT mesh angular deflection in radians (default: 0.5)",
    )
    view_command.add_argument(
        "--no-edges",
        action="store_true",
        help="omit edge line primitives and edge picking",
    )
    view_command.add_argument(
        "--allow-partial",
        action="store_true",
        help="write an explicitly warned preview when source entities are missing",
    )
    view_command.add_argument(
        "--overwrite",
        action="store_true",
        help="replace an existing preview directory",
    )
    view_command.add_argument(
        "--host",
        default="127.0.0.1",
        help="HTTP bind address (default: 127.0.0.1)",
    )
    view_command.add_argument(
        "--port",
        type=int,
        default=0,
        help="HTTP port; zero selects an ephemeral port (default: 0)",
    )
    view_command.add_argument(
        "--allow-external",
        action="store_true",
        help="explicitly permit a non-loopback --host",
    )
    view_command.add_argument(
        "--no-open",
        action="store_true",
        help="do not launch a browser automatically",
    )
    view_command.add_argument(
        "--write-only",
        action="store_true",
        help="write GLB, manifest, and UI without starting the HTTP server",
    )
    _add_format_argument(view_command)
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


def _provider(
    path: Path,
    source_format: SourceFormat,
    schema_dir: Path,
) -> DirectorySchemaProvider:
    header = _header(path, source_format)
    schema_key = SchemaKey.parse(header.schema_key)  # type: ignore[attr-defined]
    provider = DirectorySchemaProvider(schema_dir)
    if provider.get_schema(schema_key.provider_schema) is None:
        raise FileNotFoundError(
            "required exact schema catalog does not exist: "
            f"{provider.catalog_path(schema_key.provider_schema)}"
        )
    return provider


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


def _write_check_summary(
    path: Path,
    summary: BrepSummary,
    *,
    stream: TextIO | None = None,
) -> None:
    output = sys.stdout if stream is None else stream
    status = "complete" if summary.complete and summary.topology.valid else "incomplete"
    print(f"Parasolid B-Rep check: {status}", file=output)
    print(f"  Path: {path}", file=output)
    print(f"  Format: {'X_B' if summary.source_format == 'binary' else 'X_T'}", file=output)
    print(f"  Schema: {summary.schema_key.raw}", file=output)
    print(f"  Provider schema: {summary.schema_key.provider_schema}", file=output)
    print(f"  Modeller: {summary.modeller_version}", file=output)
    print(f"  File size: {summary.file_size} bytes", file=output)
    print(f"  Nodes: {summary.node_count}", file=output)
    print(
        "  Resolved schema: "
        f"{summary.resolved_schema_type_count} types, "
        f"{summary.resolved_schema_field_count} fields",
        file=output,
    )
    print(f"  Complete: {'yes' if summary.complete else 'no'}", file=output)
    print(f"  Topology valid: {'yes' if summary.topology.valid else 'no'}", file=output)

    print("Counts:", file=output)
    for name, count in summary.counts.to_dict().items():
        print(f"  {name}: {count}", file=output)

    print("Kinds:", file=output)
    print(f"  bodies: {_format_kind_counts(summary.body_kind_counts)}", file=output)
    print(f"  curves: {_format_kind_counts(summary.curve_kind_counts)}", file=output)
    print(f"  surfaces: {_format_kind_counts(summary.surface_kind_counts)}", file=output)

    print("Metrics (source transmit units; physical length unit unknown):", file=output)
    bounding_box = summary.metrics.bounding_box
    if bounding_box is None:
        print("  bounding_box: unavailable", file=output)
    else:
        print(
            "  bounding_box: "
            f"min={_format_vector(bounding_box.minimum)} "
            f"max={_format_vector(bounding_box.maximum)}",
            file=output,
        )
        print(f"  extents: {_format_vector(bounding_box.extents)}", file=output)
    print(f"  surface_area: {_format_metric(summary.metrics.surface_area)}", file=output)
    print(f"  volume: {_format_metric(summary.metrics.volume)}", file=output)

    print(f"Diagnostics ({summary.diagnostic_count}):", file=output)
    if summary.diagnostic_count == 0:
        print("  none", file=output)
    else:
        for layer, diagnostics in (
            ("document", summary.document_diagnostics),
            ("brep", summary.brep_diagnostics),
        ):
            for diagnostic in diagnostics:
                print(
                    f"  [{layer}] {diagnostic.severity.value} "
                    f"{diagnostic.code} ({_format_diagnostic_entity(diagnostic)}): "
                    f"{diagnostic.message}",
                    file=output,
                )


def _format_kind_counts(counts: tuple[tuple[str, int], ...]) -> str:
    return "none" if not counts else ", ".join(f"{kind}={count}" for kind, count in counts)


def _format_diagnostic_entity(diagnostic: Diagnostic) -> str:
    coordinates: list[str] = []
    if diagnostic.node_id is not None:
        coordinates.append(f"node_id={diagnostic.node_id}")
    if diagnostic.node_type is not None:
        coordinates.append(f"node_type={diagnostic.node_type}")
    return ", ".join(coordinates) if coordinates else "entity=unknown"


def _format_vector(vector: Vector3) -> str:
    return "(" + ", ".join(_format_number(value) for value in vector) + ")"


def _format_metric(value: float | None) -> str:
    return "unavailable" if value is None else _format_number(value)


def _format_number(value: float) -> str:
    return format(value, ".12g")


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
    if arguments.command == "check":
        parsed = read_brep(
            arguments.path,
            schema_dir=arguments.schema_dir,
            source_format=arguments.format,
        )
        complete = parsed.summary.complete and parsed.summary.topology.valid
        if arguments.json:
            _write_json(
                {
                    "status": "complete" if complete else "incomplete",
                    "path": str(arguments.path),
                    "summary": parsed.summary.to_dict(),
                }
            )
        else:
            _write_check_summary(arguments.path, parsed.summary)
        return 0 if complete else 1
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
    if arguments.command == "export-step":
        from .interop.occt import to_occt
        from .interop.occt.step import write_step

        parsed = read_brep(
            arguments.path,
            schema_dir=arguments.schema_dir,
            source_format=arguments.format,
        )
        source_sha256 = hashlib.sha256(parsed.document.raw_bytes).hexdigest()
        converted = to_occt(
            parsed.brep,
            source_unit=arguments.source_unit,
            target_unit="mm",
            source_identity=f"sha256:{source_sha256}",
        )
        exported = write_step(
            converted,
            arguments.output,
            output_unit=arguments.output_unit,
            validate=not arguments.no_validate,
            overwrite=arguments.overwrite,
        )
        step_report = exported.report
        _write_json(
            {
                "status": step_report.status,
                "source": str(arguments.path),
                "source_sha256": source_sha256,
                "output": str(exported.path),
                "sidecar": str(exported.sidecar_path),
                "step": {
                    "schema": step_report.step_schema,
                    "output_unit": step_report.output_unit,
                    "conversion_target_unit": converted.report.options.target_unit,
                    "exchange_working_unit": "mm",
                    "geometry_scale_to_mm": step_report.geometry_scale_to_mm,
                    "transfer_status": step_report.transfer_status,
                    "write_status": step_report.write_status,
                },
                "artifact": step_report.artifact.to_dict(),
                "conversion": {
                    "source_complete": converted.report.source_complete,
                    "conversion_complete": converted.report.conversion_complete,
                    "occt_valid": converted.report.occt_valid,
                    "output_topology": converted.report.output_topology.to_dict(),
                    "metrics": converted.report.metrics.to_dict(),
                },
                "validation": (
                    None if step_report.validation is None else step_report.validation.to_dict()
                ),
            }
        )
        return 0
    if arguments.command == "view":
        from .interop.occt import to_occt
        from .interop.preview import (
            PreviewOptions,
            create_preview_server,
            write_preview,
        )

        parsed = read_brep(
            arguments.path,
            schema_dir=arguments.schema_dir,
            source_format=arguments.format,
        )
        source_sha256 = hashlib.sha256(parsed.document.raw_bytes).hexdigest()
        converted = to_occt(
            parsed.brep,
            source_unit=arguments.source_unit,
            target_unit=arguments.target_unit,
            require_complete=not arguments.allow_partial,
            source_identity=f"sha256:{source_sha256}",
        )
        output = arguments.output or arguments.path.with_name(
            f"{arguments.path.stem}.parasolid-preview"
        )
        preview = write_preview(
            converted,
            parsed.brep,
            output,
            options=PreviewOptions(
                linear_deflection=arguments.linear_deflection,
                angular_deflection=arguments.angular_deflection,
                include_edges=not arguments.no_edges,
                allow_partial=arguments.allow_partial,
            ),
            overwrite=arguments.overwrite,
        )
        response: dict[str, object] = {
            "status": "generated" if arguments.write_only else "serving",
            "source_sha256": source_sha256,
            "output": str(preview.directory),
            "index": str(preview.index_path),
            "glb": str(preview.glb_path),
            "manifest": str(preview.manifest_path),
            "preview": preview.report.to_dict(),
        }
        if arguments.write_only:
            _write_json(response)
            return 0
        with create_preview_server(
            preview.directory,
            host=arguments.host,
            port=arguments.port,
            allow_external=arguments.allow_external,
        ) as server:
            response["url"] = server.url
            _write_json(response)
            sys.stdout.flush()
            if not arguments.no_open:
                server.open_browser()
            with suppress(KeyboardInterrupt):
                server.serve_forever()
        return 0
    raise RuntimeError("unreachable CLI command")


def main(argv: Sequence[str] | None = None) -> int:
    """Run the CLI and return 0 for success, 1 for incomplete/different, or 2 for errors."""

    arguments = _parser().parse_args(argv)
    try:
        return _run(arguments)
    except (ParasolidError, OSError, TypeError, ValueError) as error:
        if arguments.command == "check" and not arguments.json:
            if isinstance(error, ParasolidError) and hasattr(error, "diagnostic"):
                print(f"error [{error.diagnostic.code}]: {error}", file=sys.stderr)
            else:
                print(f"error: {error}", file=sys.stderr)
            return 2
        report: dict[str, object] = {
            "status": "error",
            "error_type": type(error).__name__,
            "message": str(error),
        }
        if isinstance(error, ParasolidError) and hasattr(error, "diagnostic"):
            report["diagnostic"] = error.diagnostic.to_dict()
        error_report = getattr(error, "report", None)
        if error_report is not None and hasattr(error_report, "to_dict"):
            report["report"] = error_report.to_dict()
        _write_json(report, stream=sys.stderr)
        return 2
