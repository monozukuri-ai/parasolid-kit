"""Dependency-free result, report, and provenance models for OCCT conversion."""

from __future__ import annotations

from dataclasses import dataclass, fields
from enum import Enum

from ...brep.topology import SourceNodeRef
from ...diagnostics import Diagnostic
from ..limits import InteropLimits
from .options import OcctConversionOptions


class SourceEntityKind(str, Enum):
    BODY = "body"
    REGION = "region"
    SHELL = "shell"
    FACE = "face"
    LOOP = "loop"
    HALF_EDGE = "half_edge"
    EDGE = "edge"
    VERTEX = "vertex"
    POINT = "point"
    CURVE = "curve"
    SURFACE = "surface"


class OcctShapeKind(str, Enum):
    COMPOUND = "compound"
    COMPSOLID = "compsolid"
    SOLID = "solid"
    SHELL = "shell"
    FACE = "face"
    WIRE = "wire"
    EDGE = "edge"
    VERTEX = "vertex"


class ShapeRelationKind(str, Enum):
    DIRECT = "direct"
    SPLIT = "split"
    MERGED = "merged"
    GENERATED = "generated"


@dataclass(frozen=True, slots=True)
class SourceEntityRef:
    """Stable document-local source identity with raw-node provenance."""

    kind: SourceEntityKind
    entity_id: int
    source: SourceNodeRef

    def __post_init__(self) -> None:
        if not isinstance(self.kind, SourceEntityKind):
            object.__setattr__(self, "kind", SourceEntityKind(self.kind))
        if isinstance(self.entity_id, bool) or not isinstance(self.entity_id, int):
            raise ValueError("entity_id must be an integer")
        if self.entity_id < 0:
            raise ValueError("entity_id must be non-negative")
        if not isinstance(self.source, SourceNodeRef):
            raise TypeError("source must be SourceNodeRef")

    @property
    def key(self) -> str:
        return f"parasolid:{self.kind.value}:{self.entity_id:06d}"

    def to_dict(self) -> dict[str, object]:
        return {
            "key": self.key,
            "kind": self.kind.value,
            "entity_id": self.entity_id,
            "node_index": self.source.node_index,
            "node_type": self.source.node_type,
            "type_name": self.source.type_name,
            "node_id": self.source.node_id,
            "byte_range": self.source.byte_range.to_dict(),
        }


@dataclass(frozen=True, slots=True)
class OcctSubshape:
    """One conversion-local OCCT object owned by a conversion result."""

    key: str
    kind: OcctShapeKind
    shape: object

    def __post_init__(self) -> None:
        if not isinstance(self.kind, OcctShapeKind):
            object.__setattr__(self, "kind", OcctShapeKind(self.kind))
        if not self.key.startswith(f"occt:{self.kind.value}:"):
            raise ValueError("subshape key must match its OCCT shape kind")
        if self.shape is None:
            raise ValueError("shape must not be None")

    def metadata(self) -> dict[str, str]:
        return {"key": self.key, "kind": self.kind.value}


@dataclass(frozen=True, slots=True)
class ShapeRelation:
    source: SourceEntityRef
    target_key: str
    relation: ShapeRelationKind
    note: str | None = None

    def __post_init__(self) -> None:
        if not self.target_key.startswith("occt:"):
            raise ValueError("target_key must be a conversion-local OCCT key")
        if not isinstance(self.relation, ShapeRelationKind):
            object.__setattr__(self, "relation", ShapeRelationKind(self.relation))
        if self.note is not None and not self.note.strip():
            raise ValueError("relation note must be non-empty or None")

    def to_dict(self) -> dict[str, object]:
        result: dict[str, object] = {
            "source": self.source.to_dict(),
            "target": self.target_key,
            "relation": self.relation.value,
        }
        if self.note is not None:
            result["note"] = self.note
        return result


@dataclass(frozen=True, slots=True)
class SourceShapeMap:
    """Bidirectional many-to-many source/subshape relations."""

    relations: tuple[ShapeRelation, ...]

    def __post_init__(self) -> None:
        keys = [
            (item.source.key, item.target_key, item.relation.value, item.note)
            for item in self.relations
        ]
        if len(keys) != len(set(keys)):
            raise ValueError("source shape map contains duplicate relations")

    def targets_for(self, source_key: str) -> tuple[str, ...]:
        return tuple(
            sorted({item.target_key for item in self.relations if item.source.key == source_key})
        )

    def sources_for(self, target_key: str) -> tuple[str, ...]:
        return tuple(
            sorted({item.source.key for item in self.relations if item.target_key == target_key})
        )

    def to_dict(self) -> dict[str, object]:
        forward: dict[str, list[dict[str, str]]] = {}
        reverse: dict[str, list[dict[str, str]]] = {}
        sources: dict[str, dict[str, object]] = {}
        for item in self.relations:
            source_key = item.source.key
            sources[source_key] = item.source.to_dict()
            forward.setdefault(source_key, []).append(
                {"target": item.target_key, "relation": item.relation.value}
            )
            reverse.setdefault(item.target_key, []).append(
                {"source": source_key, "relation": item.relation.value}
            )
        return {
            "sources": {key: sources[key] for key in sorted(sources)},
            "forward": {
                key: sorted(forward[key], key=lambda value: (value["target"], value["relation"]))
                for key in sorted(forward)
            },
            "reverse": {
                key: sorted(reverse[key], key=lambda value: (value["source"], value["relation"]))
                for key in sorted(reverse)
            },
            "relations": [item.to_dict() for item in self.relations],
        }


@dataclass(frozen=True, slots=True)
class NamedCounts:
    """Sorted immutable named counts used by JSON reports."""

    values: tuple[tuple[str, int], ...]

    def __post_init__(self) -> None:
        names = [name for name, _value in self.values]
        if names != sorted(names) or len(names) != len(set(names)):
            raise ValueError("count names must be unique and sorted")
        if any(
            not name or isinstance(value, bool) or not isinstance(value, int) or value < 0
            for name, value in self.values
        ):
            raise ValueError("counts must have non-empty names and non-negative integers")

    @classmethod
    def from_dict(cls, values: dict[str, int]) -> NamedCounts:
        return cls(tuple(sorted(values.items())))

    def to_dict(self) -> dict[str, int]:
        return dict(self.values)


@dataclass(frozen=True, slots=True)
class InteropUsage:
    entities: int = 0
    occt_subshapes: int = 0
    curve_samples: int = 0
    triangles: int = 0
    vertices: int = 0
    output_bytes: int = 0
    diagnostics: int = 0

    def __post_init__(self) -> None:
        for item in fields(self):
            value = getattr(self, item.name)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ValueError(f"{item.name} usage must be a non-negative integer")

    def to_dict(self) -> dict[str, int]:
        return {item.name: getattr(self, item.name) for item in fields(self)}


@dataclass(frozen=True, slots=True)
class OcctMetrics:
    bounding_box: tuple[float, float, float, float, float, float] | None
    surface_area: float | None
    volume: float | None

    def __post_init__(self) -> None:
        if self.bounding_box is not None:
            if len(self.bounding_box) != 6:
                raise ValueError("bounding_box must contain six values")
            if any(not isinstance(value, float) for value in self.bounding_box):
                raise ValueError("bounding_box values must be floats")
        for name in ("surface_area", "volume"):
            value = getattr(self, name)
            if value is not None and not isinstance(value, float):
                raise ValueError(f"{name} must be a float or None")

    def to_dict(self) -> dict[str, object]:
        return {
            "bounding_box": None if self.bounding_box is None else list(self.bounding_box),
            "surface_area": self.surface_area,
            "volume": self.volume,
        }


@dataclass(frozen=True, slots=True)
class ConversionReport:
    schema_version: int
    producer: str
    parser_version: str
    ocp_distribution: str | None
    ocp_version: str | None
    source_identity: str | None
    source_format: str
    schema_key: str
    options: OcctConversionOptions
    source_complete: bool
    conversion_complete: bool
    occt_valid: bool
    input_topology: NamedCounts
    input_curve_kinds: NamedCounts
    input_surface_kinds: NamedCounts
    output_topology: NamedCounts
    metrics: OcctMetrics
    mapping_relation_count: int
    generated_topology_count: int
    diagnostics: tuple[Diagnostic, ...]
    limits: InteropLimits
    usage: InteropUsage
    topology_operations: tuple[str, ...] = ()
    healing_requested: bool = False
    healing_performed: bool = False
    healing_operations: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        if self.schema_version != 1:
            raise ValueError("conversion report schema_version must be 1")
        for name in ("mapping_relation_count", "generated_topology_count"):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ValueError(f"{name} must be a non-negative integer")
        if self.healing_performed and not self.healing_requested:
            raise ValueError("healing cannot be performed when it was not requested")

    def to_dict(self) -> dict[str, object]:
        return {
            "schema_version": self.schema_version,
            "producer": self.producer,
            "parser_version": self.parser_version,
            "ocp_distribution": self.ocp_distribution,
            "ocp_version": self.ocp_version,
            "source_identity": self.source_identity,
            "source_format": self.source_format,
            "schema_key": self.schema_key,
            "units": {
                "source": self.options.source_unit,
                "target": self.options.target_unit,
                "source_to_metres": self.options.source_to_metres,
                "target_to_metres": self.options.target_to_metres,
                "applied_scale": self.options.applied_scale,
            },
            "validation_tolerances": self.options.validation.to_dict(),
            "source_complete": self.source_complete,
            "conversion_complete": self.conversion_complete,
            "occt_valid": self.occt_valid,
            "input_topology": self.input_topology.to_dict(),
            "input_curve_kinds": self.input_curve_kinds.to_dict(),
            "input_surface_kinds": self.input_surface_kinds.to_dict(),
            "output_topology": self.output_topology.to_dict(),
            "metrics": self.metrics.to_dict(),
            "mapping_relation_count": self.mapping_relation_count,
            "generated_topology_count": self.generated_topology_count,
            "diagnostics": [item.to_dict() for item in self.diagnostics],
            "limits": self.limits.to_dict(),
            "usage": self.usage.to_dict(),
            "topology_operations": list(self.topology_operations),
            "healing": {
                "requested": self.healing_requested,
                "performed": self.healing_performed,
                "operations": list(self.healing_operations),
            },
        }


@dataclass(frozen=True, slots=True)
class OcctConversionResult:
    """OCCT root, owned subshapes, provenance map, and JSON-compatible report."""

    shape: object
    subshapes: tuple[OcctSubshape, ...]
    source_map: SourceShapeMap
    report: ConversionReport

    def __post_init__(self) -> None:
        if self.shape is None:
            raise ValueError("shape must not be None")
        keys = [item.key for item in self.subshapes]
        if len(keys) != len(set(keys)):
            raise ValueError("subshape keys must be unique")
        unknown_targets = {
            item.target_key for item in self.source_map.relations if item.target_key not in keys
        }
        if unknown_targets:
            raise ValueError(f"source map references unknown subshapes: {sorted(unknown_targets)}")

    def to_dict(self) -> dict[str, object]:
        return {
            "subshapes": [item.metadata() for item in self.subshapes],
            "source_map": self.source_map.to_dict(),
            "report": self.report.to_dict(),
        }
