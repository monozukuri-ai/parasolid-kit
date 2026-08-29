"""Strict optional OCCT conversion with dependency-free imports."""

from types import ModuleType

from ..dependency import require_occt
from .conversion import to_occt
from .coverage import (
    GEOMETRY_COVERAGE,
    CoverageStatus,
    GeometryCategory,
    GeometryCoverage,
    geometry_coverage,
    render_geometry_coverage_markdown,
)
from .model import (
    ConversionReport,
    InteropUsage,
    NamedCounts,
    OcctConversionResult,
    OcctMetrics,
    OcctShapeKind,
    OcctSubshape,
    ShapeRelation,
    ShapeRelationKind,
    SourceEntityKind,
    SourceEntityRef,
    SourceShapeMap,
)
from .options import (
    UNIT_TO_METRES,
    LengthUnit,
    OcctConversionOptions,
    ValidationTolerances,
)
from .step import (
    StepArtifact,
    StepExportReport,
    StepExportResult,
    StepMetricComparison,
    StepReimportReport,
    write_step,
)


def load_runtime() -> ModuleType:
    """Validate the installed profile before importing and returning ``OCP``."""

    return require_occt()


__all__ = [
    "GEOMETRY_COVERAGE",
    "UNIT_TO_METRES",
    "ConversionReport",
    "CoverageStatus",
    "GeometryCategory",
    "GeometryCoverage",
    "InteropUsage",
    "LengthUnit",
    "NamedCounts",
    "OcctConversionOptions",
    "OcctConversionResult",
    "OcctMetrics",
    "OcctShapeKind",
    "OcctSubshape",
    "ShapeRelation",
    "ShapeRelationKind",
    "SourceEntityKind",
    "SourceEntityRef",
    "SourceShapeMap",
    "StepArtifact",
    "StepExportReport",
    "StepExportResult",
    "StepMetricComparison",
    "StepReimportReport",
    "ValidationTolerances",
    "geometry_coverage",
    "load_runtime",
    "render_geometry_coverage_markdown",
    "to_occt",
    "write_step",
]
