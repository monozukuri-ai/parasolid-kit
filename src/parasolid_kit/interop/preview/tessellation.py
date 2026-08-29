"""Bounded OCCT tessellation with source-faithful preview metadata."""

from __future__ import annotations

from dataclasses import dataclass
from math import sqrt
from typing import Any

from ...brep.model import BrepModel
from ...brep.topology import SourceNodeRef
from ...diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity, SourceLocation
from ..dependency import require_occt
from ..errors import PreviewError
from ..limits import DEFAULT_INTEROP_LIMITS, InteropLimits
from ..occt.model import (
    OcctConversionResult,
    OcctShapeKind,
    ShapeRelation,
    SourceEntityKind,
    SourceEntityRef,
)
from .glb import GlbBuilder
from .model import PreviewOptions


@dataclass(frozen=True, slots=True)
class TessellationResult:
    """In-memory GLB and JSON-compatible manifest produced before file I/O."""

    glb: bytes
    manifest: dict[str, object]
    face_primitive_count: int
    edge_primitive_count: int
    triangle_count: int
    vertex_count: int
    curve_sample_count: int
    missing_face_count: int
    missing_edge_count: int


def tessellate_preview(
    converted: OcctConversionResult,
    brep: BrepModel,
    *,
    options: PreviewOptions | None = None,
    limits: InteropLimits = DEFAULT_INTEROP_LIMITS,
) -> TessellationResult:
    """Tessellate one owned OCCT result without inferring or dropping geometry."""

    resolved_options = PreviewOptions() if options is None else options
    _validate_inputs(converted, brep, resolved_options, limits)
    require_occt()
    try:
        return _tessellate(converted, brep, resolved_options, limits)
    except PreviewError:
        raise
    except OverflowError as error:
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.limit_exceeded",
                kind=DiagnosticKind.LIMIT,
                message="preview GLB buffer exceeds the configured output byte limit",
                details={
                    "resource": "max_output_bytes",
                    "observed": limits.max_output_bytes + 1,
                    "limit": limits.max_output_bytes,
                },
            )
        ) from error
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception as error:
        diagnostic = _diagnostic(
            brep,
            code="preview.tessellation_failed",
            kind=DiagnosticKind.INVALID,
            message=f"OCCT preview tessellation failed: {error}",
            details={"exception_type": type(error).__name__},
        )
        raise PreviewError(diagnostic) from error


def _validate_inputs(
    converted: OcctConversionResult,
    brep: BrepModel,
    options: PreviewOptions,
    limits: InteropLimits,
) -> None:
    if not isinstance(converted, OcctConversionResult):
        raise TypeError("converted must be OcctConversionResult")
    if not isinstance(brep, BrepModel):
        raise TypeError("brep must be BrepModel")
    if not isinstance(options, PreviewOptions):
        raise TypeError("options must be PreviewOptions")
    if not isinstance(limits, InteropLimits):
        raise TypeError("limits must be InteropLimits")
    report = converted.report
    if report.source_format != brep.source_format or report.schema_key != brep.schema_key:
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.source_mismatch",
                kind=DiagnosticKind.INVALID,
                message="OCCT conversion report does not describe the supplied BrepModel",
            )
        )
    if report.source_complete != brep.complete:
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.source_mismatch",
                kind=DiagnosticKind.INVALID,
                message="OCCT conversion completeness differs from the supplied BrepModel",
            )
        )
    if not report.conversion_complete or not report.occt_valid:
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.invalid_conversion",
                kind=DiagnosticKind.INVALID,
                message="preview requires a complete and OCCT-valid conversion result",
                details={
                    "conversion_complete": report.conversion_complete,
                    "occt_valid": report.occt_valid,
                },
            )
        )
    if not report.source_complete and not options.allow_partial:
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.source_incomplete",
                kind=DiagnosticKind.INCOMPLETE,
                message="preview requires a complete source unless allow_partial=True",
            )
        )
    entity_count = sum(
        len(getattr(brep, name))
        for name in (
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
    )
    _check_limit(brep, "max_entities", entity_count, limits.max_entities)
    _check_limit(brep, "max_occt_subshapes", len(converted.subshapes), limits.max_occt_subshapes)
    diagnostic_count = len(brep.diagnostics) + len(report.diagnostics)
    _check_limit(brep, "max_diagnostics", diagnostic_count, limits.max_diagnostics)


def _tessellate(
    converted: OcctConversionResult,
    brep: BrepModel,
    options: PreviewOptions,
    limits: InteropLimits,
) -> TessellationResult:
    from OCP.BRep import BRep_Tool
    from OCP.BRepMesh import BRepMesh_IncrementalMesh
    from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_REVERSED
    from OCP.TopExp import TopExp
    from OCP.TopLoc import TopLoc_Location
    from OCP.TopoDS import TopoDS
    from OCP.TopTools import TopTools_IndexedDataMapOfShapeListOfShape

    mesher = BRepMesh_IncrementalMesh(
        converted.shape,
        options.linear_deflection,
        False,
        options.angular_deflection,
        False,
    )
    if hasattr(mesher, "Perform"):
        mesher.Perform()
    if not mesher.IsDone():
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.tessellation_failed",
                kind=DiagnosticKind.INVALID,
                message="OCCT incremental meshing did not complete",
            )
        )

    edge_faces = TopTools_IndexedDataMapOfShapeListOfShape()
    TopExp.MapShapesAndAncestors_s(
        converted.shape,
        TopAbs_EDGE,
        TopAbs_FACE,
        edge_faces,
    )

    target_kinds = {item.key: item.kind for item in converted.subshapes}
    source_index = _SourceIndex(
        brep,
        converted.source_map.relations,
        converted.report.diagnostics,
    )
    expected_faces = source_index.expected(SourceEntityKind.FACE, OcctShapeKind.FACE, target_kinds)
    expected_edges = (
        source_index.expected(SourceEntityKind.EDGE, OcctShapeKind.EDGE, target_kinds)
        if options.include_edges
        else {}
    )
    glb = GlbBuilder(max_binary_bytes=limits.max_output_bytes)
    primitives: list[dict[str, object]] = []
    rendered_targets: set[str] = set()
    missing_targets: dict[str, str] = {}
    bounds: list[float] | None = None
    triangle_count = 0
    vertex_count = 0
    curve_sample_count = 0
    face_count = 0
    edge_count = 0
    pick_id = 1

    face_items = [item for item in converted.subshapes if item.kind is OcctShapeKind.FACE]
    for item in face_items:
        if pick_id > 0xFFFFFF:
            _check_limit(brep, "pick_ids", pick_id, 0xFFFFFF)
        face = TopoDS.Face_s(item.shape)
        location = TopLoc_Location()
        triangulation = BRep_Tool.Triangulation_s(face, location)
        if (
            triangulation is None
            or triangulation.NbNodes() <= 0
            or triangulation.NbTriangles() <= 0
        ):
            missing_targets[item.key] = "OCCT face has no triangulation"
            continue
        face_vertices = int(triangulation.NbNodes())
        face_triangles = int(triangulation.NbTriangles())
        _check_limit(brep, "max_vertices", vertex_count + face_vertices, limits.max_vertices)
        _check_limit(brep, "max_triangles", triangle_count + face_triangles, limits.max_triangles)
        transformation = location.Transformation()
        points = [
            triangulation.Node(index).Transformed(transformation)
            for index in range(1, face_vertices + 1)
        ]
        positions = [
            coordinate for point in points for coordinate in (point.X(), point.Y(), point.Z())
        ]
        indices: list[int] = []
        reversed_face = face.Orientation() == TopAbs_REVERSED
        for index in range(1, face_triangles + 1):
            first, second, third = triangulation.Triangle(index).Get()
            triangle = [int(first) - 1, int(second) - 1, int(third) - 1]
            if reversed_face:
                triangle[1], triangle[2] = triangle[2], triangle[1]
            indices.extend(triangle)
        normals = _vertex_normals(positions, indices)
        primitive_index = glb.add_triangles(
            target_key=item.key,
            pick_id=pick_id,
            positions=positions,
            normals=normals,
            indices=indices,
        )
        metadata = source_index.metadata(item.key)
        primitives.append(
            {
                "primitive_index": primitive_index,
                "pick_id": pick_id,
                "kind": "face",
                "target_key": item.key,
                "triangle_count": face_triangles,
                "vertex_count": face_vertices,
                **metadata,
            }
        )
        bounds = _extend_bounds(bounds, positions)
        rendered_targets.add(item.key)
        triangle_count += face_triangles
        vertex_count += face_vertices
        face_count += 1
        pick_id += 1

    if options.include_edges:
        edge_items = [item for item in converted.subshapes if item.kind is OcctShapeKind.EDGE]
        for item in edge_items:
            if pick_id > 0xFFFFFF:
                _check_limit(brep, "pick_ids", pick_id, 0xFFFFFF)
            edge = TopoDS.Edge_s(item.shape)
            if not edge_faces.Contains(edge):
                missing_targets[item.key] = "OCCT edge has no meshed face ancestor"
                continue
            face = TopoDS.Face_s(edge_faces.FindFromKey(edge).First())
            location = TopLoc_Location()
            triangulation = BRep_Tool.Triangulation_s(face, location)
            if triangulation is None:
                missing_targets[item.key] = "OCCT edge face has no triangulation"
                continue
            polygon = BRep_Tool.PolygonOnTriangulation_s(edge, triangulation, location)
            if polygon is None or polygon.NbNodes() < 2:
                missing_targets[item.key] = "OCCT edge has no bounded mesh polygon"
                continue
            sample_count = int(polygon.NbNodes())
            _check_limit(
                brep,
                "max_curve_samples",
                curve_sample_count + sample_count,
                limits.max_curve_samples,
            )
            node_indices = tuple(polygon.Node(index) for index in range(1, sample_count + 1))
            if edge.Orientation() == TopAbs_REVERSED:
                node_indices = tuple(reversed(node_indices))
            _check_limit(brep, "max_vertices", vertex_count + sample_count, limits.max_vertices)
            transformation = location.Transformation()
            points = [
                triangulation.Node(index).Transformed(transformation) for index in node_indices
            ]
            positions = [
                coordinate for point in points for coordinate in (point.X(), point.Y(), point.Z())
            ]
            primitive_index = glb.add_line_strip(
                target_key=item.key,
                pick_id=pick_id,
                positions=positions,
            )
            metadata = source_index.metadata(item.key)
            primitives.append(
                {
                    "primitive_index": primitive_index,
                    "pick_id": pick_id,
                    "kind": "edge",
                    "target_key": item.key,
                    "curve_sample_count": sample_count,
                    "vertex_count": sample_count,
                    **metadata,
                }
            )
            bounds = _extend_bounds(bounds, positions)
            rendered_targets.add(item.key)
            curve_sample_count += sample_count
            vertex_count += sample_count
            edge_count += 1
            pick_id += 1

    missing = source_index.missing_entities(
        expected_faces,
        expected_edges,
        rendered_targets,
        missing_targets,
    )
    missing_faces = [item for item in missing if item["kind"] == "face"]
    missing_edges = [item for item in missing if item["kind"] == "edge"]
    partial = not brep.complete or bool(missing)
    if partial and not options.allow_partial:
        first_missing = missing[0] if missing else None
        source = None
        if first_missing is not None:
            source = source_index.source_ref(str(first_missing["source_key"]))
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.incomplete_mapping",
                kind=DiagnosticKind.INCOMPLETE,
                message="preview would omit source geometry; pass allow_partial=True to inspect it",
                source=source,
                details={
                    "missing_faces": len(missing_faces),
                    "missing_edges": len(missing_edges),
                },
            )
        )
    if not primitives or face_count == 0:
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.empty_mesh",
                kind=DiagnosticKind.INVALID,
                message="preview contains no renderable face primitives",
            )
        )

    glb_payload = glb.build()
    _check_limit(brep, "max_output_bytes", len(glb_payload), limits.max_output_bytes)
    manifest = _manifest(
        converted,
        brep,
        options,
        limits,
        primitives,
        missing,
        bounds,
        face_count,
        edge_count,
        triangle_count,
        vertex_count,
        curve_sample_count,
    )
    return TessellationResult(
        glb=glb_payload,
        manifest=manifest,
        face_primitive_count=face_count,
        edge_primitive_count=edge_count,
        triangle_count=triangle_count,
        vertex_count=vertex_count,
        curve_sample_count=curve_sample_count,
        missing_face_count=len(missing_faces),
        missing_edge_count=len(missing_edges),
    )


class _SourceIndex:
    def __init__(
        self,
        brep: BrepModel,
        relations: tuple[ShapeRelation, ...],
        diagnostics: tuple[Diagnostic, ...],
    ) -> None:
        self.brep = brep
        self.relations = relations
        self.by_target: dict[str, list[ShapeRelation]] = {}
        self.by_source: dict[str, list[ShapeRelation]] = {}
        self.refs = {item.source.key: item.source for item in relations}
        for relation in relations:
            self.by_target.setdefault(relation.target_key, []).append(relation)
            self.by_source.setdefault(relation.source.key, []).append(relation)
        self.faces = {item.id: item for item in brep.faces}
        self.edges = {item.id: item for item in brep.edges}
        for face in brep.faces:
            ref = SourceEntityRef(SourceEntityKind.FACE, face.id, face.source)
            self.refs.setdefault(ref.key, ref)
        for edge in brep.edges:
            ref = SourceEntityRef(SourceEntityKind.EDGE, edge.id, edge.source)
            self.refs.setdefault(ref.key, ref)
        self.curves = {item.id: item for item in brep.curves}
        self.surfaces = {item.id: item for item in brep.surfaces}
        self.face_bodies = _face_body_map(brep)
        self.edge_bodies = {
            edge_id: tuple(sorted(body.id for body in brep.bodies if edge_id in body.edges))
            for edge_id in self.edges
        }
        self.diagnostics = diagnostics

    def expected(
        self,
        source_kind: SourceEntityKind,
        target_kind: OcctShapeKind,
        target_kinds: dict[str, OcctShapeKind],
    ) -> dict[str, tuple[str, ...]]:
        source_ids = self.faces if source_kind is SourceEntityKind.FACE else self.edges
        result: dict[str, tuple[str, ...]] = {}
        for entity_id in sorted(source_ids):
            key = f"parasolid:{source_kind.value}:{entity_id:06d}"
            targets = tuple(
                sorted(
                    {
                        relation.target_key
                        for relation in self.by_source.get(key, ())
                        if target_kinds.get(relation.target_key) is target_kind
                    }
                )
            )
            result[key] = targets
        return result

    def source_ref(self, source_key: str) -> SourceNodeRef | None:
        ref = self.refs.get(source_key)
        return None if ref is None else ref.source

    def metadata(self, target_key: str) -> dict[str, object]:
        relations = sorted(
            self.by_target.get(target_key, ()),
            key=lambda item: (item.source.key, item.relation.value, item.note or ""),
        )
        unique_sources: dict[str, dict[str, object]] = {}
        face_ids: set[int] = set()
        edge_ids: set[int] = set()
        body_ids: set[int] = set()
        surface_kinds: set[str] = set()
        curve_kinds: set[str] = set()
        for relation in relations:
            source = relation.source
            value = source.to_dict()
            value["relation"] = relation.relation.value
            if relation.note is not None:
                value["relation_note"] = relation.note
            unique_sources.setdefault(source.key, value)
            if source.kind is SourceEntityKind.FACE and source.entity_id in self.faces:
                face_ids.add(source.entity_id)
                body_ids.update(self.face_bodies.get(source.entity_id, ()))
                surface_id = self.faces[source.entity_id].surface
                if surface_id is not None and surface_id in self.surfaces:
                    surface_kinds.add(self.surfaces[surface_id].kind.value)
            elif source.kind is SourceEntityKind.SURFACE and source.entity_id in self.surfaces:
                surface_kinds.add(self.surfaces[source.entity_id].kind.value)
            elif source.kind is SourceEntityKind.EDGE and source.entity_id in self.edges:
                edge_ids.add(source.entity_id)
                body_ids.update(self.edge_bodies.get(source.entity_id, ()))
                curve_id = self.edges[source.entity_id].curve
                if curve_id is not None and curve_id in self.curves:
                    curve_kinds.add(self.curves[curve_id].kind.value)
            elif source.kind is SourceEntityKind.CURVE and source.entity_id in self.curves:
                curve_kinds.add(self.curves[source.entity_id].kind.value)
        diagnostics = _diagnostics_for_sources(
            self.diagnostics,
            tuple(item.source for item in relations),
        )
        return {
            "source_entities": [unique_sources[key] for key in sorted(unique_sources)],
            "parasolid_face_ids": sorted(face_ids),
            "parasolid_edge_ids": sorted(edge_ids),
            "body_ids": sorted(body_ids),
            "surface_kinds": sorted(surface_kinds),
            "curve_kinds": sorted(curve_kinds),
            "diagnostic_codes": sorted({item.code for item in diagnostics}),
        }

    def missing_entities(
        self,
        expected_faces: dict[str, tuple[str, ...]],
        expected_edges: dict[str, tuple[str, ...]],
        rendered_targets: set[str],
        missing_targets: dict[str, str],
    ) -> list[dict[str, object]]:
        result: list[dict[str, object]] = []
        represented_targets: set[str] = set()
        for kind, expected in (("face", expected_faces), ("edge", expected_edges)):
            for source_key, targets in expected.items():
                absent_targets = tuple(
                    target for target in targets if target not in rendered_targets
                )
                if targets and not absent_targets:
                    continue
                ref = self.refs.get(source_key)
                source = None if ref is None else ref.to_dict()
                reasons = sorted(
                    {
                        missing_targets[target]
                        for target in absent_targets
                        if target in missing_targets
                    }
                )
                represented_targets.update(absent_targets)
                result.append(
                    {
                        "kind": kind,
                        "source_key": source_key,
                        "entity_id": int(source_key.rsplit(":", 1)[1]),
                        "source": source,
                        "target_keys": list(absent_targets),
                        "reason": reasons[0] if reasons else "no mapped render primitive",
                    }
                )
        for target_key in sorted(set(missing_targets) - represented_targets - rendered_targets):
            relations = sorted(
                self.by_target.get(target_key, ()),
                key=lambda item: (item.source.key, item.relation.value, item.note or ""),
            )
            ref = None if not relations else relations[0].source
            target_kind = target_key.split(":", 2)[1]
            result.append(
                {
                    "kind": target_kind,
                    "source_key": target_key if ref is None else ref.key,
                    "entity_id": None if ref is None else ref.entity_id,
                    "source": None if ref is None else ref.to_dict(),
                    "target_keys": [target_key],
                    "reason": missing_targets[target_key],
                }
            )
        return result


def _manifest(
    converted: OcctConversionResult,
    brep: BrepModel,
    options: PreviewOptions,
    limits: InteropLimits,
    primitives: list[dict[str, object]],
    missing: list[dict[str, object]],
    bounds: list[float] | None,
    face_count: int,
    edge_count: int,
    triangle_count: int,
    vertex_count: int,
    curve_sample_count: int,
) -> dict[str, object]:
    report = converted.report
    identity = report.source_identity or ""
    candidate_hash = identity.removeprefix("sha256:") if identity.startswith("sha256:") else ""
    source_hash = (
        candidate_hash
        if len(candidate_hash) == 64
        and all(character in "0123456789abcdef" for character in candidate_hash)
        else None
    )
    diagnostics = sorted(
        (item.to_dict() for item in report.diagnostics),
        key=lambda item: (
            str(item.get("code", "")),
            int(item.get("node_id", -1)),
            int(item.get("node_type", -1)),
        ),
    )
    source: dict[str, object] = {
        "format": brep.source_format,
        "schema_key": brep.schema_key,
        "complete": brep.complete,
    }
    if source_hash is not None:
        source["sha256"] = source_hash
    return {
        "schema_version": 1,
        "producer": "parasolid-kit.interop.preview",
        "source": source,
        "conversion": {
            "complete": report.conversion_complete,
            "occt_valid": report.occt_valid,
            "target_unit": report.options.target_unit,
            "applied_scale": report.options.applied_scale,
        },
        "preview": {
            "partial": not brep.complete or bool(missing),
            "options": options.to_dict(),
            "counts": {
                "face_primitives": face_count,
                "edge_primitives": edge_count,
                "triangles": triangle_count,
                "vertices": vertex_count,
                "curve_samples": curve_sample_count,
                "missing_faces": sum(item["kind"] == "face" for item in missing),
                "missing_edges": sum(item["kind"] == "edge" for item in missing),
            },
            "bounds": bounds,
        },
        "primitives": primitives,
        "missing_entities": missing,
        "diagnostics": diagnostics,
        "limits": limits.to_dict(),
    }


def _face_body_map(brep: BrepModel) -> dict[int, tuple[int, ...]]:
    regions = {item.id: item for item in brep.regions}
    shells = {item.id: item for item in brep.shells}
    result: dict[int, set[int]] = {item.id: set() for item in brep.faces}
    for body in brep.bodies:
        for region_id in body.regions:
            region = regions.get(region_id)
            if region is None:
                continue
            for shell_id in region.shells:
                shell = shells.get(shell_id)
                if shell is None:
                    continue
                for face_id in (*shell.back_faces, *shell.front_faces):
                    result.setdefault(face_id, set()).add(body.id)
    return {key: tuple(sorted(values)) for key, values in result.items()}


def _diagnostics_for_sources(
    diagnostics: tuple[Diagnostic, ...],
    sources: tuple[Any, ...],
) -> tuple[Diagnostic, ...]:
    identities = {(item.source.node_type, item.source.node_id) for item in sources}
    entity_identities = {(item.kind.value, item.entity_id) for item in sources}
    result = []
    for diagnostic in diagnostics:
        node_match = (diagnostic.node_type, diagnostic.node_id) in identities
        details = diagnostic.details
        entity_match = (details.get("entity_kind"), details.get("entity_id")) in entity_identities
        if node_match or entity_match:
            result.append(diagnostic)
    return tuple(result)


def _vertex_normals(positions: list[float], indices: list[int]) -> list[float]:
    normals = [0.0] * len(positions)
    for index in range(0, len(indices), 3):
        first, second, third = indices[index : index + 3]
        ax, ay, az = positions[first * 3 : first * 3 + 3]
        bx, by, bz = positions[second * 3 : second * 3 + 3]
        cx, cy, cz = positions[third * 3 : third * 3 + 3]
        ux, uy, uz = bx - ax, by - ay, bz - az
        vx, vy, vz = cx - ax, cy - ay, cz - az
        nx = uy * vz - uz * vy
        ny = uz * vx - ux * vz
        nz = ux * vy - uy * vx
        for vertex in (first, second, third):
            normals[vertex * 3] += nx
            normals[vertex * 3 + 1] += ny
            normals[vertex * 3 + 2] += nz
    for index in range(0, len(normals), 3):
        nx, ny, nz = normals[index : index + 3]
        length = sqrt(nx * nx + ny * ny + nz * nz)
        if length == 0.0:
            normals[index : index + 3] = [0.0, 0.0, 1.0]
        else:
            normals[index : index + 3] = [nx / length, ny / length, nz / length]
    return normals


def _extend_bounds(bounds: list[float] | None, positions: list[float]) -> list[float]:
    axes = tuple(positions[index::3] for index in range(3))
    candidate = [min(axis) for axis in axes] + [max(axis) for axis in axes]
    if bounds is None:
        return candidate
    return [
        min(bounds[0], candidate[0]),
        min(bounds[1], candidate[1]),
        min(bounds[2], candidate[2]),
        max(bounds[3], candidate[3]),
        max(bounds[4], candidate[4]),
        max(bounds[5], candidate[5]),
    ]


def _check_limit(brep: BrepModel, resource: str, observed: int, limit: int) -> None:
    if observed <= limit:
        return
    raise PreviewError(
        _diagnostic(
            brep,
            code="preview.limit_exceeded",
            kind=DiagnosticKind.LIMIT,
            message=f"preview {resource} exceeds the configured interop limit",
            details={"resource": resource, "observed": observed, "limit": limit},
        )
    )


def _diagnostic(
    brep: BrepModel,
    *,
    code: str,
    kind: DiagnosticKind,
    message: str,
    source: SourceNodeRef | None = None,
    details: dict[str, str | int | float | bool | None] | None = None,
) -> Diagnostic:
    return Diagnostic(
        code=code,
        severity=DiagnosticSeverity.ERROR,
        kind=kind,
        message=message,
        location=None if source is None else SourceLocation(byte_offset=source.byte_range.start),
        node_type=None if source is None else source.node_type,
        node_id=(
            None
            if source is None or source.node_id is None or source.node_id < 0
            else source.node_id
        ),
        schema_key=brep.schema_key,
        fatal=True,
        details={} if details is None else details,
    )


__all__ = ["TessellationResult", "tessellate_preview"]
