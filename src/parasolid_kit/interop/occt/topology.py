"""Source-faithful OCCT topology construction for the exact supported subset."""

from __future__ import annotations

from dataclasses import dataclass
from math import pi, sqrt

from ...brep.geometry import (
    CircleCurve,
    ConeSurface,
    CurveGeometry,
    CylinderSurface,
    EllipseCurve,
    LineCurve,
    NurbsSurface,
    OffsetSurface,
    PlaneSurface,
    SphereSurface,
    SurfaceGeometry,
    TorusSurface,
)
from ...brep.model import BrepModel
from ...brep.topology import BodyKind, Face, HalfEdge, Sense, SourceNodeRef, Vector3
from .geometry import GeometryFactory
from .model import ShapeRelationKind, SourceEntityKind, SourceEntityRef


@dataclass(frozen=True, slots=True)
class PendingRelation:
    source: SourceEntityRef
    target: object
    relation: ShapeRelationKind
    note: str | None = None


@dataclass(frozen=True, slots=True)
class BuiltTopology:
    shape: object
    relations: tuple[PendingRelation, ...]
    operations: tuple[str, ...]


class TopologyBuilder:
    """Build shared OCCT topology directly from a validated ``BrepModel``."""

    def __init__(self, model: BrepModel, geometry: GeometryFactory) -> None:
        self.model = model
        self.geometry = geometry
        self.bodies = {item.id: item for item in model.bodies}
        self.regions = {item.id: item for item in model.regions}
        self.shells = {item.id: item for item in model.shells}
        self.faces = {item.id: item for item in model.faces}
        self.loops = {item.id: item for item in model.loops}
        self.half_edges = {item.id: item for item in model.half_edges}
        self.edges = {item.id: item for item in model.edges}
        self.vertices = {item.id: item for item in model.vertices}
        self.points = {item.id: item for item in model.points}
        self.curves = {item.id: item for item in model.curves}
        self.surfaces = {item.id: item for item in model.surfaces}

        self.vertex_shapes: dict[int, object] = {}
        self.edge_shapes: dict[int, object] = {}
        self.face_shapes: dict[int, object] = {}
        self.loop_shapes: dict[int, object] = {}
        self.half_edge_shapes: dict[int, object] = {}
        self.relations: list[PendingRelation] = []
        self.operations: list[str] = []
        self.curve_geometries: dict[int, object] = {}
        self.surface_geometries: dict[int, object] = {}
        self._resolving_curves: set[int] = set()
        self._resolving_surfaces: set[int] = set()

    def build(self) -> BuiltTopology:
        self._build_vertices()
        self._build_initial_edges()
        self._build_cylinder_faces()
        self._build_cone_faces()
        self._build_plane_faces()
        self._build_closed_analytic_faces()
        self._build_generic_surface_faces()
        missing_faces = sorted(set(self.faces) - set(self.face_shapes))
        if missing_faces:
            raise ValueError(f"unsupported face construction for source faces {missing_faces}")
        self._record_final_edge_relations()
        body_shapes = self._build_bodies()
        root = self._make_root(body_shapes)
        return BuiltTopology(root, tuple(self.relations), tuple(dict.fromkeys(self.operations)))

    def _build_vertices(self) -> None:
        point_uses: dict[int, int] = {}
        for vertex in self.model.vertices:
            point_uses[vertex.point] = point_uses.get(vertex.point, 0) + 1
        for vertex in self.model.vertices:
            point = self.points[vertex.point]
            shape = self.geometry.make_vertex(point.position)
            self.vertex_shapes[vertex.id] = shape
            self._record(SourceEntityKind.VERTEX, vertex.id, vertex.source, shape)
            relation = (
                ShapeRelationKind.SPLIT if point_uses[point.id] > 1 else ShapeRelationKind.DIRECT
            )
            self._record(SourceEntityKind.POINT, point.id, point.source, shape, relation)

    def _build_initial_edges(self) -> None:
        for edge in self.model.edges:
            curve = self._curve_for_edge(edge.id)
            if isinstance(curve.definition, LineCurve):
                if edge.start_vertex is None or edge.end_vertex is None:
                    raise ValueError(f"line edge {edge.id} must have two endpoint vertices")
                start = self.vertices[edge.start_vertex]
                end = self.vertices[edge.end_vertex]
                self.edge_shapes[edge.id] = self.geometry.make_line_edge(
                    curve.definition,
                    self.vertex_shapes[start.id],
                    self.vertex_shapes[end.id],
                    self.points[start.point].position,
                    self.points[end.point].position,
                )
            else:
                start = None if edge.start_vertex is None else self.vertices[edge.start_vertex]
                end = None if edge.end_vertex is None else self.vertices[edge.end_vertex]
                self.edge_shapes[edge.id] = self.geometry.make_curve_edge(
                    curve.definition,
                    None if start is None else self.vertex_shapes[start.id],
                    None if end is None else self.vertex_shapes[end.id],
                    None if start is None else self.points[start.point].position,
                    None if end is None else self.points[end.point].position,
                    resolve_basis=self._curve_geometry,
                )

    def _build_cylinder_faces(self) -> None:
        from OCP.BRepAdaptor import BRepAdaptor_Curve
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
        from OCP.GeomAbs import GeomAbs_Circle
        from OCP.TopAbs import TopAbs_EDGE, TopAbs_FORWARD, TopAbs_VERTEX, TopAbs_WIRE
        from OCP.TopExp import TopExp
        from OCP.TopoDS import TopoDS
        from OCP.TopTools import TopTools_IndexedMapOfShape

        for face in self.model.faces:
            surface = self._surface_for_face(face)
            if not isinstance(surface.definition, CylinderSurface):
                continue
            loop_edges = [self._single_ring_edge(loop_id, face.id) for loop_id in face.loops]
            if len(loop_edges) != 2 or len(set(loop_edges)) != 2:
                raise ValueError(
                    f"cylinder face {face.id} must have exactly two distinct periodic ring loops"
                )
            definition = surface.definition
            for edge_id in loop_edges:
                self._validate_cylinder_ring(face.id, definition, self._circle_for_edge(edge_id))
            parameters = [
                self._axis_parameter(
                    self._circle_for_edge(edge_id).center,
                    definition.point,
                    definition.axis,
                )
                for edge_id in loop_edges
            ]
            v_min, v_max = min(parameters), max(parameters)
            tolerance = self.geometry.options.validation.linear_threshold(v_max - v_min)
            if v_max - v_min <= tolerance:
                raise ValueError(f"cylinder face {face.id} has coincident ring boundaries")

            builder = BRepBuilderAPI_MakeFace(
                self.geometry.cylinder(definition),
                0.0,
                2.0 * pi,
                v_min,
                v_max,
            )
            if not builder.IsDone():
                raise ValueError(f"OCCT cylinder face {face.id} construction did not complete")
            result = builder.Face()

            edge_map = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(result, TopAbs_EDGE, edge_map)
            circle_edges: list[tuple[float, object]] = []
            seam_edges: list[object] = []
            for index in range(1, edge_map.Extent() + 1):
                candidate = TopoDS.Edge_s(edge_map.FindKey(index))
                adaptor = BRepAdaptor_Curve(candidate)
                if adaptor.GetType() == GeomAbs_Circle:
                    center = adaptor.Circle().Location()
                    parameter = self._scaled_axis_parameter(
                        (center.X(), center.Y(), center.Z()),
                        definition.point,
                        definition.axis,
                    )
                    circle_edges.append((parameter, candidate.Oriented(TopAbs_FORWARD)))
                else:
                    seam_edges.append(candidate)
            if len(circle_edges) != 2 or not seam_edges:
                raise ValueError(
                    f"OCCT cylinder face {face.id} did not expose two rings and seam topology"
                )
            source_order = sorted(zip(parameters, loop_edges, strict=True))
            target_order = sorted(circle_edges, key=lambda item: item[0])
            for (source_parameter, edge_id), (target_parameter, target) in zip(
                source_order, target_order, strict=True
            ):
                if abs(source_parameter - target_parameter) > tolerance:
                    raise ValueError(
                        f"OCCT cylinder face {face.id} ring parameter differs from its source"
                    )
                target_edge = TopoDS.Edge_s(target)
                self.edge_shapes[edge_id] = target_edge

            result = TopoDS.Face_s(self._orient_shape(result, face.sense, surface.sense))
            self.face_shapes[face.id] = result
            wire_map = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(result, TopAbs_WIRE, wire_map)
            if wire_map.Extent() != 1:
                raise ValueError(f"OCCT cylinder face {face.id} must contain one merged wire")
            merged_wire = wire_map.FindKey(1)
            for loop_id in face.loops:
                self.loop_shapes[loop_id] = merged_wire
                loop = self.loops[loop_id]
                self._record(
                    SourceEntityKind.LOOP,
                    loop.id,
                    loop.source,
                    merged_wire,
                    ShapeRelationKind.MERGED,
                    "two Parasolid ring loops form one periodic OCCT wire",
                )
                for half_edge_id in loop.half_edges:
                    half_edge = self.half_edges[half_edge_id]
                    target = self.edge_shapes[half_edge.edge]
                    self.half_edge_shapes[half_edge.id] = target
                    self._record(
                        SourceEntityKind.HALF_EDGE,
                        half_edge.id,
                        half_edge.source,
                        target,
                    )

            self._record(
                SourceEntityKind.FACE,
                face.id,
                face.source,
                result,
                ShapeRelationKind.SPLIT,
                "periodic source face is represented with generated seam topology",
            )
            self._record_surface(surface, result)
            for seam in seam_edges:
                self._record(
                    SourceEntityKind.FACE,
                    face.id,
                    face.source,
                    seam,
                    ShapeRelationKind.GENERATED,
                    "OCCT periodic seam edge",
                )
            vertex_map = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(result, TopAbs_VERTEX, vertex_map)
            for index in range(1, vertex_map.Extent() + 1):
                self._record(
                    SourceEntityKind.FACE,
                    face.id,
                    face.source,
                    vertex_map.FindKey(index),
                    ShapeRelationKind.GENERATED,
                    "OCCT periodic seam vertex",
                )
            self.operations.append("bounded_periodic_cylinder_face")

    def _build_cone_faces(self) -> None:
        from OCP.BRepAdaptor import BRepAdaptor_Curve
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
        from OCP.GeomAbs import GeomAbs_Circle
        from OCP.TopAbs import TopAbs_EDGE, TopAbs_FORWARD, TopAbs_VERTEX, TopAbs_WIRE
        from OCP.TopExp import TopExp
        from OCP.TopoDS import TopoDS
        from OCP.TopTools import TopTools_IndexedMapOfShape

        for face in self.model.faces:
            surface = self._surface_for_face(face)
            if not isinstance(surface.definition, ConeSurface):
                continue
            loop_edges = [self._single_ring_edge(loop_id, face.id) for loop_id in face.loops]
            if len(loop_edges) != 2 or len(set(loop_edges)) != 2:
                raise ValueError(
                    f"cone face {face.id} must have exactly two distinct periodic ring loops"
                )
            definition = surface.definition
            parameters = []
            for edge_id in loop_edges:
                circle = self._circle_for_edge(edge_id)
                parameters.append(self._validate_cone_ring(face.id, definition, circle))
            v_min, v_max = min(parameters), max(parameters)
            tolerance = self.geometry.options.validation.linear_threshold(v_max - v_min)
            if v_max - v_min <= tolerance:
                raise ValueError(f"cone face {face.id} has coincident ring boundaries")

            builder = BRepBuilderAPI_MakeFace(
                self.geometry.cone(definition),
                0.0,
                2.0 * pi,
                v_min,
                v_max,
            )
            if not builder.IsDone():
                raise ValueError(f"OCCT cone face {face.id} construction did not complete")
            result = builder.Face()

            edge_map = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(result, TopAbs_EDGE, edge_map)
            circle_edges: list[tuple[float, object]] = []
            seam_edges: list[object] = []
            for index in range(1, edge_map.Extent() + 1):
                candidate = TopoDS.Edge_s(edge_map.FindKey(index))
                adaptor = BRepAdaptor_Curve(candidate)
                if adaptor.GetType() == GeomAbs_Circle:
                    center = adaptor.Circle().Location()
                    parameter = self._scaled_cone_v_parameter(
                        (center.X(), center.Y(), center.Z()),
                        definition,
                    )
                    circle_edges.append((parameter, candidate.Oriented(TopAbs_FORWARD)))
                else:
                    seam_edges.append(candidate)
            if len(circle_edges) != 2 or not seam_edges:
                raise ValueError(
                    f"OCCT cone face {face.id} did not expose two rings and seam topology"
                )
            source_order = sorted(zip(parameters, loop_edges, strict=True))
            target_order = sorted(circle_edges, key=lambda item: item[0])
            for (source_parameter, edge_id), (target_parameter, target) in zip(
                source_order, target_order, strict=True
            ):
                if abs(source_parameter - target_parameter) > tolerance:
                    raise ValueError(
                        f"OCCT cone face {face.id} ring parameter differs from its source"
                    )
                self.edge_shapes[edge_id] = TopoDS.Edge_s(target)

            result = TopoDS.Face_s(self._orient_shape(result, face.sense, surface.sense))
            self.face_shapes[face.id] = result
            wire_map = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(result, TopAbs_WIRE, wire_map)
            if wire_map.Extent() != 1:
                raise ValueError(f"OCCT cone face {face.id} must contain one merged wire")
            merged_wire = wire_map.FindKey(1)
            for loop_id in face.loops:
                self.loop_shapes[loop_id] = merged_wire
                loop = self.loops[loop_id]
                self._record(
                    SourceEntityKind.LOOP,
                    loop.id,
                    loop.source,
                    merged_wire,
                    ShapeRelationKind.MERGED,
                    "two Parasolid ring loops form one periodic OCCT wire",
                )
                for half_edge_id in loop.half_edges:
                    half_edge = self.half_edges[half_edge_id]
                    target = self.edge_shapes[half_edge.edge]
                    self.half_edge_shapes[half_edge.id] = target
                    self._record(
                        SourceEntityKind.HALF_EDGE,
                        half_edge.id,
                        half_edge.source,
                        target,
                    )

            self._record(
                SourceEntityKind.FACE,
                face.id,
                face.source,
                result,
                ShapeRelationKind.SPLIT,
                "periodic source face is represented with generated seam topology",
            )
            self._record_surface(surface, result)
            for seam in seam_edges:
                self._record(
                    SourceEntityKind.FACE,
                    face.id,
                    face.source,
                    seam,
                    ShapeRelationKind.GENERATED,
                    "OCCT periodic seam edge",
                )
            vertex_map = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(result, TopAbs_VERTEX, vertex_map)
            for index in range(1, vertex_map.Extent() + 1):
                self._record(
                    SourceEntityKind.FACE,
                    face.id,
                    face.source,
                    vertex_map.FindKey(index),
                    ShapeRelationKind.GENERATED,
                    "OCCT periodic seam vertex",
                )
            self.operations.append("bounded_periodic_cone_face")

    def _build_plane_faces(self) -> None:
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
        from OCP.TopoDS import TopoDS

        for face in self.model.faces:
            surface = self._surface_for_face(face)
            if not isinstance(surface.definition, PlaneSurface):
                continue
            if not face.loops:
                raise ValueError(f"plane face {face.id} has no boundary loops")
            if len(face.loops) == 1:
                wires = [(1.0, self._make_wire(face.loops[0]))]
            else:
                wires = [
                    (self._loop_area(loop_id, surface.definition), self._make_wire(loop_id))
                    for loop_id in face.loops
                ]
            outer_index = max(range(len(wires)), key=lambda index: wires[index][0])
            outer_wire = wires[outer_index][1]
            builder = BRepBuilderAPI_MakeFace(
                self.geometry.plane(surface.definition),
                outer_wire,
                True,
            )
            for index, (_area, wire) in enumerate(wires):
                if index != outer_index:
                    builder.Add(wire)
            if not builder.IsDone():
                raise ValueError(f"OCCT plane face {face.id} construction did not complete")
            result = TopoDS.Face_s(self._orient_shape(builder.Face(), face.sense, surface.sense))
            self.face_shapes[face.id] = result
            self._record(SourceEntityKind.FACE, face.id, face.source, result)
            self._record_surface(surface, result)
            self.operations.append("trimmed_plane_face")

    def _build_closed_analytic_faces(self) -> None:
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
        from OCP.TopoDS import TopoDS

        for face in self.model.faces:
            surface = self._surface_for_face(face)
            definition = surface.definition
            if not isinstance(definition, (SphereSurface, TorusSurface)):
                continue
            if face.loops:
                raise ValueError(
                    f"{surface.kind.value} face {face.id} must be an untrimmed closed face"
                )
            exact = (
                self.geometry.sphere(definition)
                if isinstance(definition, SphereSurface)
                else self.geometry.torus(definition)
            )
            builder = BRepBuilderAPI_MakeFace(exact)
            if not builder.IsDone():
                raise ValueError(
                    f"OCCT full {surface.kind.value} face {face.id} construction did not complete"
                )
            result = TopoDS.Face_s(self._orient_shape(builder.Face(), face.sense, surface.sense))
            self.face_shapes[face.id] = result
            self._record(
                SourceEntityKind.FACE,
                face.id,
                face.source,
                result,
                ShapeRelationKind.SPLIT,
                "closed analytic source face has generated OCCT seam topology",
            )
            self._record_surface(surface, result)
            self._record_generated_face_topology(
                face,
                result,
                note=f"OCCT full {surface.kind.value} seam topology",
            )
            self.operations.append(f"full_periodic_{surface.kind.value}_face")

    def _build_generic_surface_faces(self) -> None:
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
        from OCP.TopoDS import TopoDS

        for face in self.model.faces:
            surface = self._surface_for_face(face)
            if not isinstance(surface.definition, (NurbsSurface, OffsetSurface)):
                continue
            if len(face.loops) > 1:
                raise ValueError(
                    f"{surface.kind.value} face {face.id} supports at most one explicit trim loop"
                )
            exact = self._surface_geometry(surface.id)
            if face.loops:
                builder = BRepBuilderAPI_MakeFace(
                    exact,
                    self._make_wire(face.loops[0]),
                    True,
                )
            else:
                builder = BRepBuilderAPI_MakeFace(
                    exact,
                    self.geometry.options.validation.linear_absolute,
                )
            if not builder.IsDone():
                raise ValueError(
                    f"OCCT {surface.kind.value} face {face.id} construction did not complete"
                )
            result = TopoDS.Face_s(self._orient_shape(builder.Face(), face.sense, surface.sense))
            self.face_shapes[face.id] = result
            relation = ShapeRelationKind.DIRECT if face.loops else ShapeRelationKind.SPLIT
            self._record(
                SourceEntityKind.FACE,
                face.id,
                face.source,
                result,
                relation,
                None if face.loops else "natural surface bounds generate OCCT topology",
            )
            self._record_surface(surface, result)
            if not face.loops:
                self._record_generated_face_topology(
                    face,
                    result,
                    note="OCCT natural surface boundary topology",
                )
            self.operations.append(f"exact_{surface.kind.value}_face")

    def _make_wire(self, loop_id: int) -> object:
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeWire

        loop = self.loops[loop_id]
        builder = BRepBuilderAPI_MakeWire()
        for half_edge_id in loop.half_edges:
            half_edge = self.half_edges[half_edge_id]
            target = self._edge_for_half_edge(half_edge)
            builder.Add(target)
            self.half_edge_shapes[half_edge.id] = target
            self._record(
                SourceEntityKind.HALF_EDGE,
                half_edge.id,
                half_edge.source,
                target,
            )
        if not builder.IsDone():
            raise ValueError(f"OCCT wire construction failed for loop {loop.id}")
        result = builder.Wire()
        self.loop_shapes[loop.id] = result
        self._record(SourceEntityKind.LOOP, loop.id, loop.source, result)
        return result

    def _edge_for_half_edge(self, half_edge: HalfEdge) -> object:
        from OCP.TopAbs import TopAbs_FORWARD, TopAbs_REVERSED
        from OCP.TopoDS import TopoDS

        if half_edge.dummy or half_edge.edge is None:
            raise ValueError(f"half-edge {half_edge.id} is dummy or has no source edge")
        edge = self.edges[half_edge.edge]
        orientation = None
        if half_edge.vertex is not None:
            if half_edge.vertex == edge.end_vertex:
                orientation = TopAbs_FORWARD
            elif half_edge.vertex == edge.start_vertex:
                orientation = TopAbs_REVERSED
            else:
                raise ValueError(
                    f"half-edge {half_edge.id} endpoint is not an endpoint of edge {edge.id}"
                )
        else:
            curve = self._curve_for_edge(edge.id)
            sign = _sense_sign(half_edge.sense) * _sense_sign(curve.sense)
            orientation = TopAbs_FORWARD if sign > 0 else TopAbs_REVERSED
        return TopoDS.Edge_s(self.edge_shapes[edge.id].Oriented(orientation))

    def _record_final_edge_relations(self) -> None:
        curve_uses: dict[int, int] = {}
        edge_curves: dict[int, CurveGeometry] = {}
        for edge in self.model.edges:
            curve = self._curve_for_edge(edge.id)
            edge_curves[edge.id] = curve
            curve_uses[curve.id] = curve_uses.get(curve.id, 0) + 1
        for edge in self.model.edges:
            shape = self.edge_shapes[edge.id]
            curve = edge_curves[edge.id]
            self._record(SourceEntityKind.EDGE, edge.id, edge.source, shape)
            relation = (
                ShapeRelationKind.SPLIT if curve_uses[curve.id] > 1 else ShapeRelationKind.DIRECT
            )
            self._record(
                SourceEntityKind.CURVE,
                curve.id,
                curve.source,
                shape,
                relation,
            )

    def _build_bodies(self) -> list[tuple[int, object]]:
        from OCP.BRep import BRep_Builder, BRep_Tool
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeSolid
        from OCP.TopoDS import TopoDS_Shell

        results: list[tuple[int, object]] = []
        for body in self.model.bodies:
            source_shells = [
                self.shells[shell_id]
                for region_id in body.regions
                for shell_id in self.regions[region_id].shells
            ]
            face_ids: list[int] = []
            for shell in source_shells:
                face_ids.extend(shell.back_faces)
                face_ids.extend(shell.front_faces)
            face_ids = list(dict.fromkeys(face_ids))
            if not face_ids:
                raise ValueError(f"body {body.id} has no boundary faces")

            builder = BRep_Builder()
            shell_shape = TopoDS_Shell()
            builder.MakeShell(shell_shape)
            for face_id in face_ids:
                builder.Add(shell_shape, self.face_shapes[face_id])
            shell_relation = (
                ShapeRelationKind.MERGED if len(source_shells) > 1 else ShapeRelationKind.DIRECT
            )
            for shell in source_shells:
                self._record(
                    SourceEntityKind.SHELL,
                    shell.id,
                    shell.source,
                    shell_shape,
                    shell_relation,
                )

            if body.kind is BodyKind.SOLID:
                if not BRep_Tool.IsClosed_s(shell_shape):
                    raise ValueError(f"body {body.id} boundary shell is not closed")
                solid_builder = BRepBuilderAPI_MakeSolid(shell_shape)
                if not solid_builder.IsDone():
                    raise ValueError(f"OCCT solid construction failed for body {body.id}")
                body_shape = solid_builder.Solid()
                self.operations.extend(("shared_boundary_shell", "solid_from_closed_shell"))
            elif body.kind is BodyKind.SHEET:
                body_shape = shell_shape
                self.operations.append("sheet_boundary_shell")
            else:
                raise ValueError(f"body {body.id} kind {body.kind.value} is unsupported")

            self._record(SourceEntityKind.BODY, body.id, body.source, body_shape)
            source_regions = [self.regions[region_id] for region_id in body.regions]
            region_relation = (
                ShapeRelationKind.MERGED if len(source_regions) > 1 else ShapeRelationKind.DIRECT
            )
            for region in source_regions:
                self._record(
                    SourceEntityKind.REGION,
                    region.id,
                    region.source,
                    body_shape,
                    region_relation,
                )
            results.append((body.id, body_shape))
        return results

    def _make_root(self, body_shapes: list[tuple[int, object]]) -> object:
        from OCP.BRep import BRep_Builder
        from OCP.TopoDS import TopoDS_Compound

        if not body_shapes:
            raise ValueError("B-Rep model contains no bodies")
        if len(body_shapes) == 1:
            return body_shapes[0][1]
        builder = BRep_Builder()
        compound = TopoDS_Compound()
        builder.MakeCompound(compound)
        for _body_id, shape in body_shapes:
            builder.Add(compound, shape)
        self.operations.append("multi_body_compound")
        return compound

    def _curve_for_edge(self, edge_id: int) -> CurveGeometry:
        edge = self.edges[edge_id]
        curve_ids = {edge.curve} if edge.curve is not None else set()
        curve_ids.update(
            self.half_edges[half_edge_id].curve
            for half_edge_id in edge.half_edges
            if self.half_edges[half_edge_id].curve is not None
        )
        if len(curve_ids) != 1:
            raise ValueError(
                f"edge {edge.id} must resolve to exactly one curve, observed {sorted(curve_ids)}"
            )
        return self.curves[curve_ids.pop()]

    def _curve_geometry(self, curve_id: int) -> object:
        cached = self.curve_geometries.get(curve_id)
        if cached is not None:
            return cached
        if curve_id in self._resolving_curves:
            raise ValueError(f"curve basis reference cycle includes curve {curve_id}")
        curve = self.curves.get(curve_id)
        if curve is None:
            raise ValueError(f"curve basis {curve_id} is missing")
        self._resolving_curves.add(curve_id)
        try:
            result = self.geometry.curve3d(
                curve.definition,
                resolve_basis=self._curve_geometry,
            )
        finally:
            self._resolving_curves.remove(curve_id)
        self.curve_geometries[curve_id] = result
        return result

    def _surface_for_face(self, face: Face) -> SurfaceGeometry:
        if face.surface is None:
            raise ValueError(f"face {face.id} has no surface")
        return self.surfaces[face.surface]

    def _surface_geometry(self, surface_id: int) -> object:
        cached = self.surface_geometries.get(surface_id)
        if cached is not None:
            return cached
        if surface_id in self._resolving_surfaces:
            raise ValueError(f"surface basis reference cycle includes surface {surface_id}")
        surface = self.surfaces.get(surface_id)
        if surface is None:
            raise ValueError(f"surface basis {surface_id} is missing")
        self._resolving_surfaces.add(surface_id)
        try:
            result = self.geometry.surface3d(
                surface.definition,
                resolve_basis=self._surface_geometry,
            )
        finally:
            self._resolving_surfaces.remove(surface_id)
        self.surface_geometries[surface_id] = result
        return result

    def _circle_for_edge(self, edge_id: int) -> CircleCurve:
        definition = self._curve_for_edge(edge_id).definition
        if not isinstance(definition, CircleCurve):
            raise ValueError(f"cylinder boundary edge {edge_id} is not an exact circle")
        return definition

    def _single_ring_edge(self, loop_id: int, face_id: int) -> int:
        loop = self.loops[loop_id]
        if len(loop.half_edges) != 1:
            raise ValueError(
                f"cylinder face {face_id} loop {loop_id} is not one periodic half-edge"
            )
        half_edge = self.half_edges[loop.half_edges[0]]
        if half_edge.dummy or half_edge.edge is None or half_edge.vertex is not None:
            raise ValueError(f"cylinder face {face_id} loop {loop_id} is not a vertex-free ring")
        self._circle_for_edge(half_edge.edge)
        return half_edge.edge

    def _axis_parameter(
        self,
        point: Vector3,
        origin: Vector3,
        axis: Vector3,
    ) -> float:
        offset = tuple(
            point_value - origin_value
            for point_value, origin_value in zip(point, origin, strict=True)
        )
        axis_values = axis.to_tuple()
        magnitude = sqrt(sum(value * value for value in axis_values))
        if magnitude <= 1.0e-15:
            raise ValueError("cylinder axis must be non-zero")
        return (
            sum(left * right for left, right in zip(offset, axis_values, strict=True))
            / magnitude
            * self.geometry.scale
        )

    def _validate_cylinder_ring(
        self,
        face_id: int,
        cylinder: CylinderSurface,
        circle: CircleCurve,
    ) -> None:
        axis = cylinder.axis.to_tuple()
        normal = circle.normal.to_tuple()
        axis_magnitude = sqrt(sum(value * value for value in axis))
        normal_magnitude = sqrt(sum(value * value for value in normal))
        if axis_magnitude <= 1.0e-15 or normal_magnitude <= 1.0e-15:
            raise ValueError(f"cylinder face {face_id} has a zero ring or surface axis")
        alignment = abs(
            sum(left * right for left, right in zip(axis, normal, strict=True))
            / (axis_magnitude * normal_magnitude)
        )
        if abs(1.0 - alignment) > 1.0e-9:
            raise ValueError(f"cylinder face {face_id} ring normal is not parallel to its axis")
        offset = tuple(
            point - origin for point, origin in zip(circle.center, cylinder.point, strict=True)
        )
        cross = (
            offset[1] * axis[2] - offset[2] * axis[1],
            offset[2] * axis[0] - offset[0] * axis[2],
            offset[0] * axis[1] - offset[1] * axis[0],
        )
        off_axis = (
            sqrt(sum(value * value for value in cross)) / axis_magnitude * self.geometry.scale
        )
        radius = cylinder.radius * self.geometry.scale
        tolerance = self.geometry.options.validation.linear_threshold(radius)
        if off_axis > tolerance:
            raise ValueError(
                f"cylinder face {face_id} ring center is {off_axis:g} target units off-axis"
            )
        circle_radius = circle.radius * self.geometry.scale
        if abs(circle_radius - radius) > tolerance:
            raise ValueError(f"cylinder face {face_id} ring radius differs from its surface")

    def _validate_cone_ring(
        self,
        face_id: int,
        cone: ConeSurface,
        circle: CircleCurve,
    ) -> float:
        axis = cone.axis.to_tuple()
        normal = circle.normal.to_tuple()
        axis_magnitude = sqrt(sum(value * value for value in axis))
        normal_magnitude = sqrt(sum(value * value for value in normal))
        if axis_magnitude <= 1.0e-15 or normal_magnitude <= 1.0e-15:
            raise ValueError(f"cone face {face_id} has a zero ring or surface axis")
        alignment = abs(
            sum(left * right for left, right in zip(axis, normal, strict=True))
            / (axis_magnitude * normal_magnitude)
        )
        if abs(1.0 - alignment) > 1.0e-9:
            raise ValueError(f"cone face {face_id} ring normal is not parallel to its axis")
        offset = tuple(
            point - origin for point, origin in zip(circle.center, cone.point, strict=True)
        )
        cross = (
            offset[1] * axis[2] - offset[2] * axis[1],
            offset[2] * axis[0] - offset[0] * axis[2],
            offset[0] * axis[1] - offset[1] * axis[0],
        )
        off_axis = (
            sqrt(sum(value * value for value in cross)) / axis_magnitude * self.geometry.scale
        )
        tolerance = self.geometry.options.validation.linear_threshold(
            max(cone.radius, circle.radius) * self.geometry.scale
        )
        if off_axis > tolerance:
            raise ValueError(
                f"cone face {face_id} ring center is {off_axis:g} target units off-axis"
            )
        axis_distance = (
            sum(left * right for left, right in zip(offset, axis, strict=True))
            / axis_magnitude
            * self.geometry.scale
        )
        if abs(cone.cos_half_angle) <= 1.0e-15:
            raise ValueError(f"cone face {face_id} has an invalid half-angle cosine")
        parameter = axis_distance / cone.cos_half_angle
        expected_radius = cone.radius * self.geometry.scale + (parameter * cone.sin_half_angle)
        if expected_radius <= tolerance:
            raise ValueError(f"cone face {face_id} ring reaches or crosses the cone apex")
        if abs(circle.radius * self.geometry.scale - expected_radius) > tolerance:
            raise ValueError(f"cone face {face_id} ring radius differs from its surface")
        return parameter

    def _scaled_cone_v_parameter(
        self,
        point: tuple[float, float, float],
        cone: ConeSurface,
    ) -> float:
        origin = self.geometry.coordinates(cone.point)
        axis = cone.axis.to_tuple()
        magnitude = sqrt(sum(value * value for value in axis))
        axis_distance = (
            sum(
                (coordinate - base) * direction
                for coordinate, base, direction in zip(point, origin, axis, strict=True)
            )
            / magnitude
        )
        return axis_distance / cone.cos_half_angle

    def _scaled_axis_parameter(
        self,
        point: tuple[float, float, float],
        source_origin: Vector3,
        source_axis: Vector3,
    ) -> float:
        origin = self.geometry.coordinates(source_origin)
        axis = source_axis.to_tuple()
        magnitude = sqrt(sum(value * value for value in axis))
        return (
            sum(
                (coordinate - base) * direction
                for coordinate, base, direction in zip(point, origin, axis, strict=True)
            )
            / magnitude
        )

    def _loop_area(self, loop_id: int, surface: PlaneSurface) -> float:
        loop = self.loops[loop_id]
        if len(loop.half_edges) == 1:
            half_edge = self.half_edges[loop.half_edges[0]]
            if half_edge.edge is not None:
                curve = self._curve_for_edge(half_edge.edge).definition
                if isinstance(curve, CircleCurve):
                    return pi * (curve.radius * self.geometry.scale) ** 2
                if isinstance(curve, EllipseCurve):
                    return pi * curve.major_radius * curve.minor_radius * self.geometry.scale**2
        points: list[tuple[float, float, float]] = []
        for half_edge_id in loop.half_edges:
            half_edge = self.half_edges[half_edge_id]
            if half_edge.vertex is None:
                raise ValueError(f"plane loop {loop.id} contains a vertex-free non-circle edge")
            vertex = self.vertices[half_edge.vertex]
            points.append(self.geometry.coordinates(self.points[vertex.point].position))
        if len(points) < 3:
            raise ValueError(f"plane loop {loop.id} must contain at least three vertices")
        area_vector = [0.0, 0.0, 0.0]
        for current, following in zip(points, points[1:] + points[:1], strict=True):
            area_vector[0] += current[1] * following[2] - current[2] * following[1]
            area_vector[1] += current[2] * following[0] - current[0] * following[2]
            area_vector[2] += current[0] * following[1] - current[1] * following[0]
        normal = surface.normal.to_tuple()
        normal_magnitude = sqrt(sum(value * value for value in normal))
        return abs(sum(a * b for a, b in zip(area_vector, normal, strict=True))) / (
            2.0 * normal_magnitude
        )

    def _orient_shape(self, shape: object, face_sense: Sense, surface_sense: Sense) -> object:
        if _sense_sign(face_sense) * _sense_sign(surface_sense) > 0:
            return shape
        return shape.Reversed()

    def _record_surface(self, surface: SurfaceGeometry, shape: object) -> None:
        uses = sum(face.surface == surface.id for face in self.model.faces)
        relation = ShapeRelationKind.SPLIT if uses > 1 else ShapeRelationKind.DIRECT
        self._record(
            SourceEntityKind.SURFACE,
            surface.id,
            surface.source,
            shape,
            relation,
        )

    def _record_generated_face_topology(
        self,
        face: Face,
        shape: object,
        *,
        note: str,
    ) -> None:
        from OCP.TopAbs import TopAbs_EDGE, TopAbs_VERTEX
        from OCP.TopExp import TopExp
        from OCP.TopTools import TopTools_IndexedMapOfShape

        for kind in (TopAbs_EDGE, TopAbs_VERTEX):
            indexed = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(shape, kind, indexed)
            for index in range(1, indexed.Extent() + 1):
                self._record(
                    SourceEntityKind.FACE,
                    face.id,
                    face.source,
                    indexed.FindKey(index),
                    ShapeRelationKind.GENERATED,
                    note,
                )

    def _record(
        self,
        kind: SourceEntityKind,
        entity_id: int,
        source: SourceNodeRef,
        target: object,
        relation: ShapeRelationKind = ShapeRelationKind.DIRECT,
        note: str | None = None,
    ) -> None:
        self.relations.append(
            PendingRelation(SourceEntityRef(kind, entity_id, source), target, relation, note)
        )


def _sense_sign(sense: Sense) -> int:
    if sense is Sense.POSITIVE:
        return 1
    if sense is Sense.NEGATIVE:
        return -1
    raise ValueError("unknown orientation sense cannot be converted")


__all__ = ["BuiltTopology", "PendingRelation", "TopologyBuilder"]
