"""Dependency-free options and reports for bounded local previews."""

from __future__ import annotations

from dataclasses import dataclass
from math import isfinite
from pathlib import Path

from ..limits import InteropLimits


@dataclass(frozen=True, slots=True)
class PreviewOptions:
    """Deterministic tessellation choices expressed in OCCT target units."""

    linear_deflection: float = 0.1
    angular_deflection: float = 0.5
    include_edges: bool = True
    allow_partial: bool = False

    def __post_init__(self) -> None:
        for name in ("linear_deflection", "angular_deflection"):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                raise ValueError(f"{name} must be a finite positive number")
            normalized = float(value)
            if not isfinite(normalized) or normalized <= 0.0:
                raise ValueError(f"{name} must be a finite positive number")
            object.__setattr__(self, name, normalized)
        if self.angular_deflection > 3.141592653589793:
            raise ValueError("angular_deflection must not exceed pi radians")
        for name in ("include_edges", "allow_partial"):
            if not isinstance(getattr(self, name), bool):
                raise TypeError(f"{name} must be a boolean")

    def to_dict(self) -> dict[str, object]:
        return {
            "linear_deflection": self.linear_deflection,
            "angular_deflection": self.angular_deflection,
            "include_edges": self.include_edges,
            "allow_partial": self.allow_partial,
        }


@dataclass(frozen=True, slots=True)
class PreviewArtifact:
    filename: str
    byte_size: int
    sha256: str

    def __post_init__(self) -> None:
        if not self.filename or Path(self.filename).name != self.filename:
            raise ValueError("preview artifact filename must be one basename")
        if isinstance(self.byte_size, bool) or not isinstance(self.byte_size, int):
            raise ValueError("preview artifact byte_size must be an integer")
        if self.byte_size < 0:
            raise ValueError("preview artifact byte_size must be non-negative")
        if len(self.sha256) != 64 or any(value not in "0123456789abcdef" for value in self.sha256):
            raise ValueError("preview artifact sha256 must be lowercase hexadecimal")

    def to_dict(self) -> dict[str, object]:
        return {
            "filename": self.filename,
            "byte_size": self.byte_size,
            "sha256": self.sha256,
        }


@dataclass(frozen=True, slots=True)
class GlbValidationReport:
    valid: bool
    version: int
    declared_length: int
    binary_length: int
    mesh_count: int
    primitive_count: int
    accessor_count: int
    errors: tuple[str, ...] = ()

    def to_dict(self) -> dict[str, object]:
        return {
            "valid": self.valid,
            "version": self.version,
            "declared_length": self.declared_length,
            "binary_length": self.binary_length,
            "mesh_count": self.mesh_count,
            "primitive_count": self.primitive_count,
            "accessor_count": self.accessor_count,
            "errors": list(self.errors),
        }


@dataclass(frozen=True, slots=True)
class PreviewReport:
    schema_version: int
    status: str
    source_complete: bool
    conversion_complete: bool
    occt_valid: bool
    partial: bool
    options: PreviewOptions
    target_unit: str
    face_primitive_count: int
    edge_primitive_count: int
    triangle_count: int
    vertex_count: int
    curve_sample_count: int
    missing_face_count: int
    missing_edge_count: int
    output_bytes: int
    glb_validation: GlbValidationReport
    artifacts: tuple[PreviewArtifact, ...]
    asset_bundle_version: str
    asset_license: str
    limits: InteropLimits

    def __post_init__(self) -> None:
        if self.schema_version != 1:
            raise ValueError("preview report schema_version must be 1")
        if self.status not in {"complete", "partial"}:
            raise ValueError("preview report status must be complete or partial")
        if self.partial != (self.status == "partial"):
            raise ValueError("preview report status and partial flag must agree")
        if not self.conversion_complete or not self.occt_valid:
            raise ValueError("successful preview reports require a complete valid conversion")
        if not self.source_complete and not self.partial:
            raise ValueError("an incomplete source must produce a partial preview report")
        if self.partial and not self.options.allow_partial:
            raise ValueError("partial preview reports require allow_partial=True")
        if not self.glb_validation.valid:
            raise ValueError("successful preview reports require a valid GLB")
        for name in (
            "face_primitive_count",
            "edge_primitive_count",
            "triangle_count",
            "vertex_count",
            "curve_sample_count",
            "missing_face_count",
            "missing_edge_count",
            "output_bytes",
        ):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ValueError(f"{name} must be a non-negative integer")
        if (self.missing_face_count or self.missing_edge_count) and not self.partial:
            raise ValueError("missing entities require a partial preview report")
        filenames = [item.filename for item in self.artifacts]
        if len(filenames) != len(set(filenames)):
            raise ValueError("preview report artifact filenames must be unique")

    def to_dict(self) -> dict[str, object]:
        return {
            "schema_version": self.schema_version,
            "status": self.status,
            "source_complete": self.source_complete,
            "conversion_complete": self.conversion_complete,
            "occt_valid": self.occt_valid,
            "partial": self.partial,
            "options": self.options.to_dict(),
            "target_unit": self.target_unit,
            "counts": {
                "face_primitives": self.face_primitive_count,
                "edge_primitives": self.edge_primitive_count,
                "triangles": self.triangle_count,
                "vertices": self.vertex_count,
                "curve_samples": self.curve_sample_count,
                "missing_faces": self.missing_face_count,
                "missing_edges": self.missing_edge_count,
            },
            "output_bytes": self.output_bytes,
            "glb_validation": self.glb_validation.to_dict(),
            "artifacts": [item.to_dict() for item in self.artifacts],
            "asset_bundle": {
                "version": self.asset_bundle_version,
                "license": self.asset_license,
            },
            "limits": self.limits.to_dict(),
        }


@dataclass(frozen=True, slots=True)
class PreviewResult:
    directory: Path
    index_path: Path
    glb_path: Path
    manifest_path: Path
    report: PreviewReport


__all__ = [
    "GlbValidationReport",
    "PreviewArtifact",
    "PreviewOptions",
    "PreviewReport",
    "PreviewResult",
]
