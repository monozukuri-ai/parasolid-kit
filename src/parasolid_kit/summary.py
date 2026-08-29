"""Compact immutable summaries for high-level parse results."""

from __future__ import annotations

from collections import Counter
from collections.abc import Iterable
from dataclasses import dataclass
from typing import Literal, TypeAlias

from .binary import ParasolidDocument
from .brep import BrepMetrics, BrepModel, TopologyValidation
from .diagnostics import Diagnostic
from .schema import SchemaKey

SourceFormat: TypeAlias = Literal["binary", "text"]
KindCounts: TypeAlias = tuple[tuple[str, int], ...]


@dataclass(frozen=True, slots=True)
class BrepEntityCounts:
    """Counts of the source-model topology and geometry collections."""

    bodies: int
    regions: int
    shells: int
    faces: int
    loops: int
    half_edges: int
    edges: int
    vertices: int
    points: int
    curves: int
    surfaces: int

    def __post_init__(self) -> None:
        for name in self.__dataclass_fields__:
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ValueError(f"{name} must be a non-negative integer")

    @classmethod
    def from_model(cls, model: BrepModel) -> BrepEntityCounts:
        """Count every collection in one B-Rep source model."""

        if not isinstance(model, BrepModel):
            raise TypeError("model must be a BrepModel")
        return cls(
            bodies=len(model.bodies),
            regions=len(model.regions),
            shells=len(model.shells),
            faces=len(model.faces),
            loops=len(model.loops),
            half_edges=len(model.half_edges),
            edges=len(model.edges),
            vertices=len(model.vertices),
            points=len(model.points),
            curves=len(model.curves),
            surfaces=len(model.surfaces),
        )

    def to_dict(self) -> dict[str, int]:
        """Return deterministic JSON-compatible entity counts."""

        return {name: getattr(self, name) for name in self.__dataclass_fields__}


@dataclass(frozen=True, slots=True)
class BrepSummary:
    """Compact parse/B-Rep status without serializing every raw node."""

    source_format: SourceFormat
    modeller_version: str
    schema_key: SchemaKey
    file_size: int
    node_count: int
    resolved_schema_type_count: int
    resolved_schema_field_count: int
    complete: bool
    counts: BrepEntityCounts
    body_kind_counts: KindCounts
    curve_kind_counts: KindCounts
    surface_kind_counts: KindCounts
    topology: TopologyValidation
    metrics: BrepMetrics
    document_diagnostics: tuple[Diagnostic, ...]
    brep_diagnostics: tuple[Diagnostic, ...]

    def __post_init__(self) -> None:
        if self.source_format not in {"binary", "text"}:
            raise ValueError("source_format must be 'binary' or 'text'")
        if not self.modeller_version:
            raise ValueError("modeller_version must not be empty")
        if not isinstance(self.schema_key, SchemaKey):
            raise TypeError("schema_key must be a SchemaKey")
        for name in (
            "file_size",
            "node_count",
            "resolved_schema_type_count",
            "resolved_schema_field_count",
        ):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ValueError(f"{name} must be a non-negative integer")
        if not isinstance(self.complete, bool):
            raise TypeError("complete must be a boolean")
        if not isinstance(self.counts, BrepEntityCounts):
            raise TypeError("counts must be BrepEntityCounts")
        for name in ("body_kind_counts", "curve_kind_counts", "surface_kind_counts"):
            _validate_kind_counts(getattr(self, name), name)
        if not isinstance(self.topology, TopologyValidation):
            raise TypeError("topology must be TopologyValidation")
        if not isinstance(self.metrics, BrepMetrics):
            raise TypeError("metrics must be BrepMetrics")
        for name in ("document_diagnostics", "brep_diagnostics"):
            diagnostics = getattr(self, name)
            if not isinstance(diagnostics, tuple) or not all(
                isinstance(item, Diagnostic) for item in diagnostics
            ):
                raise TypeError(f"{name} must be a tuple of Diagnostic values")

    @classmethod
    def from_parsed(cls, document: ParasolidDocument, model: BrepModel) -> BrepSummary:
        """Build a summary after checking document/model identity."""

        if not isinstance(document, ParasolidDocument):
            raise TypeError("document must be a ParasolidDocument")
        if not isinstance(model, BrepModel):
            raise TypeError("model must be a BrepModel")
        if document.format != model.source_format:
            raise ValueError("document and B-Rep source formats do not match")
        if document.schema_key.raw != model.schema_key:
            raise ValueError("document and B-Rep schema keys do not match")
        return cls(
            source_format=document.format,
            modeller_version=document.header.modeller_version,
            schema_key=document.schema_key,
            file_size=len(document.raw_bytes),
            node_count=len(document.nodes),
            resolved_schema_type_count=document.schema_coverage.resolved_type_count,
            resolved_schema_field_count=document.schema_coverage.field_count,
            complete=model.complete,
            counts=BrepEntityCounts.from_model(model),
            body_kind_counts=_kind_counts(body.kind.value for body in model.bodies),
            curve_kind_counts=_kind_counts(curve.kind.value for curve in model.curves),
            surface_kind_counts=_kind_counts(surface.kind.value for surface in model.surfaces),
            topology=model.topology,
            metrics=model.metrics,
            document_diagnostics=document.diagnostics,
            brep_diagnostics=model.diagnostics,
        )

    @property
    def diagnostic_count(self) -> int:
        """Return parser and B-Rep diagnostic count combined."""

        return len(self.document_diagnostics) + len(self.brep_diagnostics)

    def to_dict(self) -> dict[str, object]:
        """Return a deterministic compact JSON-compatible report."""

        return {
            "format": self.source_format,
            "modeller_version": self.modeller_version,
            "schema_key": self.schema_key.to_dict(),
            "file_size": self.file_size,
            "node_count": self.node_count,
            "resolved_schema_type_count": self.resolved_schema_type_count,
            "resolved_schema_field_count": self.resolved_schema_field_count,
            "complete": self.complete,
            "counts": self.counts.to_dict(),
            "body_kind_counts": dict(self.body_kind_counts),
            "curve_kind_counts": dict(self.curve_kind_counts),
            "surface_kind_counts": dict(self.surface_kind_counts),
            "topology": {
                "valid": self.topology.valid,
                "closed_loop_count": self.topology.closed_loop_count,
                "closed_edge_ring_count": self.topology.closed_edge_ring_count,
                "euler_characteristic": self.topology.euler_characteristic,
            },
            "metrics": _metrics_dict(self.metrics),
            "diagnostic_count": self.diagnostic_count,
            "diagnostics": {
                "document": [item.to_dict() for item in self.document_diagnostics],
                "brep": [item.to_dict() for item in self.brep_diagnostics],
            },
        }


@dataclass(frozen=True, slots=True)
class ParsedBrep:
    """One parsed raw document, mapped B-Rep, and compact summary."""

    document: ParasolidDocument
    brep: BrepModel
    summary: BrepSummary

    def __post_init__(self) -> None:
        if not isinstance(self.document, ParasolidDocument):
            raise TypeError("document must be a ParasolidDocument")
        if not isinstance(self.brep, BrepModel):
            raise TypeError("brep must be a BrepModel")
        if not isinstance(self.summary, BrepSummary):
            raise TypeError("summary must be a BrepSummary")
        if self.document.format != self.brep.source_format:
            raise ValueError("document and B-Rep source formats do not match")
        if self.document.schema_key.raw != self.brep.schema_key:
            raise ValueError("document and B-Rep schema keys do not match")
        if self.summary.source_format != self.document.format:
            raise ValueError("summary source format does not match the parsed document")
        if self.summary.schema_key != self.document.schema_key:
            raise ValueError("summary schema key does not match the parsed document")
        if self.summary != BrepSummary.from_parsed(self.document, self.brep):
            raise ValueError("summary does not match the parsed document and B-Rep model")

    @property
    def complete(self) -> bool:
        """Return whether all mapped source geometry is supported."""

        return self.summary.complete


def _kind_counts(values: Iterable[str]) -> KindCounts:
    counts = Counter(values)
    return tuple(sorted(counts.items()))


def _validate_kind_counts(value: object, name: str) -> None:
    if not isinstance(value, tuple):
        raise TypeError(f"{name} must be a tuple")
    previous: str | None = None
    for item in value:
        if (
            not isinstance(item, tuple)
            or len(item) != 2
            or not isinstance(item[0], str)
            or not item[0]
            or isinstance(item[1], bool)
            or not isinstance(item[1], int)
            or item[1] <= 0
        ):
            raise ValueError(f"{name} must contain non-empty kind/count pairs")
        if previous is not None and item[0] <= previous:
            raise ValueError(f"{name} kinds must be sorted and unique")
        previous = item[0]


def _metrics_dict(metrics: BrepMetrics) -> dict[str, object]:
    bounding_box = metrics.bounding_box
    return {
        "unit_basis": "source_transmit_units",
        "bounding_box": (
            None
            if bounding_box is None
            else {
                "minimum": list(bounding_box.minimum),
                "maximum": list(bounding_box.maximum),
                "extents": list(bounding_box.extents),
            }
        ),
        "surface_area": metrics.surface_area,
        "volume": metrics.volume,
    }
