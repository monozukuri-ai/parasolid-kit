"""Dependency-free display of selected stored mesh/polyline records."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from ...brep.evaluation import _cross, _unit
from ...brep.faceted import FacetedGeometry, FacetedGeometryError
from ..limits import DEFAULT_INTEROP_LIMITS, InteropLimits
from ..occt.options import UNIT_TO_METRES
from .glb import GlbBuilder, validate_glb_bytes
from .model import PreviewOptions
from .tessellation import _extend_bounds
from .writer import ASSET_BUNDLE_VERSION, ASSET_LICENSE, _artifact, _static_assets, _write_directory


@dataclass(frozen=True, slots=True)
class StoredPreviewResult:
    """A selected stored-geometry preview, without OCCT or whole-model certification."""

    directory: Path
    index_path: Path
    glb_path: Path
    manifest_path: Path
    mesh_count: int
    polyline_count: int
    triangle_count: int


def write_faceted_preview(
    geometry: FacetedGeometry,
    destination: str | Path,
    *,
    source_unit: str,
    target_unit: str = "mm",
    index_origin: int | None = None,
    limits: InteropLimits = DEFAULT_INTEROP_LIMITS,
    overwrite: bool = False,
    include_edges: bool = True,
) -> StoredPreviewResult:
    """Write stored triangles/line strips and the bundled viewer, without OCCT.

    This route displays only selected stored records, not the complete document
    or topological face trims. No watertightness, solid volume, or kernel
    validity is claimed. Missing external meshes and ambiguous indices fail.
    The caller declares units and may select an index origin explicitly.
    """
    if not isinstance(geometry, FacetedGeometry) or not isinstance(limits, InteropLimits):
        raise TypeError("geometry and limits must be FacetedGeometry and InteropLimits")
    if not isinstance(destination, (str, Path)) or not isinstance(overwrite, bool):
        raise TypeError("destination must be a path and overwrite must be a boolean")
    if not isinstance(include_edges, bool):
        raise TypeError("include_edges must be a boolean")
    if source_unit not in UNIT_TO_METRES or target_unit not in UNIT_TO_METRES:
        raise ValueError("declare source_unit and target_unit as m, cm, mm, in, or ft")
    output = Path(destination)
    if output.name in {"", ".", ".."} or output.is_symlink():
        raise ValueError("destination must name a real preview directory")
    if output.exists() and not overwrite:
        raise FileExistsError(output)
    if output.exists() and not output.is_dir():
        raise NotADirectoryError(output)
    scale = UNIT_TO_METRES[source_unit] / UNIT_TO_METRES[target_unit]
    builder = GlbBuilder(limits.max_output_bytes)
    primitives = []
    bounds = None
    triangles = vertices = samples = 0

    def budget(name, observed):
        if observed > limits.limit_for(name):
            raise FacetedGeometryError(f"stored preview exceeds {name}")

    def append_primitive(method, **arguments):
        try:
            method(**arguments)
        except OverflowError as error:
            raise FacetedGeometryError(
                "stored preview exceeds max_output_bytes or the Float32 coordinate range"
            ) from error

    def metadata(source, kind, count, triangle_count=0):
        key = f"native:{kind}:{source.node_index:06d}"
        entity_kind = "surface" if kind == "face" else "curve"
        return {
            "primitive_index": len(primitives),
            "pick_id": len(primitives) + 1,
            "kind": kind,
            "target_key": key,
            "vertex_count": count,
            "triangle_count": triangle_count,
            "curve_sample_count": count if kind == "edge" else 0,
            "source_entities": [
                {
                    "key": f"parasolid:{entity_kind}:{source.node_index:06d}",
                    "kind": entity_kind,
                    "entity_id": source.node_index,
                    "node_index": source.node_index,
                    "node_type": source.node_type,
                    "type_name": source.type_name,
                    "node_id": source.node_id,
                    "byte_range": source.byte_range.to_dict(),
                    "relation": "direct",
                }
            ],
            "parasolid_face_ids": [],
            "parasolid_edge_ids": [],
            "body_ids": [],
            "surface_kinds": ["mesh"] if kind == "face" else [],
            "curve_kinds": ["polyline"] if kind == "edge" else [],
            "diagnostic_codes": [],
        }

    budget("max_entities", len(geometry.meshes) + len(geometry.polylines))
    budget("max_occt_subshapes", len(geometry.meshes) + len(geometry.polylines))
    if len(geometry.meshes) + len(geometry.polylines) > 0xFFFFFF:
        raise FacetedGeometryError("stored preview exceeds pick ID capacity")
    for mesh in geometry.meshes:
        if mesh.position_indices is None:
            raise FacetedGeometryError("mesh data is not embedded")
        count = mesh.position_indices.count // 3
        budget("max_triangles", triangles + count)
        budget("max_vertices", vertices + count * 3)
        positions, normals = [], []
        for triangle in mesh.triangles(index_origin=index_origin):
            a, b, c = (tuple(point) for point in triangle)
            normal = _unit(
                _cross(
                    tuple(y - x for x, y in zip(a, b, strict=True)),
                    tuple(y - x for x, y in zip(a, c, strict=True)),
                )
            )
            positions.extend(coordinate * scale for point in triangle for coordinate in point)
            normals.extend(normal * 3)
        item = metadata(mesh.source, "face", count * 3, count)
        append_primitive(
            builder.add_triangles,
            target_key=item["target_key"],
            pick_id=item["pick_id"],
            positions=positions,
            normals=normals,
            indices=list(range(count * 3)),
        )
        primitives.append(item)
        bounds = _extend_bounds(bounds, positions)
        triangles += count
        vertices += count * 3
    for polyline in geometry.polylines if include_edges else ():
        budget("max_curve_samples", samples + len(polyline.points) + int(polyline.closed))
        budget("max_vertices", vertices + len(polyline.points) + int(polyline.closed))
        points = polyline.display_points()
        positions = [coordinate * scale for point in points for coordinate in point]
        item = metadata(polyline.source, "edge", len(points))
        append_primitive(
            builder.add_line_strip,
            target_key=item["target_key"],
            pick_id=item["pick_id"],
            positions=positions,
        )
        primitives.append(item)
        bounds = _extend_bounds(bounds, positions)
        samples += len(points)
        vertices += len(points)
    if not primitives:
        raise FacetedGeometryError("stored preview contains no geometry")
    glb = builder.build()
    if not validate_glb_bytes(glb).valid:
        raise FacetedGeometryError("stored geometry did not produce a structurally valid GLB")
    manifest = {
        "schema_version": 1,
        "producer": "parasolid-kit.interop.preview",
        "source": {
            "complete": False,
            "format": geometry.source_format,
            "schema_key": geometry.schema_key,
            "scope": "stored_geometry_subset",
        },
        "conversion": {
            "backend": "stored",
            "complete": True,
            "occt_valid": None,
            "source_unit": source_unit,
            "target_unit": target_unit,
            "applied_scale": scale,
        },
        "bodies": [],
        "primitives": primitives,
        "missing_entities": [],
        "diagnostics": [],
        "preview": {
            "partial": True,
            "options": PreviewOptions(allow_partial=True, include_edges=include_edges).to_dict(),
            "bounds": bounds,
            "counts": {
                "face_primitives": len(geometry.meshes),
                "edge_primitives": len(geometry.polylines) if include_edges else 0,
                "triangles": triangles,
                "vertices": vertices,
                "curve_samples": samples,
                "missing_faces": 0,
                "missing_edges": 0,
            },
        },
        "limits": limits.to_dict(),
    }
    assets = _static_assets(geometry)
    manifest["glb"] = _artifact("preview.glb", glb).to_dict()
    manifest["asset_bundle"] = {
        "version": ASSET_BUNDLE_VERSION,
        "license": ASSET_LICENSE,
        "assets": [_artifact(name, data).to_dict() for name, data in sorted(assets.items())],
    }
    payloads = {
        **assets,
        "preview.glb": glb,
        "preview.manifest.json": (
            json.dumps(manifest, ensure_ascii=True, sort_keys=True, indent=2) + "\n"
        ).encode("ascii"),
    }
    budget("max_output_bytes", sum(map(len, payloads.values())))
    _write_directory(output, payloads, overwrite=overwrite)
    return StoredPreviewResult(
        output,
        output / "index.html",
        output / "preview.glb",
        output / "preview.manifest.json",
        len(geometry.meshes),
        len(geometry.polylines) if include_edges else 0,
        triangles,
    )
