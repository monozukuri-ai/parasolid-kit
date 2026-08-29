"""Direct, bounded, and independently validated STEP export from OCCT results."""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
from contextlib import suppress
from dataclasses import dataclass
from math import isclose, isfinite
from pathlib import Path
from typing import Final, Literal, TypeAlias, cast

from ...diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity
from ..dependency import require_occt
from ..errors import InteropDependencyError, StepExportError
from ..limits import InteropLimits
from .model import ConversionReport, NamedCounts, OcctConversionResult, OcctMetrics, SourceShapeMap
from .options import UNIT_TO_METRES, LengthUnit

StepExportStatus: TypeAlias = Literal[
    "validated",
    "validation_failed",
    "written_unvalidated",
]
MetricValue: TypeAlias = float | tuple[float, ...]

_STEP_UNIT_NAMES: Final[dict[LengthUnit, str]] = {
    "m": "M",
    "cm": "CM",
    "mm": "MM",
    "in": "INCH",
    "ft": "FT",
}
_STEP_READER_UNIT_NAMES: Final[dict[LengthUnit, str]] = {
    "m": "metre",
    "cm": "centimetre",
    "mm": "millimetre",
    "in": "INCH",
    "ft": "FOOT",
}
_STEP_SUFFIXES: Final = frozenset({".step", ".stp"})
_STEP_SCHEMA_PARAMETER: Final = "AP242DIS"
_STEP_SCHEMA_NAME: Final = "AP242"
_REPRODUCIBLE_TIMESTAMP: Final = "1970-01-01T00:00:00"
_SOURCE_HASH = re.compile(r"^sha256:([0-9a-f]{64})$")
_OCCT_WRITE_LOCK = threading.Lock()


@dataclass(frozen=True, slots=True)
class StepArtifact:
    """Content identity for the committed STEP file without an absolute path."""

    filename: str
    sidecar_filename: str
    byte_size: int
    sha256: str

    def __post_init__(self) -> None:
        if not self.filename or Path(self.filename).name != self.filename:
            raise ValueError("filename must be one path component")
        if not self.sidecar_filename or Path(self.sidecar_filename).name != self.sidecar_filename:
            raise ValueError("sidecar_filename must be one path component")
        if isinstance(self.byte_size, bool) or not isinstance(self.byte_size, int):
            raise TypeError("byte_size must be an integer")
        if self.byte_size <= 0:
            raise ValueError("byte_size must be positive")
        if re.fullmatch(r"[0-9a-f]{64}", self.sha256) is None:
            raise ValueError("sha256 must be a lowercase SHA-256 digest")

    def to_dict(self) -> dict[str, object]:
        return {
            "filename": self.filename,
            "sidecar_filename": self.sidecar_filename,
            "byte_size": self.byte_size,
            "sha256": self.sha256,
        }


@dataclass(frozen=True, slots=True)
class StepMetricComparison:
    """One unit-normalized metric comparison made after cold STEP reimport."""

    metric: str
    expected: MetricValue
    actual: MetricValue
    tolerance: float
    maximum_absolute_difference: float
    within_tolerance: bool

    def __post_init__(self) -> None:
        if not self.metric:
            raise ValueError("metric must be non-empty")
        for name in ("tolerance", "maximum_absolute_difference"):
            value = getattr(self, name)
            if not isinstance(value, float) or not isfinite(value) or value < 0.0:
                raise ValueError(f"{name} must be a finite non-negative float")
        if not isinstance(self.within_tolerance, bool):
            raise TypeError("within_tolerance must be a boolean")

    def to_dict(self) -> dict[str, object]:
        return {
            "metric": self.metric,
            "expected": _json_metric(self.expected),
            "actual": _json_metric(self.actual),
            "tolerance": self.tolerance,
            "maximum_absolute_difference": self.maximum_absolute_difference,
            "within_tolerance": self.within_tolerance,
        }


@dataclass(frozen=True, slots=True)
class StepReimportReport:
    """Evidence returned by a separate Python/OCCT reader process."""

    schema_version: int
    process_isolated: bool
    reader_status: str
    reader_done: bool
    candidate_roots: int
    transferred_roots: int
    shape_count: int
    nonempty: bool
    occt_valid: bool
    ocp_version: str | None
    expected_length_unit: LengthUnit
    declared_length_units: tuple[str, ...]
    expected_topology: NamedCounts
    actual_topology: NamedCounts
    metrics_mm: OcctMetrics
    metric_comparisons: tuple[StepMetricComparison, ...]
    passed: bool

    def __post_init__(self) -> None:
        if self.schema_version != 1:
            raise ValueError("STEP reimport schema_version must be 1")
        if not self.process_isolated:
            raise ValueError("STEP reimport must run in an isolated process")
        if not self.reader_status:
            raise ValueError("reader_status must be non-empty")
        for name in ("candidate_roots", "transferred_roots", "shape_count"):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ValueError(f"{name} must be a non-negative integer")
        for name in ("reader_done", "nonempty", "occt_valid", "passed"):
            if not isinstance(getattr(self, name), bool):
                raise TypeError(f"{name} must be a boolean")
        if self.expected_length_unit not in UNIT_TO_METRES:
            raise ValueError("invalid expected STEP length unit")
        if any(not item for item in self.declared_length_units):
            raise ValueError("declared STEP length units must be non-empty")
        expected_pass = (
            self.reader_done
            and self.transferred_roots > 0
            and self.shape_count > 0
            and self.nonempty
            and self.occt_valid
            and self.unit_matches
            and self.topology_matches
            and self.metrics_match
        )
        if self.passed != expected_pass:
            raise ValueError("STEP reimport passed flag disagrees with its checks")

    @property
    def topology_matches(self) -> bool:
        return self.expected_topology == self.actual_topology

    @property
    def metrics_match(self) -> bool:
        return bool(self.metric_comparisons) and all(
            item.within_tolerance for item in self.metric_comparisons
        )

    @property
    def unit_matches(self) -> bool:
        return self.declared_length_units == (_STEP_READER_UNIT_NAMES[self.expected_length_unit],)

    def to_dict(self) -> dict[str, object]:
        return {
            "schema_version": self.schema_version,
            "process_isolated": self.process_isolated,
            "reader": {
                "status": self.reader_status,
                "done": self.reader_done,
                "candidate_roots": self.candidate_roots,
                "transferred_roots": self.transferred_roots,
                "shape_count": self.shape_count,
            },
            "nonempty": self.nonempty,
            "occt_valid": self.occt_valid,
            "ocp_version": self.ocp_version,
            "expected_length_unit": self.expected_length_unit,
            "declared_length_units": list(self.declared_length_units),
            "unit_matches": self.unit_matches,
            "expected_topology": self.expected_topology.to_dict(),
            "actual_topology": self.actual_topology.to_dict(),
            "topology_matches": self.topology_matches,
            "metrics_unit": "mm",
            "metrics": self.metrics_mm.to_dict(),
            "metric_comparisons": [item.to_dict() for item in self.metric_comparisons],
            "metrics_match": self.metrics_match,
            "passed": self.passed,
        }


@dataclass(frozen=True, slots=True)
class StepExportReport:
    """Versioned sidecar model for one direct STEP export."""

    schema_version: int
    producer: str
    status: StepExportStatus
    step_schema: str
    output_unit: LengthUnit
    geometry_scale_to_mm: float
    overwrite_requested: bool
    validation_requested: bool
    source_identity_included: bool
    artifact: StepArtifact
    conversion: ConversionReport
    source_map: SourceShapeMap
    export_limits: InteropLimits
    transfer_status: str
    write_status: str
    validation: StepReimportReport | None

    def __post_init__(self) -> None:
        if self.schema_version != 1:
            raise ValueError("STEP export schema_version must be 1")
        if self.status not in ("validated", "validation_failed", "written_unvalidated"):
            raise ValueError("invalid STEP export status")
        if self.output_unit not in UNIT_TO_METRES:
            raise ValueError("invalid STEP output unit")
        if not isinstance(self.geometry_scale_to_mm, float) or not isfinite(
            self.geometry_scale_to_mm
        ):
            raise ValueError("geometry_scale_to_mm must be finite")
        if self.geometry_scale_to_mm <= 0.0:
            raise ValueError("geometry_scale_to_mm must be positive")
        for name in (
            "overwrite_requested",
            "validation_requested",
            "source_identity_included",
        ):
            if not isinstance(getattr(self, name), bool):
                raise TypeError(f"{name} must be a boolean")
        if self.validation_requested != (self.validation is not None):
            raise ValueError("validation report presence must match validation_requested")
        if self.status == "validated" and (self.validation is None or not self.validation.passed):
            raise ValueError("validated STEP report requires passing reimport evidence")
        if self.status == "validation_failed" and (
            self.validation is None or self.validation.passed
        ):
            raise ValueError("failed STEP report requires failing reimport evidence")
        if self.status == "written_unvalidated" and self.validation is not None:
            raise ValueError("unvalidated STEP report must not contain reimport evidence")

    def to_dict(self) -> dict[str, object]:
        conversion = self.conversion.to_dict()
        source_identity = self.conversion.source_identity
        source_sha256 = _source_sha256(source_identity)
        if not self.source_identity_included and source_sha256 is None:
            conversion["source_identity"] = None
        relation_counts = {name: 0 for name in ("direct", "split", "merged", "generated")}
        for relation in self.source_map.relations:
            relation_counts[relation.relation.value] += 1
        return {
            "schema_version": self.schema_version,
            "producer": self.producer,
            "status": self.status,
            "source": {
                "format": self.conversion.source_format,
                "format_name": "X_B" if self.conversion.source_format == "binary" else "X_T",
                "schema_key": self.conversion.schema_key,
                "sha256": source_sha256,
                "identity_included": self.source_identity_included,
                "parse_status": "complete" if self.conversion.source_complete else "incomplete",
            },
            "step": {
                "schema": self.step_schema,
                "output_unit": self.output_unit,
                "conversion_target_unit": self.conversion.options.target_unit,
                "exchange_working_unit": "mm",
                "geometry_scale_to_mm": self.geometry_scale_to_mm,
                "transfer_status": self.transfer_status,
                "write_status": self.write_status,
                "timestamp_policy": "reproducible_fixed",
                "timestamp": _REPRODUCIBLE_TIMESTAMP,
            },
            "artifact": self.artifact.to_dict(),
            "atomic_output": True,
            "overwrite_requested": self.overwrite_requested,
            "validation_requested": self.validation_requested,
            "conversion": conversion,
            "conversion_effects": {
                "unsupported_diagnostic_count": sum(
                    item.kind is DiagnosticKind.UNSUPPORTED for item in self.conversion.diagnostics
                ),
                "source_map_relation_counts": relation_counts,
                "topology_operations": list(self.conversion.topology_operations),
                "healing_requested": self.conversion.healing_requested,
                "healing_performed": self.conversion.healing_performed,
                "healing_operations": list(self.conversion.healing_operations),
            },
            "source_map": self.source_map.to_dict(),
            "export_limits": self.export_limits.to_dict(),
            "validation": None if self.validation is None else self.validation.to_dict(),
        }


@dataclass(frozen=True, slots=True)
class StepExportResult:
    """Committed STEP/sidecar paths and their versioned report."""

    path: Path
    sidecar_path: Path
    report: StepExportReport

    def to_dict(self) -> dict[str, object]:
        return {
            "path": str(self.path),
            "sidecar_path": str(self.sidecar_path),
            "report": self.report.to_dict(),
        }


def write_step(
    result: OcctConversionResult,
    destination: str | os.PathLike[str],
    *,
    output_unit: LengthUnit = "mm",
    validate: bool = True,
    overwrite: bool = False,
    include_source_identity: bool = False,
    limits: InteropLimits | None = None,
) -> StepExportResult:
    """Write AP242 directly from a complete OCCT result and optionally cold-reimport it."""

    if not isinstance(result, OcctConversionResult):
        raise TypeError("result must be OcctConversionResult")
    if output_unit not in UNIT_TO_METRES:
        supported = ", ".join(UNIT_TO_METRES)
        raise ValueError(f"output_unit must be one of: {supported}")
    output_unit = cast(LengthUnit, output_unit)
    for name, value in (
        ("validate", validate),
        ("overwrite", overwrite),
        ("include_source_identity", include_source_identity),
    ):
        if not isinstance(value, bool):
            raise TypeError(f"{name} must be a boolean")
    effective_limits = result.report.limits if limits is None else limits
    if not isinstance(effective_limits, InteropLimits):
        raise TypeError("limits must be InteropLimits or None")
    _require_exportable(result)
    path, sidecar_path = _validate_paths(destination, overwrite=overwrite)

    target_unit = result.report.options.target_unit
    geometry_scale_to_mm = UNIT_TO_METRES[target_unit] / UNIT_TO_METRES["mm"]
    temporary_step = _temporary_path(path)
    temporary_sidecar: Path | None = None
    try:
        transfer_status, write_status = _write_occt_step(
            result.shape,
            temporary_step,
            output_unit=output_unit,
            geometry_scale_to_mm=geometry_scale_to_mm,
        )
        step_size = temporary_step.stat().st_size
        if step_size <= 0:
            raise _step_error(
                "step.write_failed",
                DiagnosticKind.INVALID,
                "OCCT reported success but produced an empty STEP file",
            )
        if step_size > effective_limits.max_output_bytes:
            raise _output_limit_error(step_size, effective_limits.max_output_bytes)
        artifact = StepArtifact(
            filename=path.name,
            sidecar_filename=sidecar_path.name,
            byte_size=step_size,
            sha256=_file_sha256(temporary_step),
        )

        validation_report = None
        if validate:
            validation_report = _validate_cold_reimport(
                temporary_step,
                result,
                geometry_scale_to_mm=geometry_scale_to_mm,
                output_unit=output_unit,
            )
        status: StepExportStatus
        if validation_report is None:
            status = "written_unvalidated"
        elif validation_report.passed:
            status = "validated"
        else:
            status = "validation_failed"
        report = StepExportReport(
            schema_version=1,
            producer="parasolid-kit.interop.occt.step",
            status=status,
            step_schema=_STEP_SCHEMA_NAME,
            output_unit=output_unit,
            geometry_scale_to_mm=float(geometry_scale_to_mm),
            overwrite_requested=overwrite,
            validation_requested=validate,
            source_identity_included=include_source_identity,
            artifact=artifact,
            conversion=result.report,
            source_map=result.source_map,
            export_limits=effective_limits,
            transfer_status=transfer_status,
            write_status=write_status,
            validation=validation_report,
        )
        if validation_report is not None and not validation_report.passed:
            code = (
                "step.reimport_failed"
                if not validation_report.reader_done
                else "step.reimport_mismatch"
            )
            message = (
                "the independent STEP reader could not read the staged artifact"
                if not validation_report.reader_done
                else "the independently reimported STEP shape differs from the OCCT result"
            )
            raise _step_error(
                code,
                DiagnosticKind.INVALID,
                message,
                report=report,
                details={
                    "reader_status": validation_report.reader_status,
                    "nonempty": validation_report.nonempty,
                    "occt_valid": validation_report.occt_valid,
                    "unit_matches": validation_report.unit_matches,
                    "topology_matches": validation_report.topology_matches,
                    "metrics_match": validation_report.metrics_match,
                },
            )

        temporary_sidecar = _temporary_path(sidecar_path)
        _write_staged_sidecar(
            temporary_sidecar,
            report,
            maximum_bytes=effective_limits.max_output_bytes - step_size,
            staged_step_bytes=step_size,
            total_limit=effective_limits.max_output_bytes,
        )
        _commit_outputs(
            temporary_step,
            path,
            temporary_sidecar,
            sidecar_path,
            overwrite=overwrite,
        )
        temporary_step = None
        temporary_sidecar = None
        return StepExportResult(path=path, sidecar_path=sidecar_path, report=report)
    except (InteropDependencyError, StepExportError):
        raise
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception as error:
        export_error = _step_error(
            "step.write_failed",
            DiagnosticKind.INVALID,
            "STEP export failed before a validated artifact could be committed",
            details={"exception_type": type(error).__name__},
        )
        raise export_error from error
    finally:
        for temporary in (temporary_step, temporary_sidecar):
            if temporary is not None:
                _unlink_temporary(temporary)


def _require_exportable(result: OcctConversionResult) -> None:
    report = result.report
    if report.source_complete and report.conversion_complete and report.occt_valid:
        return
    raise _step_error(
        "step.incomplete_conversion",
        DiagnosticKind.INCOMPLETE,
        "STEP export requires a complete source and a complete, valid OCCT conversion",
        details={
            "source_complete": report.source_complete,
            "conversion_complete": report.conversion_complete,
            "occt_valid": report.occt_valid,
        },
    )


def _validate_paths(
    destination: str | os.PathLike[str],
    *,
    overwrite: bool,
) -> tuple[Path, Path]:
    try:
        path = Path(destination)
    except TypeError as error:
        raise TypeError("destination must be a string or path-like object") from error
    if path.suffix.lower() not in _STEP_SUFFIXES:
        raise _step_error(
            "step.invalid_path",
            DiagnosticKind.INVALID,
            "STEP destination must use a .step or .stp suffix",
        )
    parent = path.parent
    if not parent.is_dir():
        raise _step_error(
            "step.invalid_path",
            DiagnosticKind.INVALID,
            "STEP destination parent directory does not exist",
        )
    sidecar_path = Path(f"{path}.conversion.json")
    for candidate, kind in ((path, "STEP"), (sidecar_path, "sidecar")):
        if candidate.is_symlink():
            raise _step_error(
                "step.invalid_path",
                DiagnosticKind.INVALID,
                f"{kind} destination must not be a symbolic link",
            )
        if candidate.exists() and not candidate.is_file():
            raise _step_error(
                "step.invalid_path",
                DiagnosticKind.INVALID,
                f"{kind} destination exists and is not a regular file",
            )
        if candidate.exists() and not overwrite:
            raise _step_error(
                "step.output_exists",
                DiagnosticKind.INVALID,
                f"{kind} destination already exists; pass overwrite=True to replace it",
            )
    return path, sidecar_path


def _temporary_path(destination: Path) -> Path:
    descriptor, raw_path = tempfile.mkstemp(
        prefix=f".{destination.name}.",
        suffix=".tmp",
        dir=destination.parent,
    )
    os.close(descriptor)
    return Path(raw_path)


def _write_occt_step(
    shape: object,
    path: Path,
    *,
    output_unit: LengthUnit,
    geometry_scale_to_mm: float,
) -> tuple[str, str]:
    require_occt()
    from OCP.APIHeaderSection import APIHeaderSection_MakeHeader
    from OCP.BRepBuilderAPI import BRepBuilderAPI_Transform
    from OCP.gp import gp_Pnt, gp_Trsf
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.Interface import Interface_Static
    from OCP.Message import Message
    from OCP.StepBasic import StepBasic_Product
    from OCP.STEPControl import STEPControl_AsIs, STEPControl_Writer
    from OCP.TCollection import TCollection_HAsciiString

    export_shape = shape
    if not isclose(geometry_scale_to_mm, 1.0, rel_tol=0.0, abs_tol=0.0):
        transform = gp_Trsf()
        transform.SetScale(gp_Pnt(0.0, 0.0, 0.0), geometry_scale_to_mm)
        export_shape = BRepBuilderAPI_Transform(shape, transform, True).Shape()
    if export_shape.IsNull():
        raise _step_error(
            "step.write_failed",
            DiagnosticKind.INVALID,
            "the OCCT shape is null after exchange-unit normalization",
        )

    with _OCCT_WRITE_LOCK:
        messenger = Message.DefaultMessenger_s()
        sequence = messenger.Printers()
        printers = tuple(sequence.Value(index) for index in range(1, sequence.Size() + 1))
        messenger.ChangePrinters().Clear()
        prior_unit = ""
        prior_schema = ""
        try:
            writer = STEPControl_Writer()
            prior_unit = Interface_Static.CVal_s("write.step.unit")
            prior_schema = Interface_Static.CVal_s("write.step.schema")
            unit_name = _STEP_UNIT_NAMES[output_unit]
            if not Interface_Static.SetCVal_s("write.step.unit", unit_name):
                raise _step_error(
                    "step.write_failed",
                    DiagnosticKind.INVALID,
                    "the installed OCCT runtime rejected the requested STEP unit",
                    details={"output_unit": output_unit},
                )
            if not Interface_Static.SetCVal_s("write.step.schema", _STEP_SCHEMA_PARAMETER):
                raise _step_error(
                    "step.write_failed",
                    DiagnosticKind.INVALID,
                    "the installed OCCT runtime does not support AP242 STEP output",
                )
            transfer = writer.Transfer(export_shape, STEPControl_AsIs)
            transfer_name = _status_name(transfer)
            if transfer != IFSelect_RetDone:
                raise _step_error(
                    "step.write_failed",
                    DiagnosticKind.INVALID,
                    "OCCT could not transfer the complete shape into the AP242 model",
                    details={"transfer_status": transfer_name},
                )
            product_number = 0
            model = writer.Model()
            for entity_number in range(1, model.NbEntities() + 1):
                entity = model.Value(entity_number)
                if isinstance(entity, StepBasic_Product):
                    product_number += 1
                    product_name = TCollection_HAsciiString(
                        f"parasolid-kit-product-{product_number:06d}"
                    )
                    entity.SetId(product_name)
                    entity.SetName(product_name)
            header = APIHeaderSection_MakeHeader(model)
            header.SetTimeStamp(TCollection_HAsciiString(_REPRODUCIBLE_TIMESTAMP))
            written = writer.Write(str(path))
            write_name = _status_name(written)
            if written != IFSelect_RetDone:
                raise _step_error(
                    "step.write_failed",
                    DiagnosticKind.INVALID,
                    "OCCT could not write the staged AP242 file",
                    details={"write_status": write_name},
                )
            return transfer_name, write_name
        finally:
            if prior_unit:
                Interface_Static.SetCVal_s("write.step.unit", prior_unit)
            if prior_schema:
                Interface_Static.SetCVal_s("write.step.schema", prior_schema)
            for printer in printers:
                messenger.AddPrinter(printer)


def _validate_cold_reimport(
    path: Path,
    result: OcctConversionResult,
    *,
    geometry_scale_to_mm: float,
    output_unit: LengthUnit,
) -> StepReimportReport:
    payload = _run_cold_reimport(path)
    expected_counts = result.report.output_topology.to_dict()
    expected_bodies = _body_count(expected_counts)
    actual_counts = _integer_mapping(payload.get("topology"))
    actual_bodies = _body_count(actual_counts)
    expected_topology = NamedCounts.from_dict(
        {"bodies": expected_bodies, "faces": expected_counts.get("faces", 0)}
    )
    actual_topology = NamedCounts.from_dict(
        {"bodies": actual_bodies, "faces": actual_counts.get("faces", 0)}
    )
    metrics = _metrics_from_payload(payload.get("metrics"))
    comparisons = _metric_comparisons(
        result.report,
        metrics,
        geometry_scale_to_mm=geometry_scale_to_mm,
    )
    reader_done = payload.get("reader_done") is True
    nonempty = payload.get("nonempty") is True
    occt_valid = payload.get("occt_valid") is True
    topology_matches = expected_topology == actual_topology
    declared_length_units = _string_tuple(payload.get("declared_length_units"))
    unit_matches = declared_length_units == (_STEP_READER_UNIT_NAMES[output_unit],)
    metrics_match = bool(comparisons) and all(item.within_tolerance for item in comparisons)
    passed = (
        reader_done
        and int(payload.get("transferred_roots", 0)) > 0
        and int(payload.get("shape_count", 0)) > 0
        and nonempty
        and occt_valid
        and unit_matches
        and topology_matches
        and metrics_match
    )
    return StepReimportReport(
        schema_version=1,
        process_isolated=True,
        reader_status=str(payload.get("reader_status", "unknown")),
        reader_done=reader_done,
        candidate_roots=int(payload.get("candidate_roots", 0)),
        transferred_roots=int(payload.get("transferred_roots", 0)),
        shape_count=int(payload.get("shape_count", 0)),
        nonempty=nonempty,
        occt_valid=occt_valid,
        ocp_version=_optional_string(payload.get("ocp_version")),
        expected_length_unit=output_unit,
        declared_length_units=declared_length_units,
        expected_topology=expected_topology,
        actual_topology=actual_topology,
        metrics_mm=metrics,
        metric_comparisons=comparisons,
        passed=passed,
    )


def _run_cold_reimport(path: Path) -> dict[str, object]:
    environment = os.environ.copy()
    for name in ("PYTHONHOME", "PYTHONPATH"):
        environment.pop(name, None)
    try:
        completed = subprocess.run(
            [sys.executable, "-I", "-c", _COLD_REIMPORT_CODE, str(path)],
            cwd=path.parent,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
            timeout=120.0,
        )
    except (OSError, subprocess.SubprocessError) as error:
        export_error = _step_error(
            "step.reimport_failed",
            DiagnosticKind.INVALID,
            "the independent STEP validation process could not be executed",
            details={"exception_type": type(error).__name__},
        )
        raise export_error from error
    if completed.returncode != 0:
        raise _step_error(
            "step.reimport_failed",
            DiagnosticKind.INVALID,
            "the independent STEP validation process failed",
            details={
                "return_code": completed.returncode,
                "stdout_bytes": len(completed.stdout.encode("utf-8")),
                "stderr_bytes": len(completed.stderr.encode("utf-8")),
            },
        )
    try:
        payload = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        export_error = _step_error(
            "step.reimport_failed",
            DiagnosticKind.INVALID,
            "the independent STEP validation process returned invalid JSON",
            details={"stdout_bytes": len(completed.stdout.encode("utf-8"))},
        )
        raise export_error from error
    if not isinstance(payload, dict):
        raise _step_error(
            "step.reimport_failed",
            DiagnosticKind.INVALID,
            "the independent STEP validation process returned a non-object report",
        )
    return payload


def _metric_comparisons(
    report: ConversionReport,
    actual: OcctMetrics,
    *,
    geometry_scale_to_mm: float,
) -> tuple[StepMetricComparison, ...]:
    expected = report.metrics
    tolerances = report.options.validation
    result: list[StepMetricComparison] = []
    if expected.bounding_box is not None and actual.bounding_box is not None:
        expected_bounds = tuple(value * geometry_scale_to_mm for value in expected.bounding_box)
        differences = tuple(
            abs(left - right)
            for left, right in zip(expected_bounds, actual.bounding_box, strict=True)
        )
        thresholds = tuple(
            max(
                tolerances.linear_absolute * geometry_scale_to_mm,
                tolerances.relative * abs(value),
            )
            for value in expected_bounds
        )
        result.append(
            StepMetricComparison(
                metric="bounding_box",
                expected=expected_bounds,
                actual=actual.bounding_box,
                tolerance=float(max(thresholds)),
                maximum_absolute_difference=float(max(differences)),
                within_tolerance=all(
                    difference <= threshold
                    for difference, threshold in zip(differences, thresholds, strict=True)
                ),
            )
        )
    if expected.surface_area is not None and actual.surface_area is not None:
        expected_area = expected.surface_area * geometry_scale_to_mm**2
        area_tolerance = max(
            (tolerances.linear_absolute * geometry_scale_to_mm) ** 2,
            tolerances.relative * abs(expected_area),
        )
        difference = abs(expected_area - actual.surface_area)
        result.append(
            StepMetricComparison(
                metric="surface_area",
                expected=float(expected_area),
                actual=actual.surface_area,
                tolerance=float(area_tolerance),
                maximum_absolute_difference=float(difference),
                within_tolerance=difference <= area_tolerance,
            )
        )
    if expected.volume is not None and actual.volume is not None:
        expected_volume = expected.volume * geometry_scale_to_mm**3
        volume_tolerance = max(
            (tolerances.linear_absolute * geometry_scale_to_mm) ** 3,
            tolerances.relative * abs(expected_volume),
        )
        difference = abs(expected_volume - actual.volume)
        result.append(
            StepMetricComparison(
                metric="volume",
                expected=float(expected_volume),
                actual=actual.volume,
                tolerance=float(volume_tolerance),
                maximum_absolute_difference=float(difference),
                within_tolerance=difference <= volume_tolerance,
            )
        )
    return tuple(result)


def _metrics_from_payload(value: object) -> OcctMetrics:
    if not isinstance(value, dict):
        return OcctMetrics(None, None, None)
    bounding_box = value.get("bounding_box")
    normalized_bounds = None
    if (
        isinstance(bounding_box, list)
        and len(bounding_box) == 6
        and all(
            isinstance(item, (int, float)) and not isinstance(item, bool) for item in bounding_box
        )
    ):
        normalized_bounds = tuple(float(item) for item in bounding_box)
    return OcctMetrics(
        normalized_bounds,
        _optional_float(value.get("surface_area")),
        _optional_float(value.get("volume")),
    )


def _integer_mapping(value: object) -> dict[str, int]:
    if not isinstance(value, dict):
        return {}
    return {
        str(name): item
        for name, item in value.items()
        if isinstance(name, str)
        and isinstance(item, int)
        and not isinstance(item, bool)
        and item >= 0
    }


def _body_count(counts: dict[str, int]) -> int:
    solids = counts.get("solids", 0)
    return solids if solids > 0 else counts.get("shells", 0)


def _optional_float(value: object) -> float | None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    normalized = float(value)
    return normalized if isfinite(normalized) else None


def _optional_string(value: object) -> str | None:
    return value if isinstance(value, str) and value else None


def _string_tuple(value: object) -> tuple[str, ...]:
    if not isinstance(value, list):
        return ()
    return tuple(item for item in value if isinstance(item, str) and item)


def _write_staged_sidecar(
    path: Path,
    report: StepExportReport,
    *,
    maximum_bytes: int,
    staged_step_bytes: int,
    total_limit: int,
) -> None:
    encoder = json.JSONEncoder(
        ensure_ascii=False,
        indent=2,
        sort_keys=True,
        allow_nan=False,
    )
    written = 0
    with path.open("wb") as stream:
        for text in encoder.iterencode(report.to_dict()):
            chunk = text.encode("utf-8")
            if written + len(chunk) + 1 > maximum_bytes:
                raise _output_limit_error(
                    staged_step_bytes + written + len(chunk) + 1,
                    total_limit,
                )
            stream.write(chunk)
            written += len(chunk)
        stream.write(b"\n")
        stream.flush()
        os.fsync(stream.fileno())


def _commit_outputs(
    temporary_step: Path,
    destination: Path,
    temporary_sidecar: Path,
    sidecar_destination: Path,
    *,
    overwrite: bool,
) -> None:
    try:
        if overwrite:
            os.replace(temporary_step, destination)
            os.replace(temporary_sidecar, sidecar_destination)
            return
        step_linked = False
        try:
            os.link(temporary_step, destination)
            step_linked = True
            os.link(temporary_sidecar, sidecar_destination)
        except Exception:
            if step_linked and _same_file(temporary_step, destination):
                destination.unlink()
            raise
        temporary_step.unlink()
        temporary_sidecar.unlink()
    except FileExistsError as error:
        export_error = _step_error(
            "step.output_exists",
            DiagnosticKind.INVALID,
            "a STEP output appeared during commit; no existing file was overwritten",
        )
        raise export_error from error
    except StepExportError:
        raise
    except OSError as error:
        export_error = _step_error(
            "step.atomic_commit_failed",
            DiagnosticKind.INVALID,
            "validated STEP output could not be atomically committed",
            details={"exception_type": type(error).__name__},
        )
        raise export_error from error


def _same_file(left: Path, right: Path) -> bool:
    try:
        return os.path.samefile(left, right)
    except OSError:
        return False


def _unlink_temporary(path: Path) -> None:
    with suppress(OSError):
        path.unlink(missing_ok=True)


def _file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _source_sha256(identity: str | None) -> str | None:
    if identity is None:
        return None
    matched = _SOURCE_HASH.fullmatch(identity)
    return None if matched is None else matched.group(1)


def _status_name(status: object) -> str:
    name = getattr(status, "name", None)
    if isinstance(name, str):
        return name.removeprefix("IFSelect_")
    return str(status)


def _json_metric(value: MetricValue) -> float | list[float]:
    return list(value) if isinstance(value, tuple) else value


def _output_limit_error(observed: int, limit: int) -> StepExportError:
    return _step_error(
        "step.limit_exceeded",
        DiagnosticKind.LIMIT,
        "STEP and sidecar output exceed the configured interop byte limit",
        details={"resource": "max_output_bytes", "observed": observed, "limit": limit},
    )


def _step_error(
    code: str,
    kind: DiagnosticKind,
    message: str,
    *,
    report: StepExportReport | None = None,
    details: dict[str, str | int | float | bool | None] | None = None,
) -> StepExportError:
    return StepExportError(
        Diagnostic(
            code=code,
            severity=DiagnosticSeverity.ERROR,
            kind=kind,
            message=message,
            fatal=True,
            details={} if details is None else details,
        ),
        report=report,
    )


_COLD_REIMPORT_CODE: Final = r"""
import importlib.metadata
import json
import sys

from OCP.Bnd import Bnd_Box
from OCP.BRepBndLib import BRepBndLib
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps
from OCP.IFSelect import IFSelect_RetDone
from OCP.Message import Message
from OCP.STEPControl import STEPControl_Reader
from OCP.TColStd import TColStd_SequenceOfAsciiString
from OCP.TopAbs import TopAbs_FACE, TopAbs_SHELL, TopAbs_SOLID
from OCP.TopExp import TopExp
from OCP.TopTools import TopTools_IndexedMapOfShape

messenger = Message.DefaultMessenger_s()
messenger.ChangePrinters().Clear()
reader = STEPControl_Reader()
status = reader.ReadFile(sys.argv[1])
reader_done = status == IFSelect_RetDone
candidate_roots = reader.NbRootsForTransfer() if reader_done else 0
unit_sequences = [TColStd_SequenceOfAsciiString() for _ in range(3)]
if reader_done:
    reader.FileUnits(*unit_sequences)
declared_length_units = [
    unit_sequences[0].Value(index).ToCString()
    for index in range(1, unit_sequences[0].Size() + 1)
]
transferred_roots = reader.TransferRoots() if reader_done else 0
shape_count = reader.NbShapes() if reader_done else 0
shape = reader.OneShape() if shape_count else None
nonempty = shape is not None and not shape.IsNull()
topology = {"solids": 0, "shells": 0, "faces": 0}
bounds = None
area = None
volume = None
valid = False
if nonempty:
    for name, shape_type in (
        ("solids", TopAbs_SOLID),
        ("shells", TopAbs_SHELL),
        ("faces", TopAbs_FACE),
    ):
        indexed = TopTools_IndexedMapOfShape()
        TopExp.MapShapes_s(shape, shape_type, indexed)
        topology[name] = indexed.Extent()
    valid = bool(BRepCheck_Analyzer(shape).IsValid())
    box = Bnd_Box()
    BRepBndLib.AddOptimal_s(shape, box, False, True)
    if not box.IsVoid():
        bounds = [float(value) for value in box.Get()]
    surface = GProp_GProps()
    BRepGProp.SurfaceProperties_s(shape, surface)
    area = float(surface.Mass())
    solid = GProp_GProps()
    BRepGProp.VolumeProperties_s(shape, solid)
    volume = float(solid.Mass())
try:
    ocp_version = importlib.metadata.version("cadquery-ocp-novtk")
except importlib.metadata.PackageNotFoundError:
    try:
        ocp_version = importlib.metadata.version("cadquery-ocp")
    except importlib.metadata.PackageNotFoundError:
        ocp_version = None
print(json.dumps({
    "reader_status": getattr(status, "name", str(status)).removeprefix("IFSelect_"),
    "reader_done": reader_done,
    "candidate_roots": candidate_roots,
    "transferred_roots": transferred_roots,
    "shape_count": shape_count,
    "nonempty": nonempty,
    "occt_valid": valid,
    "ocp_version": ocp_version,
    "declared_length_units": declared_length_units,
    "topology": topology,
    "metrics": {
        "bounding_box": bounds,
        "surface_area": area,
        "volume": volume,
    },
}, sort_keys=True))
""".strip()


__all__ = [
    "StepArtifact",
    "StepExportReport",
    "StepExportResult",
    "StepMetricComparison",
    "StepReimportReport",
    "write_step",
]
