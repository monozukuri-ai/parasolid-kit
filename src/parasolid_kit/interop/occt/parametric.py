"""Shared trimmed topology with source UV curves and bounded intersection fits.

Imported only after the optional OCCT runtime has been resolved. Representation
operations construct pcurves, seams and parameter agreement; they do not sew,
move, or discard source faces. Every source boundary remains mapped.
"""

from __future__ import annotations

from collections import Counter, defaultdict
from itertools import pairwise
from math import floor, isfinite, sqrt

from OCP.BRep import BRep_Builder, BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Curve
from OCP.BRepBuilderAPI import (
    BRepBuilderAPI_MakeEdge,
    BRepBuilderAPI_MakeSolid,
    BRepBuilderAPI_MakeVertex,
)
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepLib import BRepLib
from OCP.BSplCLib import BSplCLib
from OCP.Precision import Precision
from OCP.ShapeBuild import ShapeBuild_ReShape
from OCP.ShapeExtend import ShapeExtend_WireData
from OCP.ShapeFix import ShapeFix_Edge, ShapeFix_Face, ShapeFix_Wire
from OCP.TopAbs import (
    TopAbs_EDGE,
    TopAbs_FACE,
    TopAbs_FORWARD,
    TopAbs_REVERSED,
    TopAbs_VERTEX,
    TopAbs_WIRE,
)
from OCP.TopExp import TopExp
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import (
    TopoDS,
    TopoDS_Compound,
    TopoDS_Face,
    TopoDS_Shell,
    TopoDS_Vertex,
    TopoDS_Wire,
)
from OCP.TopTools import TopTools_IndexedMapOfShape

from ...brep.geometry import (
    BlendBoundarySurface,
    BlendedEdgeSurface,
    ConeSurface,
    CylinderSurface,
    IntersectionCurve,
    LineCurve,
    NurbsCurve,
    NurbsSurface,
    OffsetSurface,
    PlaneSurface,
    SphereSurface,
    SpunSurface,
    SurfaceParametricCurve,
    SweptSurface,
    TorusSurface,
    TrimmedCurve,
)
from ...brep.topology import BodyKind, RegionKind, Sense, Vector3
from ...diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity, SourceLocation
from .intersection import (
    SupportSurface,
    fit_intersection,
    intersect_analytic,
    march_intersection,
    project_curve,
)
from .model import ShapeRelationKind, SourceEntityKind
from .topology import BuiltTopology, TopologyBuilder, _reason, _sense_sign


def subshapes(shape: object, kind: object) -> list:
    indexed = TopTools_IndexedMapOfShape()
    TopExp.MapShapes_s(shape, kind, indexed)
    return [indexed.FindKey(i) for i in range(1, indexed.Extent() + 1)]


def _xyz(vector):
    return (vector.x, vector.y, vector.z)


def _dot(a, b):
    return sum(x * y for x, y in zip(a, b, strict=True))


def _norm(a):
    return sqrt(_dot(a, a))


def _unit(a):
    length = _norm(a)
    if length <= 0.0:
        raise ValueError("direction has zero length")
    return tuple(x / length for x in a)


def _sphere_poles(sphere):
    centre, axis = _xyz(sphere.center), _unit(_xyz(sphere.axis))
    return tuple(
        tuple(c + sign * sphere.radius * a for c, a in zip(centre, axis, strict=True))
        for sign in (1.0, -1.0)
    )


def _cone_apex(cone):
    # The radius r + v tan(a) vanishes at v = -r cos(a) / sin(a) along the axis.
    point, axis = _xyz(cone.point), _unit(_xyz(cone.axis))
    v = -cone.radius * cone.cos_half_angle / cone.sin_half_angle
    return tuple(p + v * a for p, a in zip(point, axis, strict=True))


class ParametricTopologyBuilder(TopologyBuilder):
    """Two-pass face construction keeps both sides of each shared edge consistent."""

    def __init__(self, model, geometry):
        super().__init__(model, geometry)
        self.context = ShapeBuild_ReShape()
        resolutions = [
            b.linear_resolution * geometry.scale
            for b in model.bodies
            if b.linear_resolution is not None and b.linear_resolution > 0
        ]
        self.precision = min(resolutions, default=Precision.Confusion_s() / 10)
        if not isfinite(self.precision) or self.precision <= 0:
            raise ValueError("body linear resolution must be finite and positive")
        # The largest positional uncertainty the source declares on the body;
        # a wire fix may grow an edge or vertex tolerance up to it, never beyond.
        self.tolerance_cap = max(
            [
                self.precision,
                *(
                    item.tolerance * geometry.scale
                    for item in (*model.vertices, *model.edges)
                    if item.tolerance is not None
                    and isfinite(item.tolerance)
                    and item.tolerance > 0
                ),
            ]
        )
        self.curve_anchors = defaultdict(list)
        for curve in model.curves:
            d = curve.definition
            if isinstance(d, TrimmedCurve):
                self.curve_anchors[d.basis_curve].extend(
                    (geometry.point(d.start_point), geometry.point(d.end_point))
                )
        # The vertices of an edge that uses an intersection curve directly are
        # source points of that curve as well: a fitted branch passes through
        # them instead of missing them by what the fit leaves between chart points.
        positions = {point.id: point.position for point in model.points}
        vertex_points = {vertex.id: positions[vertex.point] for vertex in model.vertices}
        for edge in model.edges:
            if edge.curve is None or not isinstance(
                self.curves[edge.curve].definition, IntersectionCurve
            ):
                continue
            for vertex_id in (edge.start_vertex, edge.end_vertex):
                if vertex_id is not None:
                    self.curve_anchors[edge.curve].append(geometry.point(vertex_points[vertex_id]))
        # Approximated supports widen the acceptance of the curves built on them.
        self.surface_uncertainty: dict[int, float] = {}
        # Edges split at a surface singularity: the pieces along the curve.
        self.split_edges: dict[int, list] = {}
        # Ring edges built from a FIN's curve: whether each runs along its source edge.
        self.ring_edge_directions: dict[int, bool] = {}
        self.geometry_checks = Counter()
        # Curves built by approximation and the deviation each one may carry.
        self.approximated_curves: dict[int, float] = {}
        self.diagnostics = []

    def build(self) -> BuiltTopology:
        self._build_vertices()
        for edge in self.model.edges:
            try:
                self._build_edge(edge)
            except (KeyboardInterrupt, SystemExit):
                raise
            except Exception as error:
                if not self.partial:
                    raise
                self.failed_edges[edge.id] = _reason(error)
                self.edge_shapes.pop(edge.id, None)
                self.split_edges.pop(edge.id, None)
        for face in self.model.faces:
            failed_edge = self._failed_edge_of(face)
            if failed_edge is not None:
                self._drop_face(
                    face.id, f"edge {failed_edge} failed: {self.failed_edges[failed_edge]}"
                )
        # All face pcurves must exist before a periodic seam splits shared rings.
        for step in (self._build_face, self._finish_face, self._check_face):
            for face in self.model.faces:
                if face.id in self.dropped_faces:
                    continue
                try:
                    step(face)
                except (KeyboardInterrupt, SystemExit):
                    raise
                except Exception as error:
                    if not self.partial:
                        raise
                    self._drop_face(face.id, error)
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
            body_shapes=tuple(bodies),
            **self.partial_records(),
        )

    def _check_face(self, face):
        shape = TopoDS.Face_s(self.context.Apply(self.face_shapes[face.id]))
        if not BRepCheck_Analyzer(shape).IsValid():
            shape = self._cover_wire_crossings(shape)
            if not BRepCheck_Analyzer(shape).IsValid():
                if not self._sliver_orientation_only(shape):
                    raise ValueError(f"OCCT parametric face {face.id} is invalid")
                self.operations.append("accept_sliver_face_orientation")
        self.face_shapes[face.id] = shape

    def _sliver_orientation_only(self, shape):
        """Whether the only complaint is the orientation of a strip thinner than the cap.

        A face whose two long edges run within the body's declared tolerance
        of each other (a strip a hundredth of a millimetre wide and a hundred
        long, a tolerant source modelling artefact) has a signed area whose
        sign is numerical noise: the orientation fix and the kernel check
        sample it differently and disagree, while its wire closes, does not
        cross itself and is otherwise sound. Such a face is accepted; any
        other complaint still fails it.
        """
        from OCP.BRepCheck import BRepCheck_Status
        from OCP.BRepGProp import BRepGProp
        from OCP.GProp import GProp_GProps

        analyzer = BRepCheck_Analyzer(shape)
        statuses = set(analyzer.Result(shape).Status())
        if statuses != {BRepCheck_Status.BRepCheck_BadOrientationOfSubshape}:
            return False
        for kind in (TopAbs_WIRE, TopAbs_EDGE, TopAbs_VERTEX):
            for candidate in subshapes(shape, kind):
                if not analyzer.IsValid(candidate):
                    return False
        area, length = GProp_GProps(), GProp_GProps()
        BRepGProp.SurfaceProperties_s(shape, area)
        BRepGProp.LinearProperties_s(shape, length)
        perimeter = length.Mass()
        if perimeter <= 0.0:
            return False
        return 2.0 * abs(area.Mass()) / perimeter <= self.tolerance_cap

    def _guard_partial(self, error, face_ids):
        """Drop the faces in partial mode; re-raise otherwise."""
        if not self.partial:
            raise error
        for face_id in face_ids:
            if face_id not in self.dropped_faces:
                self._drop_face(face_id, error)

    def _faces_of_edge(self, edge):
        face_ids = []
        for half_edge_id in edge.half_edges:
            loop_id = self.half_edges[half_edge_id].loop
            if loop_id is not None:
                face_ids.append(self.loops[loop_id].face)
        return face_ids

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
            if vertex.id not in self.vertex_shapes:
                continue
            tolerance = vertex.tolerance or self.precision / self.geometry.scale
            if not isfinite(tolerance) or tolerance < 0:
                raise ValueError(f"vertex {vertex.id} has an invalid tolerance")
            builder.UpdateVertex(
                self.vertex_shapes[vertex.id],
                max(Precision.Confusion_s(), tolerance * self.geometry.scale),
            )

    def _singular_points(self, edge):
        """Singular points (sphere poles, cone apexes) of the surfaces of the faces on an edge."""
        points = []
        for half_edge_id in edge.half_edges:
            # A sheet body's free side has fins that belong to no loop.
            loop = self.loops.get(self.half_edges[half_edge_id].loop)
            face = None if loop is None else self.faces.get(loop.face)
            if face is None:
                continue
            definition = self.surfaces[face.surface].definition
            if isinstance(definition, SphereSurface):
                points.extend(_sphere_poles(definition))
            elif isinstance(definition, ConeSurface):
                points.append(_cone_apex(definition))
        return [self.geometry.point(Vector3(*point)) for point in points]

    def _split_at_singularities(self, edge, shape, tolerance):
        """Split an edge where its curve passes through a singular point of a face.

        No pcurve can follow a curve through a sphere pole or a cone apex: the
        surface parameter jumps there. A modeller may still write such an
        edge; SolidWorks cuts a sphere into two faces along a great circle
        through its poles, with or without vertices on the circle. The edge is
        split at every interior crossing, with a generated vertex at the
        singularity. The pieces keep the edge's curve, ``_edge_for_half_edge``
        walks them as one boundary, and the relations record the split.
        """
        curve = BRep_Tool.Curve_s(shape, 0.0, 0.0)
        first, last = BRep_Tool.Range_s(shape)
        closed = edge.start_vertex is None
        ends = [] if closed else [curve.Value(first), curve.Value(last)]
        crossings = []
        for point in self._singular_points(edge):
            parameter, distance = project_curve(curve, point)
            if distance > tolerance:
                continue
            if curve.IsPeriodic():
                parameter = first + (parameter - first) % curve.Period()
            if not closed and not first < parameter < last:
                continue
            location = curve.Value(parameter)
            if any(location.Distance(curve.Value(other)) <= tolerance for other in crossings):
                continue
            if any(location.Distance(end) <= tolerance for end in ends):
                continue
            crossings.append(parameter)
        if not crossings:
            return [shape]
        crossings.sort()
        builder = BRep_Builder()
        vertices = []
        for parameter in crossings:
            vertex = BRepBuilderAPI_MakeVertex(curve.Value(parameter)).Vertex()
            builder.UpdateVertex(vertex, max(Precision.Confusion_s(), tolerance))
            vertices.append(vertex)
        if closed:
            bounds = [*crossings, crossings[0] + (last - first)]
            bounding_vertices = [*vertices, vertices[0]]
        else:
            start, end = TopoDS_Vertex(), TopoDS_Vertex()
            TopExp.Vertices_s(shape, start, end)
            bounds = [first, *crossings, last]
            bounding_vertices = [start, *vertices, end]
        pieces = []
        for (a, b), (va, vb) in zip(pairwise(bounds), pairwise(bounding_vertices), strict=True):
            piece = BRepBuilderAPI_MakeEdge(curve, va, vb, a, b)
            if not piece.IsDone():
                raise ValueError(f"OCCT edge {edge.id} could not be split at a surface singularity")
            result = piece.Edge()
            builder.UpdateEdge(result, tolerance)
            if not BRepLib.BuildCurve3d_s(result, tolerance, MaxSegment=1000):
                raise ValueError(f"OCCT edge {edge.id} 3D approximation failed")
            pieces.append(result)
        self.operations.append("split_edge_at_surface_singularity")
        return pieces

    def _share_seam_pcurves(self, shape, face):
        """Give an edge on the seam of a periodic surface both seam pcurves.

        Two faces of one periodic surface may share an edge lying on the seam
        (SolidWorks cuts a sphere along the plane of its axis and x axis), and
        one face needs the pcurve at the start of the period while the other
        needs it at the end. An edge's pcurves are keyed by surface, so OCCT's
        seam representation is used instead: one pcurve for the forward edge
        and one for the reversed edge, as a Boolean split of a sphere leaves
        them. A face lies to the left of its boundary, so an edge running up a
        u seam bounds the face at the end of the period and one running down
        bounds it at the start; along a v seam the roles swap.
        """
        from OCP.gp import gp_Vec2d

        if BRep_Tool.IsClosed_s(shape, face):
            return
        surface = BRep_Tool.Surface_s(face)
        pcurve = BRep_Tool.CurveOnSurface_s(shape, face, 0.0, 0.0)
        if pcurve is None:
            return
        first, last = BRep_Tool.Range_s(shape)
        location = self._seam_location(pcurve, surface, first, last)
        if location is None:
            return
        seam_in_u, lower, upper, on_end, increasing = location
        period = upper - lower
        shift = gp_Vec2d(period, 0.0) if seam_in_u else gp_Vec2d(0.0, period)
        at_start = pcurve.Translated(shift.Reversed()) if on_end else pcurve
        at_end = pcurve if on_end else pcurve.Translated(shift)
        forward_at_end = increasing if seam_in_u else not increasing
        forward, reverse = (at_end, at_start) if forward_at_end else (at_start, at_end)
        BRep_Builder().UpdateEdge(
            shape, forward, reverse, surface, TopLoc_Location(), BRep_Tool.Tolerance_s(shape)
        )
        self.operations.append("share_seam_pcurves")

    def _seam_location(self, pcurve, surface, first, last):
        """Where a pcurve runs along a seam line of a periodic surface, if it does.

        Returns ``(seam_in_u, lower, upper, on_end, increasing)``: whether the
        seam is a u line, the period bounds, whether the pcurve sits at the
        end of the period, and whether its free coordinate increases with the
        parameter; ``None`` when the pcurve is not on a seam.
        """
        from OCP.GeomAdaptor import GeomAdaptor_Surface

        if not (surface.IsUPeriodic() or surface.IsVPeriodic()):
            return None
        samples = [pcurve.Value(first + (last - first) * k / 4) for k in range(5)]
        u1, u2, v1, v2 = surface.Bounds()
        adaptor = GeomAdaptor_Surface(surface)
        for seam_in_u, periodic, lower, upper in (
            (True, surface.IsUPeriodic(), u1, u2),
            (False, surface.IsVPeriodic(), v1, v2),
        ):
            if not periodic or not all(isfinite(value) for value in (lower, upper)):
                continue
            resolution = (
                adaptor.UResolution(self.precision)
                if seam_in_u
                else adaptor.VResolution(self.precision)
            )
            fixed = [sample.X() if seam_in_u else sample.Y() for sample in samples]
            on_start = all(abs(value - lower) <= resolution for value in fixed)
            on_end = all(abs(value - upper) <= resolution for value in fixed)
            if not (on_start or on_end):
                continue
            moving = [sample.Y() if seam_in_u else sample.X() for sample in samples]
            return seam_in_u, lower, upper, on_end, moving[-1] > moving[0]
        return None

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
        boundaries = [
            i
            for i in definition.surfaces
            if isinstance(self.surfaces[i].definition, BlendBoundarySurface)
        ]
        if boundaries:
            result = self._blend_contact_curve(
                definition, points, boundaries[0], self.curve_anchors[curve_id]
            )
            self.operations.append("blend_contact_curve")
            blend_id = self.surfaces[boundaries[0]].definition.blend_surface
            spine_id = self.surfaces[blend_id].definition.spine_curve
            self.approximated_curves[curve_id] = max(
                self.precision, self.geometry_checks.get("blend_contact_deviation", 0.0)
            ) + self.approximated_curves.get(spine_id, self.precision)
            self.curve_geometries[curve_id] = result
            return result
        surfaces = [self._surface_geometry(i) for i in definition.surfaces]
        analytic = all(
            isinstance(
                self.surfaces[i].definition,
                (PlaneSurface, CylinderSurface, ConeSurface, SphereSurface, TorusSurface),
            )
            for i in definition.surfaces
        )
        if analytic and any(
            isinstance(self.surfaces[i].definition, PlaneSurface) for i in definition.surfaces
        ):
            result = intersect_analytic(
                surfaces, points, max(self.precision, Precision.Confusion_s())
            )
            self.operations.append("source_identified_surface_intersection")
        else:
            periodic = [self._source_periodicity(i) for i in definition.surfaces]
            slack = [self._support_uncertainty(i) for i in definition.surfaces]
            cap = self.geometry.options.fin_deviation_cap
            report = {}
            anchors = self.curve_anchors[curve_id]
            result = fit_intersection(
                surfaces,
                points,
                anchors,
                self.precision,
                periodic=periodic,
                slack=slack,
                cap=cap,
                report=report,
            )
            extended = self._spine_extension(curve_id, result, points, surfaces, periodic, slack)
            if extended is not None:
                result = fit_intersection(
                    surfaces,
                    extended,
                    anchors,
                    self.precision,
                    periodic=periodic,
                    slack=slack,
                    cap=cap,
                    report=report,
                )
                self.operations.append("extend_blend_spine_past_chart")
            self.operations.append("intersection_curve_numerical_approximation")
            deviation = report.get("deviation", 0.0)
            self.approximated_curves[curve_id] = max(self.precision, deviation) + max(slack)
            if deviation > 0.0:
                self._accept_intersection_deviation(curve_id, deviation, cap)
        first, last = result.FirstParameter(), result.LastParameter()
        if not all(isfinite(value) and abs(value) < 1e99 for value in (first, last)):
            # OCCT bounds an infinite branch (a line, a hyperbola) by a huge
            # parameter. The pipe of a blend along such a spine, and projections
            # onto that pipe, need a finite range: the branch is trimmed to its
            # chart extent, extended by that extent on both sides.
            from OCP.Geom import Geom_TrimmedCurve

            parameters = [project_curve(result, point)[0] for point in points]
            a, b = min(parameters), max(parameters)
            margin = max(b - a, self.precision)
            result = Geom_TrimmedCurve(result, a - margin, b + margin, True, True)
            self.operations.append("bound_infinite_intersection_branch")
        self.curve_geometries[curve_id] = result
        return result

    def _blend_coverage_points(self, blend_id):
        """Source points the blend surface has to reach.

        The chart points of every intersection on the blend or on one of its
        boundary surfaces (its contact curves) and the vertices of the faces
        it carries.
        """
        boundaries = {
            s.id
            for s in self.model.surfaces
            if isinstance(s.definition, BlendBoundarySurface)
            and s.definition.blend_surface == blend_id
        }
        points = []
        for curve in self.model.curves:
            definition = curve.definition
            if isinstance(definition, IntersectionCurve) and (
                blend_id in definition.surfaces or boundaries.intersection(definition.surfaces)
            ):
                points.extend(
                    self.geometry.point(p)
                    for p in (
                        *definition.start_points,
                        *definition.chart_points,
                        *definition.end_points,
                    )
                )
        for face in self.model.faces:
            if face.surface != blend_id:
                continue
            for loop_id in face.loops:
                for half_edge_id in self.loops[loop_id].half_edges:
                    edge_id = self.half_edges[half_edge_id].edge
                    if edge_id is None:
                        continue
                    edge = self.edges[edge_id]
                    for vertex_id in (edge.start_vertex, edge.end_vertex):
                        if vertex_id is not None:
                            position = self.points[self.vertices[vertex_id].point].position
                            points.append(self.geometry.point(position))
        return points

    def _spine_extension(self, curve_id, curve, points, surfaces, periodic, slack):
        """The chart of a blend spine continued past its ends where the blend needs it.

        A blend stores the chart of its spine's own intersection, which can
        stop short of the blend face: on production parts the contact curves
        and the face's vertices run up to 18 mm past the last chart point.
        There the contact construction clamps to the end of the spine and the
        pipe does not reach the face. Both ends are continued along the
        intersection of the two supports until every such point projects onto
        the interior; the result is refitted through chart and continuation.
        """
        from OCP.gp import gp_Pnt, gp_Vec

        blends = [
            s.id
            for s in self.model.surfaces
            if isinstance(s.definition, BlendedEdgeSurface) and s.definition.spine_curve == curve_id
        ]
        if not blends or curve.IsPeriodic() or curve.IsClosed():
            return None
        coverage = [p for blend_id in blends for p in self._blend_coverage_points(blend_id)]
        if not coverage:
            return None
        first, last = curve.FirstParameter(), curve.LastParameter()
        at_end = 1e-9 * (last - first)
        reach = {first: 0.0, last: 0.0}
        # A coverage point can only reach past an end of the spine when that
        # end is its nearest sample of the spine; the exact projection is paid
        # for those alone (the chart points of every contact curve are tested).
        samples = [curve.Value(first + (last - first) * k / 64) for k in range(65)]
        for point in coverage:
            nearest = min(range(65), key=lambda k: point.SquareDistance(samples[k]))
            if nearest not in (0, 64):
                continue
            parameter, _ = project_curve(curve, point)
            for end in (first, last):
                if abs(parameter - end) <= at_end:
                    reach[end] = max(reach[end], point.Distance(curve.Value(end)))
        if max(reach.values()) <= self.precision:
            return None
        supports = [
            SupportSurface(surface, max(1e-14, self.precision * 1e-4), flags)
            for surface, flags in zip(surfaces, periodic, strict=True)
        ]
        spacings = sorted(a.Distance(b) for a, b in pairwise(points) if a.Distance(b) > 0)
        step = spacings[len(spacings) // 2] if spacings else max(reach.values()) / 8
        extended = list(points)
        for end, sign in ((last, 1.0), (first, -1.0)):
            if reach[end] <= self.precision:
                continue
            origin, derivative = gp_Pnt(), gp_Vec()
            curve.D1(end, origin, derivative)
            if derivative.Magnitude() <= 1e-300:
                raise ValueError(f"intersection {curve_id} has no tangent at its end")
            derivative.Normalize()
            heading = tuple(sign * x for x in derivative.Coord())
            length = reach[end] * 1.1 + self.precision
            marched = march_intersection(
                supports,
                origin,
                heading,
                length,
                min(step, length / 2),
                self.precision,
                tuple(slack),
            )
            if sign > 0:
                extended.extend(marched)
            else:
                extended = [*reversed(marched), *extended]
        return extended if len(extended) > len(points) else None

    def _contact_offsets(self, support_id, support_geometry, radius, spine_id):
        """The offset of a blend support that its ball centres lie on.

        The spine is cut from the offsets of the two supports, and an offset
        keeps the parameters of its basis (XT p. 71); the offset from this
        support therefore names the contact point on the support by the
        centre's parameters. When the source stores that offset as another
        surface, both signed offsets are built and the chart's first centre
        selects one.
        """
        from OCP.Geom import Geom_OffsetSurface

        numerical = max(1e-14, self.precision * 1e-3)
        periodic = self._source_periodicity(support_id)
        spine_definition = self.curves[spine_id].definition
        if isinstance(spine_definition, IntersectionCurve):
            for surface_id in spine_definition.surfaces:
                definition = self.surfaces[surface_id].definition
                if isinstance(definition, OffsetSurface) and definition.basis_surface == support_id:
                    return [SupportSurface(self._surface_geometry(surface_id), numerical, periodic)]
        return [
            SupportSurface(
                Geom_OffsetSurface(support_geometry, sign * radius, True), numerical, periodic
            )
            for sign in (1.0, -1.0)
        ]

    def _support_uncertainty(self, surface_id):
        """How far a support may sit from the source's own surface.

        Zero for a surface built exactly; for a rebuilt blend pipe the
        approximation its spine carries (the pipe's own error is a hundredth
        of the tolerance). The blend is built here so that its spine is known.
        """
        definition = self.surfaces[surface_id].definition
        # An offset keeps the uncertainty of its basis: the offsets a blend
        # spine is cut from may be offsets of other blends.
        while isinstance(definition, OffsetSurface):
            surface_id = definition.basis_surface
            definition = self.surfaces[surface_id].definition
        if isinstance(definition, BlendedEdgeSurface):
            self._surface_geometry(surface_id)
            return self.approximated_curves.get(definition.spine_curve, self.precision)
        return 0.0

    def _accept_intersection_deviation(self, curve_id, deviation, cap):
        source = self.curves[curve_id].source
        self.diagnostics.append(
            Diagnostic(
                code="occt.intersection_deviation",
                severity=DiagnosticSeverity.WARNING,
                kind=DiagnosticKind.INVALID,
                fatal=False,
                message=(
                    f"intersection {curve_id} meets its supports only within {deviation:g} "
                    f"(tolerance {self.precision:g}); accepted within the FIN deviation cap "
                    f"{cap:g}, the edges on it carry the deviation"
                ),
                location=SourceLocation(byte_offset=source.byte_range.start),
                node_type=source.node_type,
                schema_key=self.model.schema_key,
                details={
                    "curve_id": curve_id,
                    "deviation": deviation,
                    "tolerance": self.precision,
                    "fin_deviation_cap": cap,
                    "target_unit": self.geometry.options.target_unit,
                },
            )
        )
        self.operations.append("accept_intersection_deviation_within_cap")

    def _source_periodicity(self, surface_id):
        """The source's periodic flags of a surface, or of an offset's basis."""
        definition = self.surfaces[surface_id].definition
        if isinstance(definition, OffsetSurface):
            definition = self.surfaces[definition.basis_surface].definition
        if isinstance(definition, NurbsSurface):
            return (bool(definition.u_periodic), bool(definition.v_periodic))
        return (False, False)

    def _blend_contact_curve(self, definition, points, boundary_id, anchors=()):
        """The curve where a rolling-ball blend touches one support.

        A blend boundary surface is the construction surface whose
        intersection with a support is that contact curve; its support is
        ``surface[1 - boundary]`` of the blend (XT p. 65). The contact point
        for a spine point is the foot of the ball on the support, at the
        blend radius. The source CHART points fix the spine range; the curve
        is interpolated through contact points between them and must pass
        through every chart point within the precision. The trim points of
        the edges on the curve (``anchors``) are source points of it as well
        and join the chart: between chart points the construction follows a
        spine that is only within the precision of two offsets, which can
        put it a few hundredths of a millimetre along the support.
        """
        from OCP.GeomAPI import GeomAPI_Interpolate
        from OCP.gp import gp_Pnt
        from OCP.TColgp import TColgp_HArray1OfPnt

        boundary = self.surfaces[boundary_id].definition
        blend = self.surfaces[boundary.blend_surface].definition
        if not isinstance(blend, BlendedEdgeSurface):
            raise ValueError("blend boundary does not reference a blend surface")
        support_id = blend.supporting_surfaces[1 - boundary.boundary_index]
        support_definition = self.surfaces[support_id].definition
        spine = self._curve_geometry(blend.spine_curve)
        radius = abs(blend.ranges[0]) * self.geometry.scale
        # The spine is itself a fit: within the precision of two offsets, or
        # farther where the cap admitted a gap between them.
        spine_slack = self.approximated_curves.get(blend.spine_curve, self.precision)
        if isinstance(support_definition, BlendedEdgeSurface):
            # The support is itself a rolling-ball blend: the ball touches that
            # pipe on the line from its centre to the nearest point of the
            # pipe's spine, at the pipe radius from that point (a blend of
            # range zero is the cliff edge itself). The nearest point of the
            # approximated pipe surface misses the chart points by far more
            # than the precision.
            support = None
            support_spine = self._curve_geometry(support_definition.spine_curve)
            support_radius = abs(support_definition.ranges[0]) * self.geometry.scale
        else:
            support_geometry = self._surface_geometry(support_id)
            support = SupportSurface(
                support_geometry,
                max(1e-14, self.precision * 1e-3),
                self._source_periodicity(support_id),
            )
            offset_candidates = self._contact_offsets(
                support_id, support_geometry, radius, blend.spine_curve
            )
            support_spine = None
            support_radius = 0.0
        numerical = max(1e-14, self.precision * 1e-3)
        if anchors and not (spine.IsPeriodic() or spine.IsClosed()):
            ordered = sorted(
                ((project_curve(spine, p)[0], p) for p in (*points, *anchors)),
                key=lambda item: item[0],
            )
            merged = []
            for _, p in ordered:
                if not merged or p.Distance(merged[-1]) > numerical:
                    merged.append(p)
            if len(merged) > len(points):
                self.operations.append("blend_contact_curve_through_trim_points")
            points = merged
        parameters = [project_curve(spine, p)[0] for p in points]
        if spine.IsPeriodic() or spine.IsClosed():
            period = (
                spine.Period()
                if spine.IsPeriodic()
                else spine.LastParameter() - spine.FirstParameter()
            )
            for index in range(1, len(parameters)):
                step = parameters[index] - parameters[index - 1]
                parameters[index] = (
                    parameters[index - 1] + (step + period / 2) % period - period / 2
                )

        offset_support = None
        if support_spine is None:
            if len(offset_candidates) == 1:
                offset_support = offset_candidates[0]
            else:
                first_centre = spine.Value(parameters[0])
                offset_support = min(
                    offset_candidates,
                    key=lambda candidate: candidate.project(first_centre, self.precision)[2],
                )

        def contact(parameter):
            centre = spine.Value(parameter)
            if support_spine is not None:
                foot_parameter, distance = project_curve(support_spine, centre)
                if distance <= numerical:
                    raise ValueError("blend contact centre lies on the support spine")
                foot = support_spine.Value(foot_parameter)
                point = gp_Pnt(
                    *(
                        f + (c - f) * support_radius / distance
                        for f, c in zip(foot.Coord(), centre.Coord(), strict=True)
                    )
                )
                if abs(point.Distance(centre) - radius) > self.precision + spine_slack:
                    raise ValueError("blend contact point is not at the blend radius")
                return point
            # The centre lies on the offset of the support by the radius, which
            # shares the support's parameters: the contact is the support at
            # the centre's parameters there. The nearest point of the support
            # is not it where the support curves more tightly than the radius
            # (the offset's swallowtail) or where the contact runs past the
            # support's domain.
            _, _, residual = offset_support.project(centre, self.precision + spine_slack)
            if residual > self.precision + spine_slack:
                raise ValueError("blend contact centre is not at the blend radius")
            return support.value(*offset_support.seed)

        # The geometric contact is ill-conditioned along the surfaces when the
        # spines are approximations: a spine within e of its source places the
        # foot of the ball within about sqrt(2 d e) of the source contact, 1e-4
        # for a branch OCCT walked within 1e-8 and a ball 100 mm from the
        # support spine. The chart points are the source's own contact points,
        # so the geometric contact between two of them is corrected by the
        # residuals at both, interpolated along the span, and the curve passes
        # through every chart point.
        residuals = [
            tuple(p - g for p, g in zip(point.Coord(), contact(parameter).Coord(), strict=True))
            for point, parameter in zip(points, parameters, strict=True)
        ]
        drift = max(sum(r * r for r in residual) ** 0.5 for residual in residuals)
        # The correction interpolates the residuals between chart points, so
        # they must be small against the chart spacing; a residual of that
        # order means the spine or support is not the one the chart follows.
        # The typical spacing is the median: a chart can repeat a point within
        # a few nanometres of its neighbour, which says nothing about how far
        # the correction is interpolated elsewhere. A tenth of it separates a
        # spine that is merely ill-conditioned (two offsets meeting at a
        # shallow angle put the ball centre tens of micrometres along the
        # support) from a support the chart does not follow at all.
        spacings = sorted(a.Distance(b) for a, b in pairwise(points) if a.Distance(b) > numerical)
        spacing = spacings[len(spacings) // 2] if spacings else 0.0
        # A chart of a few micrometres (a blend that ends almost where it
        # starts) is still judged against the precision, not against itself.
        allowed_drift = max(0.1 * spacing, 10 * (self.precision + spine_slack), 0.01 * radius)
        if spacing and drift > allowed_drift:
            raise ValueError(
                f"blend contact drift {drift:g} exceeds a tenth of the chart spacing {spacing:g}"
            )
        samples = []

        def add(sample):
            if not samples or sample.Distance(samples[-1]) > numerical:
                samples.append(sample)

        spans = zip(pairwise(parameters), pairwise(residuals), strict=True)
        for index, ((a, b), (first, second)) in enumerate(spans):
            # Every chart point is a sample. Two chart points can share a spine
            # parameter (both project onto the end of the spine) and still be
            # micrometres apart; that span has no interior and only its chart
            # point is kept, instead of losing the point with the span.
            add(points[index])
            if abs(b - a) <= numerical:
                continue
            for k in range(1, 8):
                fraction = k / 8
                geometric = contact(a + (b - a) * fraction)
                add(
                    gp_Pnt(
                        *(
                            g + (1 - fraction) * x + fraction * y
                            for g, x, y in zip(geometric.Coord(), first, second, strict=True)
                        )
                    )
                )
        add(points[-1])
        closed = points[0].Distance(points[-1]) <= numerical
        if closed and len(samples) > 2 and samples[0].Distance(samples[-1]) <= numerical:
            samples.pop()
        if len(samples) < 2:
            raise ValueError("blend contact curve has fewer than two distinct points")
        array = TColgp_HArray1OfPnt(1, len(samples))
        for index, point in enumerate(samples, 1):
            array.SetValue(index, point)
        fit = GeomAPI_Interpolate(array, closed, numerical)
        fit.Perform()
        if not fit.IsDone():
            raise ValueError("blend contact curve interpolation failed")
        curve = fit.Curve()
        # The spine is itself an approximation within the precision and the
        # chart points carry the source resolution, so the contact curve may
        # differ from a chart point by both.
        worst = max(project_curve(curve, p)[1] for p in points)
        if worst > 4 * self.precision + spine_slack:
            raise ValueError(
                f"blend contact curve misses a source chart point by {worst:g} "
                f"(limit {4 * self.precision:g})"
            )
        self.geometry_checks["blend_contact_deviation"] = max(
            self.geometry_checks.get("blend_contact_deviation", 0.0), worst
        )
        self.geometry_checks["blend_contact_drift"] = max(
            self.geometry_checks.get("blend_contact_drift", 0.0), drift
        )
        return curve

    def _parameter_curve(self, definition):
        curve = self.curves[definition.parameter_curve].definition
        if not isinstance(curve, NurbsCurve):
            raise ValueError("surface parameter curve must reference NURBS")
        support = self.surfaces[definition.surface].definition
        # An offset surface keeps the parameters of its basis (XT p. 71).
        seen = set()
        while isinstance(support, OffsetSurface):
            if support.basis_surface in seen:
                raise ValueError("offset surface basis chain is circular")
            seen.add(support.basis_surface)
            support = self.surfaces[support.basis_surface].definition
        section = None
        if isinstance(support, SweptSurface):
            section = self.curves[support.section].definition
        elif isinstance(support, SpunSurface):
            section = self.curves[support.profile].definition
        return self.geometry.parameter_curve(curve, support, section=section)

    def _wrapped_trim(self, definition, curve, evaluated, tolerance):
        """A FIN's trim parameters (start, end), moved by whole periods into its curve's range.

        A trim of a closed basis may be stored a period away from the basis's
        own parameters: a ring edge's FIN runs from 0 to -1 over a curve whose
        parameters span [0, 1] and whose image closes, on a periodic surface
        or as a periodic B-curve. The OCCT curve is not periodic and continues
        its end polynomials there, so such a trim is moved into the range,
        where it is the same arc. A basis whose image does not close, or a
        trim that would straddle the end of the range, keeps its parameters.
        """
        start, end = definition.start_parameter, definition.end_parameter
        lo, hi = min(start, end), max(start, end)
        if self._within_range(curve, lo, hi):
            return start, end
        period = self._closed_period(curve, evaluated, tolerance)
        first, last = curve.FirstParameter(), curve.LastParameter()
        slack = 1e-9 * max(1.0, abs(last - first))
        if period is None or hi - lo > period + slack:
            return start, end
        shift = period * floor((lo - first) / period + 1e-9)
        if hi - shift > last + slack:
            return start, end
        self.operations.append("wrap_closed_fin_trim")
        return (
            min(max(start - shift, first), last),
            min(max(end - shift, first), last),
        )

    @staticmethod
    def _within_range(curve, lo, hi):
        """Whether ``[lo, hi]`` lies in the parameter range of ``curve``."""
        first, last = curve.FirstParameter(), curve.LastParameter()
        slack = 1e-9 * max(1.0, abs(last - first))
        return first - slack <= lo and hi <= last + slack

    @staticmethod
    def _closed_period(curve, evaluated, tolerance):
        """The parameter range of a FIN curve whose image closes on its surface, or None."""
        first, last = curve.FirstParameter(), curve.LastParameter()
        if not last > first:
            return None
        p, q = curve.Value(first), curve.Value(last)
        if evaluated.value(p.X(), p.Y()).Distance(evaluated.value(q.X(), q.Y())) > tolerance:
            return None
        return last - first

    @staticmethod
    def _trim_uv(curve, parameter, period):
        """The FIN curve's point at ``parameter``, whole periods on where it leaves a closed range.

        A trim that crosses the end of a closed basis (a FIN from 0.999 to
        1.144 of a curve spanning [0, 1] whose image closes) continues on the
        curve's start; the OCCT curve would continue its end polynomial.
        """
        if period is not None:
            first, last = curve.FirstParameter(), curve.LastParameter()
            slack = 1e-9 * max(1.0, period)
            if parameter < first - slack or parameter > last + slack:
                parameter = first + (parameter - first) % period
        return curve.Value(parameter)

    def _surface_trim(self, curve_id):
        definition = None if curve_id is None else self.curves[curve_id].definition
        if isinstance(definition, TrimmedCurve):
            basis = self.curves[definition.basis_curve].definition
            if isinstance(basis, SurfaceParametricCurve):
                return definition, basis
        return None

    def _build_edge(self, edge):
        tolerance = self._edge_tolerance(edge)
        if edge.curve is not None:
            # An edge on an approximated branch carries that approximation:
            # the branch is built first so that its deviation is known.
            basis = self.curves[edge.curve].definition
            geometry_id = basis.basis_curve if isinstance(basis, TrimmedCurve) else edge.curve
            if isinstance(self.curves[geometry_id].definition, IntersectionCurve):
                self._curve_geometry(geometry_id)
            if geometry_id in self.approximated_curves:
                tolerance = max(tolerance, 2 * self.approximated_curves[geometry_id])
        vs = None if edge.start_vertex is None else self.vertex_shapes[edge.start_vertex]
        ve = None if edge.end_vertex is None else self.vertex_shapes[edge.end_vertex]
        if (vs is None) != (ve is None):
            raise ValueError(f"edge {edge.id} has only one endpoint vertex")
        ps, pe = (None, None) if vs is None else (BRep_Tool.Pnt_s(vs), BRep_Tool.Pnt_s(ve))
        # Only an edge with its own 3D curve can be split at a surface singularity.
        splittable = False

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

        fin_trim = None
        blend_curve = None
        if edge.curve is None:
            if not edge.half_edges:
                raise ValueError(f"edge {edge.id} requires finite FIN geometry")
            fin_trim = self._fin_trim_outside_blends(edge)
            if fin_trim is None:
                if vs is None:
                    raise ValueError(f"edge {edge.id} requires finite FIN geometry")
                blend_curve = self._blend_intersection_curve(edge, vs, ve, tolerance)
        else:
            own = self._surface_trim(edge.curve)
            own_curve = own[1] if own is not None else self.curves[edge.curve].definition
            if isinstance(own_curve, SurfaceParametricCurve) and self._blend_parameters(own_curve):
                # The edge's own geometry lives in a blend's parameters, which
                # the rebuilt blend does not reproduce; the FIN on the adjacent
                # face carries the same curve in that face's parameters.
                if vs is None:
                    raise ValueError(f"edge {edge.id} on a blend parameter curve has no endpoints")
                fin_trim = self._fin_trim_outside_blends(edge)
                if fin_trim is None:
                    # Both faces are blends: the edge is their intersection.
                    blend_curve = self._blend_intersection_curve(edge, vs, ve, tolerance)
                else:
                    self.operations.append("blend_parameter_edge_from_adjacent_fin")
        if fin_trim is not None:
            definition, support, fin = fin_trim
            curve = self._parameter_curve(support)
            surface = self._surface_geometry(support.surface)
            evaluated = self._support_surface(support.surface)
            if definition.start_parameter == definition.end_parameter:
                raise ValueError(f"edge {edge.id} requires distinct UV trim parameters")
            a, b = self._wrapped_trim(definition, curve, evaluated, tolerance)
            period = self._closed_period(curve, evaluated, tolerance)
            q1, q2 = (
                evaluated.value(uv.X(), uv.Y())
                for uv in (self._trim_uv(curve, t, period) for t in (a, b))
            )
            for point, source in ((q1, definition.start_point), (q2, definition.end_point)):
                if point.Distance(self.geometry.point(source)) > tolerance:
                    raise ValueError(f"edge {edge.id} UV trim disagrees with its source point")
            # A trim stored against the parameter direction keeps its end
            # points; the edge range is the same interval, so the vertex at
            # the lower parameter comes first.
            if vs is None:
                # A ring edge without a curve of its own: the FIN closes.
                if q1.Distance(q2) > tolerance:
                    raise ValueError(f"edge {edge.id} without vertices has an open FIN curve")
                # Nothing else orients its FINs: a FIN's trimmed SP-curve runs
                # along the FIN, from its start parameter to its end parameter
                # (backwards when the start is the larger), and the edge is
                # built in increasing parameter. The edge therefore follows
                # the source edge direction when the FIN does both or neither.
                self.ring_edge_directions[edge.id] = (a < b) is (fin.sense is Sense.POSITIVE)
            else:
                endpoints(q1, q2) if a < b else endpoints(q2, q1)
            samples = [
                self._trim_uv(curve, min(a, b) + abs(b - a) * k / 32, period) for k in range(33)
            ]
            if self._within_range(curve, min(a, b), max(a, b)) and all(
                evaluated.within_bounds(uv.X(), uv.Y()) for uv in samples
            ):
                builder = (
                    BRepBuilderAPI_MakeEdge(curve, surface, min(a, b), max(a, b))
                    if vs is None
                    else BRepBuilderAPI_MakeEdge(curve, surface, vs, ve, min(a, b), max(a, b))
                )
            else:
                # The FIN runs past the knots of its B-surface, where OCCT
                # continues the polynomial and the source the tangent plane
                # (or the next period), or across the end of its closed
                # curve: the 3D curve is fitted through the source's points.
                # The pcurve, added by the face, then meets it only within the
                # edge tolerance that the same-parameter pass measures.
                fitted = self._fit_through([evaluated.value(uv.X(), uv.Y()) for uv in samples])
                builder = (
                    BRepBuilderAPI_MakeEdge(fitted)
                    if vs is None
                    else BRepBuilderAPI_MakeEdge(fitted, vs, ve)
                )
                self.operations.append("fin_curve_fitted_on_surface_extension")
            self.operations.append("surface_parametric_curve_3d_approximation")
        elif blend_curve is not None:
            builder = BRepBuilderAPI_MakeEdge(blend_curve, vs, ve)
            self.operations.append("blend_intersection_edge_walked")
        else:
            source = self.curves[edge.curve]
            definition = source.definition
            trimmed_parameter = self._surface_trim(edge.curve)
            if isinstance(definition, LineCurve):
                if vs is None:
                    raise ValueError(f"line edge {edge.id} requires endpoints")
                shape = self.geometry.make_line_edge(
                    definition,
                    vs,
                    ve,
                    self.points[self.vertices[edge.start_vertex].point].position,
                    self.points[self.vertices[edge.end_vertex].point].position,
                )
                builder = None
                splittable = True
            elif isinstance(definition, SurfaceParametricCurve) or trimmed_parameter:
                # An edge whose own geometry is a surface parameter curve, or a
                # trim of one: the 3D edge is the image of the 2D curve on its
                # surface over the stored interval.
                if trimmed_parameter:
                    trim, definition = trimmed_parameter
                    if trim.start_parameter == trim.end_parameter:
                        raise ValueError(f"edge {edge.id} requires distinct UV trim parameters")
                    curve = self._parameter_curve(definition)
                    surface = self._surface_geometry(definition.surface)
                    evaluated = self._support_surface(definition.surface)
                    start, end = self._wrapped_trim(trim, curve, evaluated, tolerance)
                    period = self._closed_period(curve, evaluated, tolerance)
                    a, b = sorted((start, end))
                    for parameter, source_point in (
                        (start, trim.start_point),
                        (end, trim.end_point),
                    ):
                        uv = self._trim_uv(curve, parameter, period)
                        point = evaluated.value(uv.X(), uv.Y())
                        if point.Distance(self.geometry.point(source_point)) > tolerance:
                            raise ValueError(
                                f"edge {edge.id} UV trim disagrees with its source point"
                            )
                else:
                    curve = self._parameter_curve(definition)
                    surface = self._surface_geometry(definition.surface)
                    a, b = curve.FirstParameter(), curve.LastParameter()
                    period = None
                evaluated = self._support_surface(definition.surface)
                q1, q2 = (
                    evaluated.value(uv.X(), uv.Y())
                    for uv in (self._trim_uv(curve, t, period) for t in (a, b))
                )
                if vs is None and q1.Distance(q2) > tolerance:
                    raise ValueError(f"edge {edge.id} on a surface parameter curve is open")
                if vs is not None:
                    endpoints(q1, q2)
                if self._within_range(curve, a, b):
                    builder = (
                        BRepBuilderAPI_MakeEdge(curve, surface, a, b)
                        if vs is None
                        else BRepBuilderAPI_MakeEdge(curve, surface, vs, ve, a, b)
                    )
                else:
                    # The trim crosses the end of its closed curve: the 3D
                    # curve is fitted through the source's points around it.
                    fitted = self._fit_through(
                        [
                            evaluated.value(uv.X(), uv.Y())
                            for uv in (
                                self._trim_uv(curve, a + (b - a) * k / 32, period)
                                for k in range(33)
                            )
                        ]
                    )
                    builder = (
                        BRepBuilderAPI_MakeEdge(fitted)
                        if vs is None
                        else BRepBuilderAPI_MakeEdge(fitted, vs, ve)
                    )
                    self.operations.append("sp_curve_fitted_across_its_closure")
                self.operations.append("surface_parametric_curve_3d_approximation")
            elif isinstance(definition, TrimmedCurve):
                basis = self.curves[definition.basis_curve]
                curve = self._curve_geometry(basis.id)
                splittable = True
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
                if curve.IsPeriodic():
                    # On a closed basis the trim is the arc that follows the
                    # source parameter direction from start to end.
                    if definition.start_parameter > definition.end_parameter:
                        a, b = b, a
                    if b <= a:
                        b += curve.Period()
                else:
                    a, b = sorted((a, b))
                endpoints(curve.Value(a), curve.Value(b))
                builder = (
                    BRepBuilderAPI_MakeEdge(curve, a, b)
                    if vs is None
                    else BRepBuilderAPI_MakeEdge(curve, vs, ve, a, b)
                )
            else:
                curve = self._curve_geometry(source.id)
                splittable = True
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
                    # A vertex on an approximated branch must enclose its
                    # distance to that branch for the edge to be built on it.
                    for vertex, distance in ((vs, da), (ve, db)):
                        if distance > BRep_Tool.Tolerance_s(vertex):
                            BRep_Builder().UpdateVertex(vertex, distance + Precision.Confusion_s())
                    if curve.IsPeriodic():
                        if source.sense is Sense.POSITIVE and b <= a:
                            b += curve.Period()
                        elif source.sense is Sense.NEGATIVE and a <= b:
                            a += curve.Period()
                    a, b = sorted((a, b))
                    endpoints(curve.Value(a), curve.Value(b))
                    builder = BRepBuilderAPI_MakeEdge(curve, vs, ve, a, b)
        if builder is not None:
            if not builder.IsDone():
                raise ValueError(f"OCCT edge {edge.id} construction failed: {builder.Error()}")
            shape = builder.Edge()
            BRep_Builder().UpdateEdge(shape, tolerance)
            if not BRepLib.BuildCurve3d_s(shape, tolerance, MaxSegment=1000):
                raise ValueError(f"OCCT edge {edge.id} 3D approximation failed")
        pieces = self._split_at_singularities(edge, shape, tolerance) if splittable else [shape]
        if len(pieces) == 1:
            self.edge_shapes[edge.id] = shape
            return
        compound = TopoDS_Compound()
        builder = BRep_Builder()
        builder.MakeCompound(compound)
        for piece in pieces:
            builder.Add(compound, piece)
        self.split_edges[edge.id] = pieces
        self.edge_shapes[edge.id] = compound

    def _half_edge_forward(self, half_edge):
        """Whether the half-edge runs along the parameter of its built edge."""
        edge = self.edges[half_edge.edge]
        if half_edge.vertex is None:
            if edge.curve is not None:
                forward = half_edge.sense is self.curves[edge.curve].sense
            elif edge.id in self.ring_edge_directions:
                # A ring edge built from one FIN's curve: the half-edge runs
                # along it when it agrees with the source edge as the built
                # edge does.
                forward = (half_edge.sense is Sense.POSITIVE) is self.ring_edge_directions[edge.id]
            else:
                raise ValueError(f"half-edge {half_edge.id} has no orientation reference")
        elif half_edge.vertex == edge.end_vertex:
            forward = True
        elif half_edge.vertex == edge.start_vertex:
            forward = False
        else:
            raise ValueError(f"half-edge {half_edge.id} does not end at its edge vertex")
        if edge.id in self.reversed_edges:
            forward = not forward
        return forward

    def _edge_for_half_edge(self, half_edge):
        edge = self.edges[half_edge.edge]
        forward = self._half_edge_forward(half_edge)
        orientation = TopAbs_FORWARD if forward else TopAbs_REVERSED
        pieces = self.split_edges.get(edge.id)
        if pieces is None:
            return TopoDS.Edge_s(self.edge_shapes[edge.id].Oriented(orientation))
        # The pieces follow the curve; a half-edge against it walks them backwards.
        wire = TopoDS_Wire()
        builder = BRep_Builder()
        builder.MakeWire(wire)
        for piece in pieces if forward else reversed(pieces):
            builder.Add(wire, piece.Oriented(orientation))
        return wire

    def _make_wire(self, loop_id):
        """The loop's wire from its oriented edges, in source order.

        BRepBuilderAPI_MakeWire merges vertices that lie within each other's
        tolerance and replaces the edges it joins there by copies carrying the
        merged vertex. An edge shorter than its vertex tolerance (a tolerant
        source sliver) is joined that way in one face only; the neighbouring
        face keeps the original and the shell no longer closes. The source
        loop's edges already share their vertex shapes, so the wire is
        assembled directly; the wire fixes order and connect it afterwards.
        """
        loop = self.loops[loop_id]
        wire = TopoDS_Wire()
        builder = BRep_Builder()
        builder.MakeWire(wire)
        for half_edge_id in loop.half_edges:
            half_edge = self.half_edges[half_edge_id]
            target = self._edge_for_half_edge(half_edge)
            if target.ShapeType() == TopAbs_EDGE:
                builder.Add(wire, target)
            else:
                # The pieces of a split edge arrive as a wire of their own.
                for piece in subshapes(target, TopAbs_EDGE):
                    builder.Add(wire, piece)
            self.half_edge_shapes[half_edge.id] = target
            self._record(SourceEntityKind.HALF_EDGE, half_edge.id, half_edge.source, target)
        self.loop_shapes[loop.id] = wire
        self._record(SourceEntityKind.LOOP, loop.id, loop.source, wire)
        return wire

    def _support_surface(self, surface_id):
        """The surface evaluated as the source does (linear past a B-surface's domain)."""
        return SupportSurface(
            self._surface_geometry(surface_id),
            max(1e-14, self.precision * 1e-4),
            self._source_periodicity(surface_id),
        )

    def _fit_through(self, points):
        """A C2 B-spline through ordered points, within the numerical tolerance."""
        from OCP.GeomAPI import GeomAPI_Interpolate
        from OCP.TColgp import TColgp_HArray1OfPnt

        numerical = max(1e-14, self.precision * 1e-4)
        kept = [points[0]]
        for point in points[1:]:
            if point.Distance(kept[-1]) > numerical:
                kept.append(point)
        if len(kept) < 2:
            raise ValueError("curve samples do not span a curve")
        array = TColgp_HArray1OfPnt(1, len(kept))
        for index, point in enumerate(kept, 1):
            array.SetValue(index, point)
        fit = GeomAPI_Interpolate(array, False, numerical)
        fit.Perform()
        if not fit.IsDone():
            raise ValueError("curve interpolation through surface samples failed")
        return fit.Curve()

    def _blend_intersection_curve(self, edge, vs, ve, tolerance):
        """The 3D curve of an edge known only in its two blends' parameters.

        An edge between two blend faces can have no curve of its own, its
        FINs being SP-curves in the blends' parameters, which the rebuilt
        canal surfaces do not reproduce. The curve is the intersection of the
        two canal surfaces: it is walked from the start vertex along their
        common tangent up to the end vertex and interpolated through the
        points found.
        """
        from OCP.GeomAPI import GeomAPI_Interpolate
        from OCP.TColgp import TColgp_HArray1OfPnt

        faces = {face.id: face for face in self.model.faces}
        surface_ids = []
        for half_edge_id in edge.half_edges:
            half_edge = self.half_edges[half_edge_id]
            if half_edge.loop is None:
                continue
            surface_id = faces[self.loops[half_edge.loop].face].surface
            if (
                isinstance(self.surfaces[surface_id].definition, BlendedEdgeSurface)
                and surface_id not in surface_ids
            ):
                surface_ids.append(surface_id)
        if len(surface_ids) != 2:
            raise ValueError(f"edge {edge.id} requires trimmed surface-parametric FIN curves")
        numerical = max(1e-14, self.precision * 1e-4)
        supports = [
            SupportSurface(self._surface_geometry(i), numerical, self._source_periodicity(i))
            for i in surface_ids
        ]
        start, end = BRep_Tool.Pnt_s(vs), BRep_Tool.Pnt_s(ve)
        length = start.Distance(end)
        if length <= tolerance:
            raise ValueError(f"edge {edge.id} between two blends has coincident vertices")
        (a1, a2, a3), (b1, b2, b3) = (support.project(start, tolerance)[1] for support in supports)
        heading = (a2 * b3 - a3 * b2, a3 * b1 - a1 * b3, a1 * b2 - a2 * b1)
        magnitude = sum(x * x for x in heading) ** 0.5
        if magnitude <= 1e-2:
            # Two blends (nearly) tangent to each other meet along the ball's
            # cross-section: the edge is that circular arc, which the
            # intersection of two such surfaces cannot locate.
            arc = self._blend_section_arc(surface_ids, start, end, tolerance)
            if arc is not None:
                return arc
            if magnitude <= 1e-6:
                raise ValueError(f"edge {edge.id} lies where its two blends are tangent")
        towards = tuple((e - s) / length for s, e in zip(start.Coord(), end.Coord(), strict=True))
        sign = 1.0 if sum(h * t for h, t in zip(heading, towards, strict=True)) >= 0 else -1.0
        heading = tuple(sign * h / magnitude for h in heading)
        step = max(length / 16, 10 * tolerance)
        slack = tuple(self._support_uncertainty(i) for i in surface_ids)
        chain = [start]
        walk = march_intersection(supports, start, heading, 1.5 * length, step, tolerance, slack)
        for point in walk:
            beyond = zip(point.Coord(), end.Coord(), towards, strict=True)
            if sum((p - e) * t for p, e, t in beyond) > 0 or point.Distance(end) <= step / 2:
                break
            chain.append(point)
        if chain[-1].Distance(end) > 2 * step:
            raise ValueError(f"edge {edge.id} walked along its blends misses its end vertex")
        chain.append(end)
        array = TColgp_HArray1OfPnt(1, len(chain))
        for index, point in enumerate(chain, 1):
            array.SetValue(index, point)
        fit = GeomAPI_Interpolate(array, False, numerical)
        fit.Perform()
        if not fit.IsDone():
            raise ValueError(f"edge {edge.id} interpolation along its blends failed")
        return fit.Curve()

    def _blend_section_arc(self, surface_ids, start, end, tolerance):
        """The arc of a blend's rolling ball through both vertices.

        Where a blend meets another blend tangent to it (consecutive fillets
        of one radius), their common edge is a cross-section of the rolling
        ball: the circle of the blend radius about a spine point, in the
        plane normal to the spine there. The spine point is the one at the
        radius from both vertices; the arc between them that lies on both
        blends is returned, or None when no spine has such a point.
        """
        from OCP.GC import GC_MakeArcOfCircle
        from OCP.gp import gp_Pnt, gp_Vec

        numerical = max(1e-14, self.precision * 1e-4)
        supports = [self._support_surface(i) for i in surface_ids]
        slack = [self._support_uncertainty(i) for i in surface_ids]
        for surface_id in surface_ids:
            definition = self.surfaces[surface_id].definition
            spine = self._curve_geometry(definition.spine_curve)
            radius = abs(definition.ranges[0]) * self.geometry.scale
            allowance = (
                tolerance
                + self.approximated_curves.get(definition.spine_curve, 0.0)
                + max(BRep_Tool.Tolerance_s(v) for v in self._vertex_pair(start, end))
            )
            first, last = spine.FirstParameter(), spine.LastParameter()

            def misfit(parameter, spine=spine, radius=radius):
                centre = spine.Value(parameter)
                return max(abs(centre.Distance(start) - radius), abs(centre.Distance(end) - radius))

            # Coarse search along the spine, then a golden-section refinement.
            samples = [first + (last - first) * k / 400 for k in range(401)]
            best = min(samples, key=misfit)
            lower = max(first, best - (last - first) / 400)
            upper = min(last, best + (last - first) / 400)
            golden = (5**0.5 - 1) / 2
            for _ in range(60):
                c = upper - golden * (upper - lower)
                d = lower + golden * (upper - lower)
                if misfit(c) < misfit(d):
                    upper = d
                else:
                    lower = c
            best = (lower + upper) / 2
            if misfit(best) > allowance:
                continue
            centre, tangent = gp_Pnt(), gp_Vec()
            spine.D1(best, centre, tangent)
            if tangent.Magnitude() <= 1e-300:
                continue
            # Both vertices lie on the circle: try the arc through the middle
            # of the short way round, then the long way, against both blends.
            for long_way in (False, True):
                middle = gp_Pnt((start.XYZ() + end.XYZ()) / 2)
                radial = gp_Vec(centre, middle)
                if long_way:
                    radial.Reverse()
                if radial.Magnitude() <= numerical:
                    continue
                radial.Normalize()
                arc = GC_MakeArcOfCircle(start, gp_Pnt(centre.XYZ() + radial.XYZ() * radius), end)
                if not arc.IsDone():
                    continue
                curve = arc.Value()
                probe = curve.Value((curve.FirstParameter() + curve.LastParameter()) / 2)
                if all(
                    support.project(probe, tolerance)[2] <= tolerance + extra
                    for support, extra in zip(supports, slack, strict=True)
                ):
                    self.operations.append("blend_section_arc_edge")
                    return curve
        return None

    def _vertex_pair(self, start, end):
        """The vertex shapes at two points, for their tolerances."""
        return [
            shape
            for shape in self.vertex_shapes.values()
            if BRep_Tool.Pnt_s(shape).Distance(start) <= BRep_Tool.Tolerance_s(shape)
            or BRep_Tool.Pnt_s(shape).Distance(end) <= BRep_Tool.Tolerance_s(shape)
        ] or [next(iter(self.vertex_shapes.values()))]

    def _fin_trim_outside_blends(self, edge):
        """The first FIN trim of the edge whose SP-curve is not in a blend's parameters.

        Returns (trim, SP-curve, half-edge), or None.
        """
        for half_edge_id in edge.half_edges:
            half_edge = self.half_edges[half_edge_id]
            candidate = self._surface_trim(half_edge.curve)
            if candidate is not None and not self._blend_parameters(candidate[1]):
                return (*candidate, half_edge)
        return None

    def _blend_parameters(self, support):
        """Whether a FIN's SP-curve lives in a blend's own parameters.

        A blend surface is rebuilt as the canal surface around its spine; its
        parameters are not the source's, so such a FIN cannot be mapped and
        the edge's 3D curve is projected instead (the source validation of
        that FIN is skipped for the same reason).
        """
        definition = self.surfaces[support.surface].definition
        return isinstance(definition, (BlendedEdgeSurface, BlendBoundarySurface))

    def _add_pcurve(self, half_edge, face):
        trimmed = self._surface_trim(half_edge.curve)
        if trimmed is not None and self._blend_parameters(trimmed[1]):
            trimmed = None
            self.operations.append("project_blend_fin_curve")
        pieces = self.split_edges.get(half_edge.edge)
        if trimmed is None:
            for shape in pieces or [self.edge_shapes[half_edge.edge]]:
                ShapeFix_Edge().FixAddPCurve(shape, face, False, self.precision)
                self._share_seam_pcurves(shape, face)
                self._same_parameter(shape)
            self.operations.append("project_3d_curve_to_surface_parameters")
            return
        if pieces is not None and self.edges[half_edge.edge].start_vertex is None:
            # The pieces of a closed edge start at a singularity, not where
            # the FIN trim starts, so its parameters cannot be mapped onto them.
            raise ValueError(
                f"closed edge {half_edge.edge} is split at a surface singularity "
                "and carries FIN geometry"
            )
        shapes = pieces or [self.edge_shapes[half_edge.edge]]
        definition, support = trimmed
        surface = BRep_Tool.Surface_s(face)
        if self._surface_geometry(support.surface) is not surface:
            raise ValueError(f"FIN {half_edge.id} support differs from its face surface")
        curve = self._parameter_curve(support)
        evaluated = self._support_surface(support.surface)
        start, end = self._wrapped_trim(
            definition, curve, evaluated, self._edge_tolerance(self.edges[half_edge.edge])
        )
        a, b = sorted((start, end))
        within = self._within_range(curve, a, b)
        if within:
            curve.Segment(a, b)
        if not within or not all(
            evaluated.within_bounds(
                curve.Value(a + (b - a) * k / 32).X(), curve.Value(a + (b - a) * k / 32).Y()
            )
            for k in range(33)
        ):
            # The FIN leaves the knots of its B-surface (into the source's
            # linear extension or its next period), where the OCCT surface
            # differs from the source, or crosses the end of its closed curve,
            # where no single interval of the OCCT pcurve describes it; the
            # edge's 3D curve, which followed the source, is projected onto
            # the surface instead.
            for shape in shapes:
                ShapeFix_Edge().FixAddPCurve(shape, face, False, self.precision)
                self._share_seam_pcurves(shape, face)
                self._same_parameter(shape)
            self.operations.append("project_fin_curve_outside_knots")
            return
        # The trim parameters are mapped linearly onto the edge's own range,
        # which spans every piece of a split edge; the same-parameter check
        # and the source FIN validation measure what that map leaves.
        first_parameter = BRep_Tool.Range_s(shapes[0])[0]
        last_parameter = BRep_Tool.Range_s(shapes[-1])[1]
        if self.edges[half_edge.edge].start_vertex is None:
            # A ring edge's ends coincide and cannot tell the direction: the
            # segment runs along the FIN when its trim rises (the FIN's curve
            # runs from its start parameter to its end), and the FIN along
            # the built edge when the half-edge is forward.
            if (start < end) is not self._half_edge_forward(half_edge):
                curve.Reverse()
        else:
            first = BRepAdaptor_Curve(shapes[0]).Value(first_parameter)
            p1, p2 = (evaluated.value(curve.Value(t).X(), curve.Value(t).Y()) for t in (a, b))
            if first.Distance(p2) < first.Distance(p1):
                curve.Reverse()
        knots = curve.Knots()
        BSplCLib.Reparametrize_s(first_parameter, last_parameter, knots)
        curve.SetKnots(knots)
        builder = BRep_Builder()
        for shape in shapes:
            piece_first, piece_last = BRep_Tool.Range_s(shape)
            pcurve = curve
            if len(shapes) > 1:
                pcurve = curve.Copy()
                pcurve.Segment(piece_first, piece_last)
            if BRep_Tool.IsClosed_s(shape, face) and self._seam_location(
                pcurve, surface, piece_first, piece_last
            ):
                # The face across this seam already installed both seam
                # pcurves, and this FIN lies on the same seam line.
                pass
            else:
                builder.UpdateEdge(shape, pcurve, face, BRep_Tool.Tolerance_s(shape))
                builder.Range(shape, face, piece_first, piece_last)
                self._share_seam_pcurves(shape, face)
            self._same_parameter(shape)

    def _same_parameter(self, edge):
        """Re-measure the edge's representations without lowering its tolerance.

        BRepLib::SameParameter sets the edge tolerance to the deviation it
        measures, even below the tolerance the source declares; a ring edge
        whose circle and pcurves agree to 1e-7 then drops from the declared
        1e-5, and the planar face it bounds with a second ring fails the
        orientation check. The declared tolerance is kept as the floor.
        """
        declared = BRep_Tool.Tolerance_s(edge)
        vertices = [TopoDS.Vertex_s(candidate) for candidate in subshapes(edge, TopAbs_VERTEX)]
        recorded = [BRep_Tool.Tolerance_s(vertex) for vertex in vertices]
        measured = self._reparametrize(edge, self.precision)
        # Approx_SameParameter keeps the pcurve it was given when the
        # reparametrization it finds misses its target. A FIN mapped linearly
        # onto an edge whose parameter runs at another speed (the 3D curve
        # follows the other face's FIN) then stays a few tenths of a
        # millimetre off along the edge while its image is within
        # micrometres. A looser target installs the reparametrized curve, and
        # the strict target is reached from it. The target only rises, a
        # decade at a time, so the rescue ends after at most nine attempts.
        target = self.precision
        rescued = False
        while measured > max(declared, 1e4 * self.precision) and target < 1e5 * self.precision:
            target *= 10
            if self._reparametrize(edge, target) < measured:
                measured = self._reparametrize(edge, self.precision)
                rescued = True
        if rescued:
            # The first pass grew the vertices to the deviation of the pcurve
            # it then discarded. BRep_Builder only grows a tolerance; the
            # vertex record itself takes the value the installed pcurve needs.
            for vertex, before in zip(vertices, recorded, strict=True):
                vertex.TShape().Tolerance(max(before, measured))
            self.operations.append("reparametrize_pcurve_through_looser_target")
        BRep_Builder().UpdateEdge(edge, max(declared, measured))

    @staticmethod
    def _reparametrize(edge, target):
        """Re-measure the edge's pcurves against ``target``; the tolerance reached."""
        BRep_Builder().SameParameter(edge, False)
        BRepLib.SameParameter_s(edge, target)
        return BRep_Tool.Tolerance_s(edge)

    def _build_face(self, face):
        loops = self._trim_loops(face)
        if not loops:
            raise ValueError(f"parametric face {face.id} requires explicit source trim loops")
        shape = TopoDS_Face()
        builder = BRep_Builder()
        builder.MakeFace(
            shape,
            self._surface_geometry(face.surface),
            max(self.precision, Precision.Confusion_s()),
        )
        # Source loops run counterclockwise about the face normal, while the
        # seam and orientation fixes assume the surface normal. A face that
        # opposes its surface keeps its source region on a periodic surface
        # only when its wires are reversed here; _finish_face then reverses
        # the whole face, so the final boundary direction is unchanged.
        opposed = _sense_sign(face.sense) * _sense_sign(self.surfaces[face.surface].sense) < 0
        if opposed:
            self.operations.append("reverse_opposed_face_loops")
        for loop_id in loops:
            for half_edge_id in self.loops[loop_id].half_edges:
                self._add_pcurve(self.half_edges[half_edge_id], shape)
            wire = self._make_wire(loop_id)
            if opposed:
                # Reversed before the fixes: an edge on a seam carries one
                # pcurve per orientation, and the degenerated edges must be
                # attached for the orientation the face finally stores.
                data = ShapeExtend_WireData(wire)
                data.Reverse()
                wire = data.Wire()
            shift = ShapeFix_Wire(wire, shape, self.precision)
            if shift.FixShifted():
                self.operations.append("unwrap_periodic_surface_parameters")
            # A loop through a sphere pole or a cone apex needs the degenerated
            # edge of that singularity; the seam fix alone leaves it unorientable.
            if shift.FixDegenerated():
                self.operations.append("insert_degenerated_singularity_edges")
            builder.Add(shape, shift.Wire())
        self.face_shapes[face.id] = shape

    def _finish_face(self, face):
        shape = self._order_wire_edges(TopoDS.Face_s(self.context.Apply(self.face_shapes[face.id])))
        operation = ShapeFix_Face(shape)
        operation.SetContext(self.context)
        operation.SetPrecision(self.precision)
        operation.SetMaxTolerance(max(self.precision, Precision.Confusion_s()))
        if operation.FixMissingSeam():
            self.operations.append("generate_periodic_surface_seam")
        split = self._split_self_touching_loops(operation)
        if split is not None:
            operation = ShapeFix_Face(split)
            operation.SetContext(self.context)
            operation.SetPrecision(self.precision)
            operation.SetMaxTolerance(max(self.precision, Precision.Confusion_s()))
        if operation.FixOrientation():
            self.operations.append("orient_surface_boundary_wires")
        shape = operation.Face()
        if shape.IsNull():
            # The seam fix can hand back its single face inside a shell.
            faces = subshapes(operation.Result(), TopAbs_FACE)
            if len(faces) != 1:
                raise ValueError(f"OCCT face {face.id} was split into {len(faces)} faces")
            shape = TopoDS.Face_s(faces[0])
            self.operations.append("unwrap_single_face_fix_result")
        if BRep_Tool.Surface_s(shape).DynamicType().Name() == "Geom_RectangularTrimmedSurface":
            # The seam fix trims the surface to the face and keys the pcurves
            # of the edges it split or made on that copy. The other face of a
            # sphere split along one circle shares the original surface and
            # looks its pcurves up by that handle, so each is copied there.
            original = self._surface_geometry(face.surface)
            for candidate in subshapes(shape, TopAbs_EDGE):
                edge = TopoDS.Edge_s(candidate)
                if BRep_Tool.CurveOnSurface_s(edge, original, TopLoc_Location(), 0.0, 0.0) is None:
                    pcurve = BRep_Tool.CurveOnSurface_s(edge, shape, 0.0, 0.0)
                    if pcurve is not None:
                        BRep_Builder().UpdateEdge(
                            edge, pcurve, original, TopLoc_Location(), BRep_Tool.Tolerance_s(edge)
                        )
        for candidate in subshapes(shape, TopAbs_EDGE):
            edge = TopoDS.Edge_s(candidate)
            if not BRep_Tool.SameRange_s(edge):
                BRep_Builder().SameParameter(edge, False)
                BRepLib.SameRange_s(edge, max(1e-14, self.precision * 1e-4))
            # The seam and orientation fixes can replace a pcurve after the edge
            # was marked same-parameter; the flag is reset so the deviation of
            # every representation is measured again and the edge tolerance
            # covers it.
            self._same_parameter(edge)
        self.face_shapes[face.id] = TopoDS.Face_s(
            self._orient_shape(shape, face.sense, self.surfaces[face.surface].sense)
        )

    def _split_self_touching_loops(self, operation):
        """Split a loop that passes through one vertex twice into simple wires.

        A sliver face thinner than its vertex tolerance (a tolerant source
        loop of two spikes on a strip, every edge paired with another between
        the same vertices) is one source loop that visits its junction
        vertices twice. OCCT's wire traversal of such a loop is ambiguous at
        those vertices and depends on the direction: the face passes forward
        and fails reversed with its two-dimensional closure. ShapeFix splits
        the loop at the repeated vertices into the simple wires it is made
        of, which bound the same region. Returns the rebuilt face, or None.
        """
        from OCP.TopTools import TopTools_SequenceOfShape

        shape = TopoDS.Face_s(operation.Face())
        tool = operation.FixWireTool()
        rebuilt = TopoDS.Face_s(shape.EmptyCopied())
        builder = BRep_Builder()
        split = False
        for candidate in subshapes(shape, TopAbs_WIRE):
            wire = TopoDS.Wire_s(candidate)
            tool.Load(wire)
            tool.SetFace(shape)
            tool.SetPrecision(self.precision)
            pieces = TopTools_SequenceOfShape()
            if operation.FixLoopWire(pieces) and pieces.Length() > 1:
                split = True
                for index in range(1, pieces.Length() + 1):
                    builder.Add(rebuilt, pieces.Value(index))
            else:
                builder.Add(rebuilt, wire)
        if not split:
            return None
        if len(subshapes(rebuilt, TopAbs_EDGE)) != len(subshapes(shape, TopAbs_EDGE)):
            raise ValueError("splitting a self-touching loop changed the boundary edges")
        self.operations.append("split_self_touching_loops")
        self.context.Replace(shape, rebuilt)
        return rebuilt

    def _order_wire_edges(self, shape):
        """Store the edges of every wire in traversal order before the orientation fix.

        The seam fix of a periodic face replaces an edge it splits by the two
        pieces in that edge's own direction, in every face that shares it. A
        face running the edge backwards then holds the pieces against its
        traversal. ShapeFix judges a single boundary wire by the signed area
        of its edges in stored order (ShapeAnalysis::TotCross2D); the swapped
        pair makes the area of a correctly oriented loop negative and the
        orientation fix would reverse it. Only the storage order changes.
        """
        for candidate in subshapes(shape, TopAbs_WIRE):
            wire = TopoDS.Wire_s(candidate)
            repair = ShapeFix_Wire(wire, shape, self.precision)
            if not repair.FixReorder():
                continue
            fixed = repair.Wire()
            if len(subshapes(fixed, TopAbs_EDGE)) != len(subshapes(wire, TopAbs_EDGE)):
                raise ValueError("wire edge reordering changed the boundary edges")
            self.operations.append("order_wire_edges_after_seam_split")
            if not fixed.IsSame(wire):
                self.context.Replace(wire, fixed)
                shape = TopoDS.Face_s(self.context.Apply(shape))
        return shape

    def _cover_wire_crossings(self, shape):
        """Grow edge tolerances over a crossing of two edges just before their vertex.

        Two edges of a sliver face (a fillet a tenth of a millimetre wide meeting
        a ruling at about one degree) can cross in parameter space just before
        their shared vertex, farther out than its tolerance. ShapeFix covers such
        a crossing by growing the edge tolerances, which it caps at the precision
        by default; here the cap is the largest tolerance the source declares on
        the body. This runs after every face is finished: making an edge
        same-parameter resets its tolerance to the measured deviation, which
        would undo the growth.
        """
        for candidate in subshapes(shape, TopAbs_WIRE):
            wire = TopoDS.Wire_s(candidate)
            repair = ShapeFix_Wire(wire, shape, self.precision)
            repair.SetMaxTolerance(self.tolerance_cap)
            if not repair.FixSelfIntersection():
                continue
            self.operations.append("fix_wire_self_intersection")
            fixed = repair.Wire()
            if not fixed.IsSame(wire):
                self.context.Replace(wire, fixed)
                shape = TopoDS.Face_s(self.context.Apply(shape))
        return shape

    def _validate_source_boundaries(self):
        # Sample the original FIN geometry, independently of the reparameterized
        # output pcurve. Keep source tolerance and representation error separate.
        exceeded = {}
        tolerant = {}
        for edge in self.model.edges:
            if edge.id in self.failed_edges:
                continue
            try:
                self._validate_edge_boundaries(edge, exceeded, tolerant)
            except (KeyboardInterrupt, SystemExit):
                raise
            except Exception as error:
                self._guard_partial(error, self._faces_of_edge(edge))
        if tolerant:
            edge_id = max(tolerant, key=lambda i: tolerant[i][0])
            distance, limit, cap = tolerant[edge_id]
            source = self.edges[edge_id].source
            self.geometry_checks["edges_exceeding_conversion_budget"] = len(tolerant)
            self.diagnostics.append(
                Diagnostic(
                    code="occt.source_fin_deviation",
                    severity=DiagnosticSeverity.WARNING,
                    kind=DiagnosticKind.INVALID,
                    fatal=False,
                    message=(
                        f"{len(tolerant)} edges have sampled FIN geometry beyond their declared "
                        "tolerance and the conversion budget; accepted within the FIN deviation "
                        "cap, the output edge tolerance covers the deviation"
                    ),
                    location=SourceLocation(byte_offset=source.byte_range.start),
                    node_type=source.node_type,
                    schema_key=self.model.schema_key,
                    details={
                        "edge_count": len(tolerant),
                        "max_deviation_edge_id": edge_id,
                        "max_deviation": distance,
                        "conversion_limit": limit,
                        "fin_deviation_cap": cap,
                        "samples_per_fin": 33,
                        "target_unit": self.geometry.options.target_unit,
                    },
                )
            )
            self.operations.append("accept_source_fin_deviation_within_cap")
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

    def _validate_edge_boundaries(self, edge, exceeded, tolerant):
        curves = [
            BRep_Tool.Curve_s(shape, 0.0, 0.0)
            for shape in self.split_edges.get(edge.id) or [self.edge_shapes[edge.id]]
        ]
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
            half_edge = self.half_edges[half_edge_id]
            face_id = None if half_edge.loop is None else self.loops[half_edge.loop].face
            if face_id in self.dropped_faces:
                continue
            try:
                trimmed = self._surface_trim(self.half_edges[half_edge_id].curve)
                if trimmed is None or self._blend_parameters(trimmed[1]):
                    continue
                definition, support = trimmed
                pc = self._parameter_curve(support)
                # Evaluated as the source does: a FIN that runs past the
                # domain of a B-surface follows its linear extension.
                surface = SupportSurface(
                    self._surface_geometry(support.surface),
                    max(1e-14, self.precision * 1e-4),
                    self._source_periodicity(support.surface),
                )
                start, end = self._wrapped_trim(definition, pc, surface, approximation)
                period = self._closed_period(pc, surface, approximation)
                for sample in range(33):
                    t = start + (end - start) * sample / 32
                    uv = self._trim_uv(pc, t, period)
                    point = surface.value(uv.X(), uv.Y())
                    distance = min(project_curve(curve, point)[1] for curve in curves)
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
                        cap = self.geometry.options.fin_deviation_cap
                        if cap is None or distance > cap:
                            raise ValueError(
                                f"FIN {half_edge_id} differs from shared edge {edge.id} "
                                f"by {distance:g} (conversion limit {limit:g})"
                            )
                        # The source's own FIN leaves its declared tolerance.
                        # The output edge is same-parameter with that FIN's
                        # pcurve, so its tolerance already covers the deviation;
                        # the diagnostic reports it.
                        previous = tolerant.get(edge.id, (0.0, limit, cap))
                        tolerant[edge.id] = max(previous, (distance, limit, cap))
                    elif distance > source_tolerance:
                        previous = exceeded.get(edge.id, (0.0, source_tolerance, limit))
                        exceeded[edge.id] = max(previous, (distance, source_tolerance, limit))
                    self.geometry_checks["source_uv_samples"] += 1
            except (KeyboardInterrupt, SystemExit):
                raise
            except Exception as error:
                # A FIN that does not follow its edge drops only the face it
                # belongs to; the edge and its other face stay.
                self._guard_partial(error, [] if face_id is None else [face_id])

    def _record_boundaries(self):
        def record_many(kind, source, shapes, note=None):
            relation = ShapeRelationKind.SPLIT if len(shapes) > 1 else ShapeRelationKind.DIRECT
            for shape in shapes:
                self._record(kind, source.id, source.source, shape, relation, note)

        source_vertices = set()
        for vertex in self.model.vertices:
            if vertex.id in self.isolated_vertices:
                continue
            shape = self.context.Apply(self.vertex_shapes[vertex.id])
            record_many(SourceEntityKind.VERTEX, vertex, [shape])
            point = self.points[vertex.point]
            record_many(SourceEntityKind.POINT, point, [shape])
            source_vertices.add(hash(shape))
        source_edges = set()
        mapped_edges = {}
        curve_targets = defaultdict(dict)
        for edge in self.model.edges:
            if edge.id in self.failed_edges:
                continue
            shapes = subshapes(self.context.Apply(self.edge_shapes[edge.id]), TopAbs_EDGE)
            if not shapes:
                if self.partial:
                    self.failed_edges[edge.id] = "edge disappeared during seam construction"
                    self._guard_partial(ValueError("edge disappeared"), self._faces_of_edge(edge))
                    continue
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
            if face.id in self.dropped_faces:
                continue
            relation_count = len(self.relations)
            try:
                shape = self.face_shapes[face.id]
                record_many(SourceEntityKind.FACE, face, [shape])
                self._record_surface(self.surfaces[face.surface], shape)
                wires = subshapes(shape, TopAbs_WIRE)
                wire_edges = [
                    {hash(edge) for edge in subshapes(wire, TopAbs_EDGE)} for wire in wires
                ]
                loop_targets = []
                for loop_id in self._trim_loops(face):
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
                    self._record(
                        SourceEntityKind.LOOP, loop.id, loop.source, wires[index], relation
                    )
                for kind, known in ((TopAbs_EDGE, source_edges), (TopAbs_VERTEX, source_vertices)):
                    for generated in subshapes(shape, kind):
                        if hash(generated) not in known:
                            self._record(
                                SourceEntityKind.FACE,
                                face.id,
                                face.source,
                                generated,
                                ShapeRelationKind.GENERATED,
                                "periodic seam, split-boundary or surface-singularity vertex",
                            )
            except (KeyboardInterrupt, SystemExit):
                raise
            except Exception as error:
                del self.relations[relation_count:]
                self._guard_partial(error, [face.id])

    def _applied(self, shape):
        return self.context.Apply(shape)

    def _connection_tolerance(self):
        return self.precision

    def _build_bodies(self):
        results = []
        for body in self.model.bodies:
            if body.kind is BodyKind.WIRE:
                shape = self._build_wire_body(body)
                if shape is not None:
                    results.append((body.id, shape))
                continue
            regions = [self.regions[i] for i in body.regions]
            material = [r for r in regions if r.kind is RegionKind.SOLID]
            if body.kind is BodyKind.SHEET:
                material = regions
            shell_shapes = {}
            solids = []
            open_body = False
            for region in material:
                region_shells = []
                for shell_id in region.shells:
                    shell = self.shells[shell_id]
                    faces = frozenset((*shell.back_faces, *shell.front_faces))
                    if not faces:
                        raise ValueError(f"shell {shell_id} has no boundary faces")
                    available = [i for i in sorted(faces) if i in self.face_shapes]
                    if not available:
                        if not self.partial:
                            raise ValueError(f"shell {shell_id} has no boundary faces")
                        open_body = True
                        continue
                    shape = TopoDS_Shell()
                    builder = BRep_Builder()
                    builder.MakeShell(shape)
                    for face_id in available:
                        builder.Add(shape, self.face_shapes[face_id])
                    if body.kind is BodyKind.SOLID and (
                        len(available) < len(faces) or not BRep_Tool.IsClosed_s(shape)
                    ):
                        if not self.partial:
                            raise ValueError(f"shell {shell_id} is not closed")
                        open_body = True
                    if faces in shell_shapes:
                        raise ValueError("material regions repeat the same boundary shell")
                    shell_shapes[faces] = shape
                    region_shells.append(shape)
                    if body.kind is BodyKind.SHEET:
                        solids.append((region.id, shape))
                if body.kind is BodyKind.SOLID and region_shells:
                    if open_body:
                        # A solid region with a dropped face comes back as its
                        # open shells, never as a solid.
                        region_shape = self._make_root([(region.id, s) for s in region_shells])
                    else:
                        solid = BRepBuilderAPI_MakeSolid()
                        for shape in region_shells:
                            solid.Add(shape)
                        if not solid.IsDone():
                            raise ValueError(f"region {region.id} solid construction failed")
                        region_shape = solid.Solid()
                    solids.append((region.id, region_shape))
                    self._record(SourceEntityKind.REGION, region.id, region.source, region_shape)
            if not solids:
                self.failed_bodies[body.id] = "every boundary face was dropped"
                continue
            if open_body:
                self.partial_bodies.append(body.id)
                self.operations.append("partial_body_open_shell")
            body_shape = self._make_root(solids)
            for region in regions:
                for shell_id in region.shells:
                    shell = self.shells[shell_id]
                    key = frozenset((*shell.back_faces, *shell.front_faces))
                    if key not in shell_shapes:
                        if self.partial:
                            continue
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
