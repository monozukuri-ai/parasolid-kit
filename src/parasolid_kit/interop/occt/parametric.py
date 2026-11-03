"""Shared trimmed topology with source UV curves and bounded intersection fits.

Imported only after the optional OCCT runtime has been resolved. Representation
operations construct pcurves, seams and parameter agreement; they do not sew,
move, or discard source faces. Every source boundary remains mapped.
"""

from __future__ import annotations

from collections import Counter, defaultdict
from itertools import pairwise
from math import isfinite, sqrt

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
from .intersection import fit_intersection, intersect_analytic, project_curve
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
        # Edges split at a surface singularity: the pieces along the curve.
        self.split_edges: dict[int, list] = {}
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
                raise ValueError(f"OCCT parametric face {face.id} is invalid")
        self.face_shapes[face.id] = shape

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
            result = self._blend_contact_curve(definition, points, boundaries[0])
            self.operations.append("blend_contact_curve")
            self.approximated_curves[curve_id] = max(
                self.precision, self.geometry_checks.get("blend_contact_deviation", 0.0)
            )
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
            result = fit_intersection(
                surfaces, points, self.curve_anchors[curve_id], self.precision
            )
            self.operations.append("intersection_curve_numerical_approximation")
            self.approximated_curves[curve_id] = self.precision
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

    def _blend_contact_curve(self, definition, points, boundary_id):
        """The curve where a rolling-ball blend touches one support.

        A blend boundary surface is the construction surface whose
        intersection with a support is that contact curve; its support is
        ``surface[1 - boundary]`` of the blend (XT p. 65). The contact point
        for a spine point is the foot of the ball on the support, at the
        blend radius. The source CHART points fix the spine range; the curve
        is interpolated through contact points between them and must pass
        through every chart point within the precision.
        """
        from OCP.GeomAPI import GeomAPI_Interpolate, GeomAPI_ProjectPointOnSurf
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
            support = self._surface_geometry(support_id)
            support_spine = None
            support_radius = 0.0
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
        numerical = max(1e-14, self.precision * 1e-3)

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
            else:
                projection = GeomAPI_ProjectPointOnSurf(centre, support, numerical)
                if not projection.NbPoints():
                    raise ValueError("blend contact projection failed")
                point = projection.NearestPoint()
            if abs(point.Distance(centre) - radius) > self.precision:
                raise ValueError("blend contact point is not at the blend radius")
            return point

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
        spacing = min(
            (a.Distance(b) for a, b in pairwise(points) if a.Distance(b) > numerical),
            default=0.0,
        )
        if spacing and drift > 0.01 * spacing:
            raise ValueError(
                f"blend contact drift {drift:g} exceeds one percent of the chart spacing "
                f"{spacing:g}"
            )
        samples = []
        for (a, b), (first, second) in zip(pairwise(parameters), pairwise(residuals), strict=True):
            if abs(b - a) <= numerical:
                continue
            for k in range(8):
                fraction = k / 8
                geometric = contact(a + (b - a) * fraction)
                sample = gp_Pnt(
                    *(
                        g + (1 - fraction) * x + fraction * y
                        for g, x, y in zip(geometric.Coord(), first, second, strict=True)
                    )
                )
                if not samples or sample.Distance(samples[-1]) > numerical:
                    samples.append(sample)
        last = points[-1]
        if not samples or last.Distance(samples[-1]) > numerical:
            samples.append(last)
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
        if worst > 4 * self.precision:
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
            if a == b:
                raise ValueError(f"edge {edge.id} requires distinct UV trim parameters")
            q1, q2 = (surface.Value(curve.Value(t).X(), curve.Value(t).Y()) for t in (a, b))
            for point, source in ((q1, definition.start_point), (q2, definition.end_point)):
                if point.Distance(self.geometry.point(source)) > tolerance:
                    raise ValueError(f"edge {edge.id} UV trim disagrees with its source point")
            # A trim stored against the parameter direction keeps its end
            # points; the edge range is the same interval, so the vertex at
            # the lower parameter comes first.
            endpoints(q1, q2) if a < b else endpoints(q2, q1)
            builder = BRepBuilderAPI_MakeEdge(curve, surface, vs, ve, min(a, b), max(a, b))
            self.operations.append("surface_parametric_curve_3d_approximation")
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
                    a, b = sorted((trim.start_parameter, trim.end_parameter))
                    if a == b:
                        raise ValueError(f"edge {edge.id} requires distinct UV trim parameters")
                    curve = self._parameter_curve(definition)
                    surface = self._surface_geometry(definition.surface)
                    for parameter, source_point in (
                        (trim.start_parameter, trim.start_point),
                        (trim.end_parameter, trim.end_point),
                    ):
                        uv = curve.Value(parameter)
                        point = surface.Value(uv.X(), uv.Y())
                        if point.Distance(self.geometry.point(source_point)) > tolerance:
                            raise ValueError(
                                f"edge {edge.id} UV trim disagrees with its source point"
                            )
                else:
                    curve = self._parameter_curve(definition)
                    surface = self._surface_geometry(definition.surface)
                    a, b = curve.FirstParameter(), curve.LastParameter()
                q1, q2 = (surface.Value(curve.Value(t).X(), curve.Value(t).Y()) for t in (a, b))
                if vs is None:
                    if q1.Distance(q2) > tolerance:
                        raise ValueError(f"edge {edge.id} on a surface parameter curve is open")
                    builder = BRepBuilderAPI_MakeEdge(curve, surface, a, b)
                else:
                    endpoints(q1, q2)
                    builder = BRepBuilderAPI_MakeEdge(curve, surface, vs, ve, a, b)
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

    def _add_pcurve(self, half_edge, face):
        trimmed = self._surface_trim(half_edge.curve)
        pieces = self.split_edges.get(half_edge.edge)
        if trimmed is None:
            for shape in pieces or [self.edge_shapes[half_edge.edge]]:
                ShapeFix_Edge().FixAddPCurve(shape, face, False, self.precision)
                self._share_seam_pcurves(shape, face)
                BRep_Builder().SameParameter(shape, False)
                BRepLib.SameParameter_s(shape, self.precision)
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
        a, b = sorted((definition.start_parameter, definition.end_parameter))
        curve.Segment(a, b)
        # The trim parameters are mapped linearly onto the edge's own range,
        # which spans every piece of a split edge; the same-parameter check
        # and the source FIN validation measure what that map leaves.
        first_parameter = BRep_Tool.Range_s(shapes[0])[0]
        last_parameter = BRep_Tool.Range_s(shapes[-1])[1]
        first = BRepAdaptor_Curve(shapes[0]).Value(first_parameter)
        p1, p2 = (surface.Value(curve.Value(t).X(), curve.Value(t).Y()) for t in (a, b))
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
            BRep_Builder().SameParameter(shape, False)
            BRepLib.SameParameter_s(shape, self.precision)

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
            BRep_Builder().SameParameter(edge, False)
            BRepLib.SameParameter_s(edge, self.precision)
        self.face_shapes[face.id] = TopoDS.Face_s(
            self._orient_shape(shape, face.sense, self.surfaces[face.surface].sense)
        )

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
        for edge in self.model.edges:
            if edge.id in self.failed_edges:
                continue
            try:
                self._validate_edge_boundaries(edge, exceeded)
            except (KeyboardInterrupt, SystemExit):
                raise
            except Exception as error:
                self._guard_partial(error, self._faces_of_edge(edge))
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

    def _validate_edge_boundaries(self, edge, exceeded):
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
                        raise ValueError(
                            f"FIN {half_edge_id} differs from shared edge {edge.id} "
                            f"by {distance:g} (conversion limit {limit:g})"
                        )
                    if distance > source_tolerance:
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

    def _build_bodies(self):
        results = []
        for body in self.model.bodies:
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
