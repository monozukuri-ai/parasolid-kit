"""Shared trimmed topology with source UV curves and bounded intersection fits.

Imported only after the optional OCCT runtime has been resolved. Representation
operations construct pcurves, seams and parameter agreement; they do not sew,
move, or discard source faces. Every source boundary remains mapped.
"""

from __future__ import annotations

from collections import Counter, defaultdict
from math import isfinite

from OCP.BRep import BRep_Builder, BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Curve
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge, BRepBuilderAPI_MakeSolid
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepLib import BRepLib
from OCP.BSplCLib import BSplCLib
from OCP.Precision import Precision
from OCP.ShapeBuild import ShapeBuild_ReShape
from OCP.ShapeFix import ShapeFix_Edge, ShapeFix_Face, ShapeFix_Wire
from OCP.TopAbs import TopAbs_EDGE, TopAbs_FORWARD, TopAbs_REVERSED, TopAbs_VERTEX, TopAbs_WIRE
from OCP.TopExp import TopExp
from OCP.TopoDS import TopoDS, TopoDS_Face, TopoDS_Shell
from OCP.TopTools import TopTools_IndexedMapOfShape

from ...brep.geometry import (
    IntersectionCurve,
    LineCurve,
    NurbsCurve,
    PlaneSurface,
    SurfaceParametricCurve,
    TrimmedCurve,
)
from ...brep.topology import BodyKind, RegionKind, Sense
from ...diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity, SourceLocation
from .intersection import fit_intersection, intersect_analytic, project_curve
from .model import ShapeRelationKind, SourceEntityKind
from .topology import BuiltTopology, TopologyBuilder


def subshapes(shape: object, kind: object) -> list:
    indexed = TopTools_IndexedMapOfShape()
    TopExp.MapShapes_s(shape, kind, indexed)
    return [indexed.FindKey(i) for i in range(1, indexed.Extent() + 1)]


class ParametricTopologyBuilder(TopologyBuilder):
    """Two-pass face construction keeps both sides of each shared edge consistent."""

    def __init__(self, model, geometry):
        super().__init__(model, geometry)
        self.context = ShapeBuild_ReShape()
        self.reversed_edges = set()
        resolutions = [
            b.linear_resolution * geometry.scale
            for b in model.bodies
            if b.linear_resolution is not None and b.linear_resolution > 0
        ]
        self.precision = min(resolutions, default=Precision.Confusion_s() / 10)
        if not isfinite(self.precision) or self.precision <= 0:
            raise ValueError("body linear resolution must be finite and positive")
        self.curve_anchors = defaultdict(list)
        for curve in model.curves:
            d = curve.definition
            if isinstance(d, TrimmedCurve):
                self.curve_anchors[d.basis_curve].extend(
                    (geometry.point(d.start_point), geometry.point(d.end_point))
                )
        self.geometry_checks = Counter()
        self.diagnostics = []

    def build(self) -> BuiltTopology:
        self._build_vertices()
        for edge in self.model.edges:
            self._build_edge(edge)
        # All face pcurves must exist before a periodic seam splits shared rings.
        for face in self.model.faces:
            self._build_face(face)
        for face in self.model.faces:
            self._finish_face(face)
        for face in self.model.faces:
            shape = TopoDS.Face_s(self.context.Apply(self.face_shapes[face.id]))
            if not BRepCheck_Analyzer(shape).IsValid():
                raise ValueError(f"OCCT parametric face {face.id} is invalid")
            self.face_shapes[face.id] = shape
        self._validate_source_boundaries()
        # The base wire builder records provisional relations; resolve them only
        # after every seam replacement has propagated to both adjacent faces.
        self.relations.clear()
        self._record_boundaries()
        bodies = self._build_bodies()
        return BuiltTopology(
            self._make_root(bodies),
            tuple(self.relations),
            tuple(dict.fromkeys(self.operations)),
            tuple(self.diagnostics),
        )

    def _edge_tolerance(self, edge):
        if edge.tolerance is not None and (not isfinite(edge.tolerance) or edge.tolerance < 0):
            raise ValueError(f"edge {edge.id} has an invalid source tolerance")
        return max(
            Precision.Confusion_s(),
            (edge.tolerance or self.precision / self.geometry.scale) * self.geometry.scale,
        )

    def _build_vertices(self):
        super()._build_vertices()
        builder = BRep_Builder()
        for vertex in self.model.vertices:
            tolerance = vertex.tolerance or self.precision / self.geometry.scale
            if not isfinite(tolerance) or tolerance < 0:
                raise ValueError(f"vertex {vertex.id} has an invalid tolerance")
            builder.UpdateVertex(
                self.vertex_shapes[vertex.id],
                max(Precision.Confusion_s(), tolerance * self.geometry.scale),
            )

    def _curve_geometry(self, curve_id):
        if curve_id in self.curve_geometries:
            return self.curve_geometries[curve_id]
        definition = self.curves[curve_id].definition
        if not isinstance(definition, IntersectionCurve):
            return super()._curve_geometry(curve_id)
        if (
            not definition.start_points
            or not definition.end_points
            or len(definition.chart_points) < 2
        ):
            raise ValueError(f"intersection {curve_id} requires source CHART and LIMIT points")
        points = [
            self.geometry.point(p)
            for p in (
                definition.start_points[0],
                *definition.chart_points,
                definition.end_points[0],
            )
        ]
        surfaces = [self._surface_geometry(i) for i in definition.surfaces]
        if any(isinstance(self.surfaces[i].definition, PlaneSurface) for i in definition.surfaces):
            result = intersect_analytic(
                surfaces, points, max(self.precision, Precision.Confusion_s())
            )
            self.operations.append("source_identified_surface_intersection")
        else:
            result = fit_intersection(
                surfaces, points, self.curve_anchors[curve_id], self.precision
            )
            self.operations.append("intersection_curve_numerical_approximation")
        self.curve_geometries[curve_id] = result
        return result

    def _parameter_curve(self, definition):
        curve = self.curves[definition.parameter_curve].definition
        if not isinstance(curve, NurbsCurve):
            raise ValueError("surface parameter curve must reference NURBS")
        return self.geometry.parameter_curve(curve, self.surfaces[definition.surface].definition)

    def _surface_trim(self, curve_id):
        definition = None if curve_id is None else self.curves[curve_id].definition
        if isinstance(definition, TrimmedCurve):
            basis = self.curves[definition.basis_curve].definition
            if isinstance(basis, SurfaceParametricCurve):
                return definition, basis
        return None

    def _build_edge(self, edge):
        tolerance = self._edge_tolerance(edge)
        vs = None if edge.start_vertex is None else self.vertex_shapes[edge.start_vertex]
        ve = None if edge.end_vertex is None else self.vertex_shapes[edge.end_vertex]
        if (vs is None) != (ve is None):
            raise ValueError(f"edge {edge.id} has only one endpoint vertex")
        ps, pe = (None, None) if vs is None else (BRep_Tool.Pnt_s(vs), BRep_Tool.Pnt_s(ve))

        def endpoints(q1, q2):
            nonlocal vs, ve
            if vs is None:
                return
            if q1.Distance(ps) + q2.Distance(pe) > q1.Distance(pe) + q2.Distance(ps):
                vs, ve = ve, vs
                self.reversed_edges.add(edge.id)
            for point, vertex in ((q1, vs), (q2, ve)):
                if point.Distance(BRep_Tool.Pnt_s(vertex)) > max(
                    tolerance, BRep_Tool.Tolerance_s(vertex)
                ):
                    raise ValueError(
                        f"edge {edge.id} geometry misses its source endpoint tolerance"
                    )

        if edge.curve is None:
            if not edge.half_edges or vs is None:
                raise ValueError(f"edge {edge.id} requires finite FIN geometry")
            trimmed = self._surface_trim(self.half_edges[edge.half_edges[0]].curve)
            if trimmed is None:
                raise ValueError(f"edge {edge.id} requires trimmed surface-parametric FIN curves")
            definition, support = trimmed
            curve = self._parameter_curve(support)
            surface = self._surface_geometry(support.surface)
            a, b = definition.start_parameter, definition.end_parameter
            if a >= b:
                raise ValueError(f"edge {edge.id} requires increasing UV trim parameters")
            q1, q2 = (surface.Value(curve.Value(t).X(), curve.Value(t).Y()) for t in (a, b))
            for point, source in ((q1, definition.start_point), (q2, definition.end_point)):
                if point.Distance(self.geometry.point(source)) > tolerance:
                    raise ValueError(f"edge {edge.id} UV trim disagrees with its source point")
            endpoints(q1, q2)
            builder = BRepBuilderAPI_MakeEdge(curve, surface, vs, ve, a, b)
            self.operations.append("surface_parametric_curve_3d_approximation")
        else:
            source = self.curves[edge.curve]
            definition = source.definition
            if isinstance(definition, LineCurve):
                if vs is None:
                    raise ValueError(f"line edge {edge.id} requires endpoints")
                self.edge_shapes[edge.id] = self.geometry.make_line_edge(
                    definition,
                    vs,
                    ve,
                    self.points[self.vertices[edge.start_vertex].point].position,
                    self.points[self.vertices[edge.end_vertex].point].position,
                )
                return
            if isinstance(definition, TrimmedCurve):
                basis = self.curves[definition.basis_curve]
                curve = self._curve_geometry(basis.id)
                a, b = definition.start_parameter, definition.end_parameter
                if isinstance(basis.definition, IntersectionCurve):
                    a, da = project_curve(curve, self.geometry.point(definition.start_point))
                    b, db = project_curve(curve, self.geometry.point(definition.end_point))
                    if max(da, db) > tolerance:
                        raise ValueError(
                            f"intersection edge {edge.id} misses its source trim points"
                        )
                elif isinstance(basis.definition, LineCurve):
                    a, b = a * self.geometry.scale, b * self.geometry.scale
                for parameter, point in ((a, definition.start_point), (b, definition.end_point)):
                    if curve.Value(parameter).Distance(self.geometry.point(point)) > tolerance:
                        raise ValueError(
                            f"trimmed edge {edge.id} parameter disagrees with its source point"
                        )
                a, b = sorted((a, b))
                endpoints(curve.Value(a), curve.Value(b))
                builder = (
                    BRepBuilderAPI_MakeEdge(curve, a, b)
                    if vs is None
                    else BRepBuilderAPI_MakeEdge(curve, vs, ve, a, b)
                )
            else:
                curve = self._curve_geometry(source.id)
                if vs is None:
                    if not curve.IsClosed():
                        raise ValueError(f"edge {edge.id} is unbounded")
                    builder = BRepBuilderAPI_MakeEdge(curve)
                else:
                    a, da = project_curve(curve, ps)
                    b, db = project_curve(curve, pe)
                    if max(da, db) > max(
                        tolerance, BRep_Tool.Tolerance_s(vs), BRep_Tool.Tolerance_s(ve)
                    ):
                        raise ValueError(f"edge {edge.id} cannot project its source vertices")
                    if curve.IsPeriodic():
                        if source.sense is Sense.POSITIVE and b <= a:
                            b += curve.Period()
                        elif source.sense is Sense.NEGATIVE and a <= b:
                            a += curve.Period()
                    a, b = sorted((a, b))
                    endpoints(curve.Value(a), curve.Value(b))
                    builder = BRepBuilderAPI_MakeEdge(curve, vs, ve, a, b)
        if not builder.IsDone():
            raise ValueError(f"OCCT edge {edge.id} construction failed: {builder.Error()}")
        shape = builder.Edge()
        BRep_Builder().UpdateEdge(shape, tolerance)
        if not BRepLib.BuildCurve3d_s(shape, tolerance, MaxSegment=1000):
            raise ValueError(f"OCCT edge {edge.id} 3D approximation failed")
        self.edge_shapes[edge.id] = shape

    def _edge_for_half_edge(self, half_edge):
        edge = self.edges[half_edge.edge]
        if half_edge.vertex is None:
            if edge.curve is None:
                raise ValueError(f"half-edge {half_edge.id} has no orientation reference")
            forward = half_edge.sense is self.curves[edge.curve].sense
        elif half_edge.vertex == edge.end_vertex:
            forward = True
        elif half_edge.vertex == edge.start_vertex:
            forward = False
        else:
            raise ValueError(f"half-edge {half_edge.id} does not end at its edge vertex")
        if edge.id in self.reversed_edges:
            forward = not forward
        return TopoDS.Edge_s(
            self.edge_shapes[edge.id].Oriented(TopAbs_FORWARD if forward else TopAbs_REVERSED)
        )

    def _add_pcurve(self, half_edge, face):
        shape = self.edge_shapes[half_edge.edge]
        trimmed = self._surface_trim(half_edge.curve)
        if trimmed is None:
            ShapeFix_Edge().FixAddPCurve(shape, face, False, self.precision)
            self.operations.append("project_3d_curve_to_surface_parameters")
        else:
            definition, support = trimmed
            if self._surface_geometry(support.surface) is not BRep_Tool.Surface_s(face):
                raise ValueError(f"FIN {half_edge.id} support differs from its face surface")
            curve = self._parameter_curve(support)
            a, b = sorted((definition.start_parameter, definition.end_parameter))
            curve.Segment(a, b)
            adaptor = BRepAdaptor_Curve(shape)
            first = adaptor.Value(adaptor.FirstParameter())
            surface = BRep_Tool.Surface_s(face)
            p1, p2 = (surface.Value(curve.Value(t).X(), curve.Value(t).Y()) for t in (a, b))
            if first.Distance(p2) < first.Distance(p1):
                curve.Reverse()
            knots = curve.Knots()
            BSplCLib.Reparametrize_s(adaptor.FirstParameter(), adaptor.LastParameter(), knots)
            curve.SetKnots(knots)
            builder = BRep_Builder()
            builder.UpdateEdge(shape, curve, face, BRep_Tool.Tolerance_s(shape))
            builder.Range(shape, face, adaptor.FirstParameter(), adaptor.LastParameter())
        BRep_Builder().SameParameter(shape, False)
        BRepLib.SameParameter_s(shape, self.precision)

    def _build_face(self, face):
        if not face.loops:
            raise ValueError(f"parametric face {face.id} requires explicit source trim loops")
        shape = TopoDS_Face()
        builder = BRep_Builder()
        builder.MakeFace(
            shape,
            self._surface_geometry(face.surface),
            max(self.precision, Precision.Confusion_s()),
        )
        for loop_id in face.loops:
            for half_edge_id in self.loops[loop_id].half_edges:
                self._add_pcurve(self.half_edges[half_edge_id], shape)
            wire = self._make_wire(loop_id)
            shift = ShapeFix_Wire(wire, shape, self.precision)
            if shift.FixShifted():
                self.operations.append("unwrap_periodic_surface_parameters")
            builder.Add(shape, shift.Wire())
        self.face_shapes[face.id] = shape

    def _finish_face(self, face):
        shape = TopoDS.Face_s(self.context.Apply(self.face_shapes[face.id]))
        operation = ShapeFix_Face(shape)
        operation.SetContext(self.context)
        operation.SetPrecision(self.precision)
        operation.SetMaxTolerance(max(self.precision, Precision.Confusion_s()))
        if operation.FixMissingSeam():
            self.operations.append("generate_periodic_surface_seam")
        if operation.FixOrientation():
            self.operations.append("orient_surface_boundary_wires")
        shape = operation.Face()
        for candidate in subshapes(shape, TopAbs_EDGE):
            edge = TopoDS.Edge_s(candidate)
            if not BRep_Tool.SameRange_s(edge):
                BRep_Builder().SameParameter(edge, False)
                BRepLib.SameRange_s(edge, max(1e-14, self.precision * 1e-4))
            BRepLib.SameParameter_s(edge, self.precision)
        self.face_shapes[face.id] = TopoDS.Face_s(
            self._orient_shape(shape, face.sense, self.surfaces[face.surface].sense)
        )

    def _validate_source_boundaries(self):
        # Sample the original FIN geometry, independently of the reparameterized
        # output pcurve. Keep source tolerance and representation error separate.
        exceeded = {}
        for edge in self.model.edges:
            curve = BRep_Tool.Curve_s(self.edge_shapes[edge.id], 0.0, 0.0)
            source_tolerance = (
                edge.tolerance or self.precision / (2 * self.geometry.scale)
            ) * self.geometry.scale
            approximation = self._edge_tolerance(edge)
            validation = self.geometry.options.validation.linear_threshold(0.0)
            budget = source_tolerance + approximation + validation
            vertices = [
                (
                    self.geometry.point(self.points[self.vertices[i].point].position),
                    BRep_Tool.Tolerance_s(self.vertex_shapes[i]),
                )
                for i in (edge.start_vertex, edge.end_vertex)
                if i is not None
            ]
            for half_edge_id in edge.half_edges:
                trimmed = self._surface_trim(self.half_edges[half_edge_id].curve)
                if trimmed is None:
                    continue
                definition, support = trimmed
                pc = self._parameter_curve(support)
                surface = self._surface_geometry(support.surface)
                for sample in range(33):
                    t = (
                        definition.start_parameter
                        + (definition.end_parameter - definition.start_parameter) * sample / 32
                    )
                    uv = pc.Value(t)
                    point = surface.Value(uv.X(), uv.Y())
                    _, distance = project_curve(curve, point)
                    # Trim endpoints and their vertex neighborhoods carry their
                    # own source tolerances, distinct from the edge interior.
                    limit = max(
                        [
                            budget,
                            *(
                                tol + approximation
                                for p, tol in vertices
                                if p.Distance(point) <= tol
                            ),
                        ]
                    )
                    if distance > limit:
                        raise ValueError(
                            f"FIN {half_edge_id} differs from shared edge {edge.id} "
                            f"by {distance:g} (conversion limit {limit:g})"
                        )
                    if distance > source_tolerance:
                        previous = exceeded.get(edge.id, (0.0, source_tolerance, limit))
                        exceeded[edge.id] = max(previous, (distance, source_tolerance, limit))
                    self.geometry_checks["source_uv_samples"] += 1
        if exceeded:
            edge_id = max(exceeded, key=lambda i: exceeded[i][0])
            distance, tolerance, limit = exceeded[edge_id]
            source = self.edges[edge_id].source
            self.geometry_checks["edges_exceeding_source_tolerance"] = len(exceeded)
            self.diagnostics.append(
                Diagnostic(
                    code="occt.parametric_approximation",
                    severity=DiagnosticSeverity.WARNING,
                    kind=DiagnosticKind.INVALID,
                    fatal=False,
                    message=(
                        f"{len(exceeded)} edges have sampled FIN-to-output differences "
                        "above their source edge tolerance; conversion error budgets passed"
                    ),
                    location=SourceLocation(byte_offset=source.byte_range.start),
                    node_type=source.node_type,
                    schema_key=self.model.schema_key,
                    details={
                        "edge_count": len(exceeded),
                        "max_deviation_edge_id": edge_id,
                        "max_deviation": distance,
                        "source_edge_tolerance": tolerance,
                        "conversion_limit": limit,
                        "samples_per_fin": 33,
                        "target_unit": self.geometry.options.target_unit,
                    },
                )
            )
        self.operations.append("validate_source_fin_geometry")

    def _record_boundaries(self):
        def record_many(kind, source, shapes, note=None):
            relation = ShapeRelationKind.SPLIT if len(shapes) > 1 else ShapeRelationKind.DIRECT
            for shape in shapes:
                self._record(kind, source.id, source.source, shape, relation, note)

        source_vertices = set()
        for vertex in self.model.vertices:
            shape = self.context.Apply(self.vertex_shapes[vertex.id])
            record_many(SourceEntityKind.VERTEX, vertex, [shape])
            point = self.points[vertex.point]
            record_many(SourceEntityKind.POINT, point, [shape])
            source_vertices.add(hash(shape))
        source_edges = set()
        mapped_edges = {}
        curve_targets = defaultdict(dict)
        for edge in self.model.edges:
            shapes = subshapes(self.context.Apply(self.edge_shapes[edge.id]), TopAbs_EDGE)
            if not shapes:
                raise ValueError(f"source edge {edge.id} disappeared during seam construction")
            mapped_edges[edge.id] = shapes
            source_edges.update(hash(shape) for shape in shapes)
            record_many(SourceEntityKind.EDGE, edge, shapes)
            curve_ids = {edge.curve} if edge.curve is not None else set()
            for half_edge_id in edge.half_edges:
                half_edge = self.half_edges[half_edge_id]
                record_many(SourceEntityKind.HALF_EDGE, half_edge, shapes)
                if half_edge.curve is not None:
                    curve_ids.add(half_edge.curve)
            pending = list(curve_ids)
            while pending:
                curve_id = pending.pop()
                curve_targets[curve_id].update((hash(shape), shape) for shape in shapes)
                definition = self.curves[curve_id].definition
                basis = (
                    definition.basis_curve
                    if isinstance(definition, TrimmedCurve)
                    else definition.parameter_curve
                    if isinstance(definition, SurfaceParametricCurve)
                    else None
                )
                if basis is not None and basis not in curve_ids:
                    curve_ids.add(basis)
                    pending.append(basis)
        for curve_id, targets in curve_targets.items():
            record_many(SourceEntityKind.CURVE, self.curves[curve_id], list(targets.values()))
        for face in self.model.faces:
            shape = self.face_shapes[face.id]
            record_many(SourceEntityKind.FACE, face, [shape])
            self._record_surface(self.surfaces[face.surface], shape)
            wires = subshapes(shape, TopAbs_WIRE)
            wire_edges = [{hash(edge) for edge in subshapes(wire, TopAbs_EDGE)} for wire in wires]
            loop_targets = []
            for loop_id in face.loops:
                expected = {
                    hash(edge)
                    for hid in self.loops[loop_id].half_edges
                    for edge in mapped_edges[self.half_edges[hid].edge]
                }
                candidates = [i for i, values in enumerate(wire_edges) if expected <= values]
                if len(candidates) != 1:
                    raise ValueError(
                        f"source loop {loop_id} cannot be mapped to its final OCCT wire"
                    )
                loop_targets.append((loop_id, candidates[0]))
            counts = Counter(index for _, index in loop_targets)
            for loop_id, index in loop_targets:
                loop = self.loops[loop_id]
                relation = (
                    ShapeRelationKind.MERGED if counts[index] > 1 else ShapeRelationKind.DIRECT
                )
                self._record(SourceEntityKind.LOOP, loop.id, loop.source, wires[index], relation)
            for kind, known in ((TopAbs_EDGE, source_edges), (TopAbs_VERTEX, source_vertices)):
                for generated in subshapes(shape, kind):
                    if hash(generated) not in known:
                        self._record(
                            SourceEntityKind.FACE,
                            face.id,
                            face.source,
                            generated,
                            ShapeRelationKind.GENERATED,
                            "periodic seam or split-boundary vertex",
                        )

    def _build_bodies(self):
        results = []
        for body in self.model.bodies:
            regions = [self.regions[i] for i in body.regions]
            material = [r for r in regions if r.kind is RegionKind.SOLID]
            if body.kind is BodyKind.SHEET:
                material = regions
            shell_shapes = {}
            solids = []
            for region in material:
                solid = BRepBuilderAPI_MakeSolid()
                for shell_id in region.shells:
                    shell = self.shells[shell_id]
                    faces = frozenset((*shell.back_faces, *shell.front_faces))
                    if not faces:
                        raise ValueError(f"shell {shell_id} has no boundary faces")
                    shape = TopoDS_Shell()
                    builder = BRep_Builder()
                    builder.MakeShell(shape)
                    for face_id in sorted(faces):
                        builder.Add(shape, self.face_shapes[face_id])
                    if body.kind is BodyKind.SOLID and not BRep_Tool.IsClosed_s(shape):
                        raise ValueError(f"shell {shell_id} is not closed")
                    if faces in shell_shapes:
                        raise ValueError("material regions repeat the same boundary shell")
                    shell_shapes[faces] = shape
                    solid.Add(shape)
                    if body.kind is BodyKind.SHEET:
                        solids.append((region.id, shape))
                if body.kind is BodyKind.SOLID:
                    if not solid.IsDone():
                        raise ValueError(f"region {region.id} solid construction failed")
                    region_shape = solid.Solid()
                    solids.append((region.id, region_shape))
                    self._record(SourceEntityKind.REGION, region.id, region.source, region_shape)
            body_shape = self._make_root(solids)
            for region in regions:
                for shell_id in region.shells:
                    shell = self.shells[shell_id]
                    key = frozenset((*shell.back_faces, *shell.front_faces))
                    if key not in shell_shapes:
                        raise ValueError(f"shell {shell_id} has no matching material boundary")
                    shape = shell_shapes[key]
                    self._record(
                        SourceEntityKind.SHELL,
                        shell.id,
                        shell.source,
                        shape,
                        ShapeRelationKind.MERGED,
                    )
                    if region not in material or body.kind is BodyKind.SHEET:
                        self._record(
                            SourceEntityKind.REGION,
                            region.id,
                            region.source,
                            shape,
                            ShapeRelationKind.SPLIT
                            if len(region.shells) > 1
                            else ShapeRelationKind.DIRECT,
                            "source region boundary",
                        )
            self._record(SourceEntityKind.BODY, body.id, body.source, body_shape)
            results.append((body.id, body_shape))
        self.operations.append("solid_with_source_region_shells")
        return results
