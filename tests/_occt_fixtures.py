"""Small exact BrepModel fixtures used by optional OCCT integration tests."""

from __future__ import annotations

from dataclasses import dataclass
from math import cosh, pi, sinh

from parasolid_kit import (
    Body,
    BodyKind,
    BoundingBox,
    BrepMetrics,
    BrepModel,
    ByteRange,
    CircleCurve,
    ConeSurface,
    CurveGeometry,
    CurveKind,
    CylinderSurface,
    Edge,
    EllipseCurve,
    Face,
    HalfEdge,
    HyperbolaCurve,
    LineCurve,
    Loop,
    NurbsCurve,
    NurbsSurface,
    OffsetSurface,
    ParabolaCurve,
    PlaneSurface,
    PointGeometry,
    Region,
    RegionKind,
    Sense,
    Shell,
    SourceNodeRef,
    SphereSurface,
    SurfaceGeometry,
    SurfaceKind,
    TopologyValidation,
    TorusSurface,
    TrimmedCurve,
    Vector3,
    Vertex,
)


@dataclass
class _Sources:
    index: int = 0

    def next(self, type_name: str) -> SourceNodeRef:
        self.index += 1
        return SourceNodeRef(
            node_index=self.index,
            node_type=10 + self.index,
            type_name=type_name,
            node_id=self.index,
            byte_range=ByteRange(self.index * 10, self.index * 10 + 9),
        )


def make_box_model(
    length: float = 40.0,
    width: float = 30.0,
    height: float = 20.0,
    *,
    _origin: tuple[float, float, float] = (0.0, 0.0, 0.0),
    _id_offset: int = 0,
    _source_offset: int = 0,
) -> BrepModel:
    def item_id(local_id: int) -> int:
        return _id_offset + local_id

    x_origin, y_origin, z_origin = _origin
    body_id = item_id(1)
    region_id = item_id(1)
    shell_id = item_id(1)
    sources = _Sources(_source_offset)
    body_source = sources.next("body")
    region_source = sources.next("region")
    shell_source = sources.next("shell")
    positions = {
        item_id(1): (x_origin, y_origin, z_origin),
        item_id(2): (x_origin + length, y_origin, z_origin),
        item_id(3): (x_origin + length, y_origin + width, z_origin),
        item_id(4): (x_origin, y_origin + width, z_origin),
        item_id(5): (x_origin, y_origin, z_origin + height),
        item_id(6): (x_origin + length, y_origin, z_origin + height),
        item_id(7): (x_origin + length, y_origin + width, z_origin + height),
        item_id(8): (x_origin, y_origin + width, z_origin + height),
    }
    point_sources = {item_id: sources.next("point") for item_id in positions}
    vertex_sources = {item_id: sources.next("vertex") for item_id in positions}
    points = tuple(
        PointGeometry(
            id=item_id,
            position=Vector3(*position),
            owner=vertex_sources[item_id],
            source=point_sources[item_id],
        )
        for item_id, position in positions.items()
    )
    vertices = tuple(
        Vertex(
            id=item_id,
            point=item_id,
            tolerance=None,
            owner=body_source,
            source=vertex_sources[item_id],
        )
        for item_id in positions
    )

    face_cycles = (
        (
            item_id(1),
            tuple(item_id(value) for value in (1, 4, 3, 2)),
            (0.0, 0.0, -1.0),
            (1.0, 0.0, 0.0),
        ),
        (
            item_id(2),
            tuple(item_id(value) for value in (5, 6, 7, 8)),
            (0.0, 0.0, 1.0),
            (1.0, 0.0, 0.0),
        ),
        (
            item_id(3),
            tuple(item_id(value) for value in (1, 2, 6, 5)),
            (0.0, -1.0, 0.0),
            (1.0, 0.0, 0.0),
        ),
        (
            item_id(4),
            tuple(item_id(value) for value in (4, 8, 7, 3)),
            (0.0, 1.0, 0.0),
            (1.0, 0.0, 0.0),
        ),
        (
            item_id(5),
            tuple(item_id(value) for value in (1, 5, 8, 4)),
            (-1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
        ),
        (
            item_id(6),
            tuple(item_id(value) for value in (2, 3, 7, 6)),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
        ),
    )
    edge_by_pair: dict[tuple[int, int], int] = {}
    edge_vertices: dict[int, tuple[int, int]] = {}
    occurrences: dict[int, list[int]] = {}
    half_edge_data: dict[int, tuple[int, int, int]] = {}
    loop_half_edges: dict[int, tuple[int, ...]] = {}
    next_edge = item_id(1)
    next_half_edge = item_id(1)
    for face_id, cycle, _normal, _x_axis in face_cycles:
        half_edge_ids: list[int] = []
        for start, end in zip(cycle, cycle[1:] + cycle[:1], strict=True):
            key = tuple(sorted((start, end)))
            if key not in edge_by_pair:
                edge_by_pair[key] = next_edge
                edge_vertices[next_edge] = (start, end)
                next_edge += 1
            edge_id = edge_by_pair[key]
            half_edge_data[next_half_edge] = (face_id, edge_id, end)
            occurrences.setdefault(edge_id, []).append(next_half_edge)
            half_edge_ids.append(next_half_edge)
            next_half_edge += 1
        loop_half_edges[face_id] = tuple(half_edge_ids)

    curve_sources = {edge_id: sources.next("line") for edge_id in edge_vertices}
    edge_sources = {edge_id: sources.next("edge") for edge_id in edge_vertices}
    half_edge_sources = {half_edge_id: sources.next("fin") for half_edge_id in half_edge_data}
    loop_sources = {face_id: sources.next("loop") for face_id, *_rest in face_cycles}
    face_sources = {face_id: sources.next("face") for face_id, *_rest in face_cycles}
    surface_sources = {face_id: sources.next("plane") for face_id, *_rest in face_cycles}
    curves = []
    edges = []
    for edge_id, (start, end) in edge_vertices.items():
        start_position = Vector3(*positions[start])
        end_position = Vector3(*positions[end])
        curves.append(
            CurveGeometry(
                id=edge_id,
                sense=Sense.POSITIVE,
                owner=edge_sources[edge_id],
                kind=CurveKind.LINE,
                definition=LineCurve(
                    point=start_position,
                    direction=Vector3(
                        end_position.x - start_position.x,
                        end_position.y - start_position.y,
                        end_position.z - start_position.z,
                    ),
                ),
                source=curve_sources[edge_id],
            )
        )
        edges.append(
            Edge(
                id=edge_id,
                owner=body_source,
                half_edges=tuple(occurrences[edge_id]),
                start_vertex=start,
                end_vertex=end,
                curve=edge_id,
                tolerance=None,
                source=edge_sources[edge_id],
            )
        )
    half_edges = []
    for half_edge_id, (face_id, edge_id, end) in half_edge_data.items():
        loop_members = loop_half_edges[face_id]
        loop_index = loop_members.index(half_edge_id)
        edge_members = occurrences[edge_id]
        half_edges.append(
            HalfEdge(
                id=half_edge_id,
                loop=face_id,
                forward=loop_members[(loop_index + 1) % len(loop_members)],
                backward=loop_members[(loop_index - 1) % len(loop_members)],
                vertex=end,
                other=next(item for item in edge_members if item != half_edge_id),
                edge=edge_id,
                curve=edge_id,
                sense=Sense.POSITIVE,
                dummy=False,
                source=half_edge_sources[half_edge_id],
            )
        )

    loops = tuple(
        Loop(
            id=face_id,
            face=face_id,
            half_edges=loop_half_edges[face_id],
            source=loop_sources[face_id],
        )
        for face_id, *_rest in face_cycles
    )
    surfaces = []
    faces = []
    for face_id, cycle, normal, x_axis in face_cycles:
        surfaces.append(
            SurfaceGeometry(
                id=face_id,
                sense=Sense.POSITIVE,
                owner=face_sources[face_id],
                kind=SurfaceKind.PLANE,
                definition=PlaneSurface(
                    point=Vector3(*positions[cycle[0]]),
                    normal=Vector3(*normal),
                    x_axis=Vector3(*x_axis),
                ),
                source=surface_sources[face_id],
            )
        )
        faces.append(
            Face(
                id=face_id,
                back_shell=shell_id,
                front_shell=shell_id,
                loops=(face_id,),
                surface=face_id,
                sense=Sense.POSITIVE,
                source=face_sources[face_id],
            )
        )

    return BrepModel(
        source_format="text",
        schema_key="synthetic-box",
        complete=True,
        bodies=(
            Body(
                id=body_id,
                kind=BodyKind.SOLID,
                size_resolution=1.0e-6,
                linear_resolution=1.0e-8,
                regions=(region_id,),
                edges=tuple(sorted(edge_vertices)),
                vertices=tuple(positions),
                source=body_source,
            ),
        ),
        regions=(Region(region_id, RegionKind.SOLID, body_id, (shell_id,), region_source),),
        shells=(
            Shell(
                shell_id,
                region_id,
                (),
                tuple(item_id(value) for value in range(1, 7)),
                (),
                None,
                shell_source,
            ),
        ),
        faces=tuple(faces),
        loops=loops,
        half_edges=tuple(half_edges),
        edges=tuple(edges),
        vertices=vertices,
        points=points,
        curves=tuple(curves),
        surfaces=tuple(surfaces),
        topology=TopologyValidation(True, 6, 12, 2),
        metrics=BrepMetrics(
            BoundingBox(
                Vector3(x_origin, y_origin, z_origin),
                Vector3(x_origin + length, y_origin + width, z_origin + height),
            ),
            2.0 * (length * width + width * height + height * length),
            length * width * height,
        ),
        diagnostics=(),
    )


def make_two_box_model(
    length: float = 40.0,
    width: float = 30.0,
    height: float = 20.0,
    gap: float = 10.0,
) -> BrepModel:
    """Return two disjoint solids with independent IDs and source provenance."""

    first = make_box_model(length, width, height)
    second = make_box_model(
        length,
        width,
        height,
        _origin=(length + gap, 0.0, 0.0),
        _id_offset=100,
        _source_offset=1000,
    )
    return BrepModel(
        source_format="text",
        schema_key="synthetic-two-boxes",
        complete=True,
        bodies=first.bodies + second.bodies,
        regions=first.regions + second.regions,
        shells=first.shells + second.shells,
        faces=first.faces + second.faces,
        loops=first.loops + second.loops,
        half_edges=first.half_edges + second.half_edges,
        edges=first.edges + second.edges,
        vertices=first.vertices + second.vertices,
        points=first.points + second.points,
        curves=first.curves + second.curves,
        surfaces=first.surfaces + second.surfaces,
        topology=TopologyValidation(True, 12, 24, 4),
        metrics=BrepMetrics(
            BoundingBox(
                Vector3(0.0, 0.0, 0.0),
                Vector3(2.0 * length + gap, width, height),
            ),
            4.0 * (length * width + width * height + height * length),
            2.0 * length * width * height,
        ),
        diagnostics=(),
    )


def make_cylinder_hole_model(
    outer_radius: float = 10.0,
    inner_radius: float = 4.0,
    height: float = 30.0,
) -> BrepModel:
    sources = _Sources()
    body_source = sources.next("body")
    region_source = sources.next("region")
    shell_source = sources.next("shell")
    ring_specs = {
        1: (outer_radius, 0.0),
        2: (outer_radius, height),
        3: (inner_radius, 0.0),
        4: (inner_radius, height),
    }
    curve_sources = {edge_id: sources.next("circle") for edge_id in ring_specs}
    edge_sources = {edge_id: sources.next("edge") for edge_id in ring_specs}
    face_sources = {face_id: sources.next("face") for face_id in range(1, 5)}
    surface_sources = {face_id: sources.next("surface") for face_id in range(1, 5)}
    # outer cylinder, inner cylinder, top annulus, bottom annulus
    face_ring_edges = {1: (1, 2), 2: (3, 4), 3: (2, 4), 4: (1, 3)}
    loop_sources = {loop_id: sources.next("loop") for loop_id in range(1, 9)}
    half_edge_sources = {half_edge_id: sources.next("fin") for half_edge_id in range(1, 9)}
    loop_face_edge: dict[int, tuple[int, int]] = {}
    loop_id = 1
    for face_id, edge_ids in face_ring_edges.items():
        for edge_id in edge_ids:
            loop_face_edge[loop_id] = (face_id, edge_id)
            loop_id += 1
    edge_uses: dict[int, list[int]] = {edge_id: [] for edge_id in ring_specs}
    for half_edge_id, (_face_id, edge_id) in loop_face_edge.items():
        edge_uses[edge_id].append(half_edge_id)

    curves = tuple(
        CurveGeometry(
            id=edge_id,
            sense=Sense.POSITIVE,
            owner=edge_sources[edge_id],
            kind=CurveKind.CIRCLE,
            definition=CircleCurve(
                center=Vector3(0.0, 0.0, z_value),
                normal=Vector3(0.0, 0.0, 1.0),
                x_axis=Vector3(1.0, 0.0, 0.0),
                radius=radius,
            ),
            source=curve_sources[edge_id],
        )
        for edge_id, (radius, z_value) in ring_specs.items()
    )
    edges = tuple(
        Edge(
            id=edge_id,
            owner=body_source,
            half_edges=tuple(edge_uses[edge_id]),
            start_vertex=None,
            end_vertex=None,
            curve=edge_id,
            tolerance=None,
            source=edge_sources[edge_id],
        )
        for edge_id in ring_specs
    )
    planar_senses = {
        5: Sense.POSITIVE,  # top outer
        6: Sense.NEGATIVE,  # top inner
        7: Sense.POSITIVE,  # bottom outer before the face reversal
        8: Sense.NEGATIVE,  # bottom inner before the face reversal
    }
    half_edges = tuple(
        HalfEdge(
            id=half_edge_id,
            loop=half_edge_id,
            forward=half_edge_id,
            backward=half_edge_id,
            vertex=None,
            other=next(item for item in edge_uses[edge_id] if item != half_edge_id),
            edge=edge_id,
            curve=edge_id,
            sense=planar_senses.get(half_edge_id, Sense.POSITIVE),
            dummy=False,
            source=half_edge_sources[half_edge_id],
        )
        for half_edge_id, (_face_id, edge_id) in loop_face_edge.items()
    )
    loops = tuple(
        Loop(loop_id, face_id, (loop_id,), loop_sources[loop_id])
        for loop_id, (face_id, _edge_id) in loop_face_edge.items()
    )
    surfaces = (
        SurfaceGeometry(
            1,
            Sense.POSITIVE,
            face_sources[1],
            SurfaceKind.CYLINDER,
            CylinderSurface(
                Vector3(0.0, 0.0, 0.0),
                Vector3(0.0, 0.0, 1.0),
                outer_radius,
                Vector3(1.0, 0.0, 0.0),
            ),
            surface_sources[1],
        ),
        SurfaceGeometry(
            2,
            Sense.POSITIVE,
            face_sources[2],
            SurfaceKind.CYLINDER,
            CylinderSurface(
                Vector3(0.0, 0.0, 0.0),
                Vector3(0.0, 0.0, 1.0),
                inner_radius,
                Vector3(1.0, 0.0, 0.0),
            ),
            surface_sources[2],
        ),
        SurfaceGeometry(
            3,
            Sense.POSITIVE,
            face_sources[3],
            SurfaceKind.PLANE,
            PlaneSurface(
                Vector3(0.0, 0.0, height),
                Vector3(0.0, 0.0, 1.0),
                Vector3(1.0, 0.0, 0.0),
            ),
            surface_sources[3],
        ),
        SurfaceGeometry(
            4,
            Sense.POSITIVE,
            face_sources[4],
            SurfaceKind.PLANE,
            PlaneSurface(
                Vector3(0.0, 0.0, 0.0),
                Vector3(0.0, 0.0, 1.0),
                Vector3(1.0, 0.0, 0.0),
            ),
            surface_sources[4],
        ),
    )
    faces = (
        Face(1, 1, 1, (1, 2), 1, Sense.POSITIVE, face_sources[1]),
        Face(2, 1, 1, (3, 4), 2, Sense.NEGATIVE, face_sources[2]),
        Face(3, 1, 1, (5, 6), 3, Sense.POSITIVE, face_sources[3]),
        Face(4, 1, 1, (7, 8), 4, Sense.NEGATIVE, face_sources[4]),
    )
    expected_area = (
        2.0 * pi * outer_radius * height
        + 2.0 * pi * inner_radius * height
        + 2.0 * pi * (outer_radius**2 - inner_radius**2)
    )
    expected_volume = pi * (outer_radius**2 - inner_radius**2) * height
    return BrepModel(
        source_format="binary",
        schema_key="synthetic-cylinder-hole",
        complete=True,
        bodies=(
            Body(
                1,
                BodyKind.SOLID,
                1.0e-6,
                1.0e-8,
                (1,),
                (1, 2, 3, 4),
                (),
                body_source,
            ),
        ),
        regions=(Region(1, RegionKind.SOLID, 1, (1,), region_source),),
        shells=(Shell(1, 1, (), (1, 2, 3, 4), (), None, shell_source),),
        faces=faces,
        loops=loops,
        half_edges=half_edges,
        edges=edges,
        vertices=(),
        points=(),
        curves=curves,
        surfaces=surfaces,
        topology=TopologyValidation(True, 8, 4, 0),
        metrics=BrepMetrics(
            BoundingBox(
                Vector3(-outer_radius, -outer_radius, 0.0),
                Vector3(outer_radius, outer_radius, height),
            ),
            expected_area,
            expected_volume,
        ),
        diagnostics=(),
    )


def make_analytic_curve_sheet_model(kind: CurveKind) -> BrepModel:
    """Return a one-face public fixture for one newly supported exact curve kind."""

    sources = _Sources()
    body_source = sources.next("body")
    region_source = sources.next("region")
    shell_source = sources.next("shell")
    face_source = sources.next("face")
    surface_source = sources.next("plane")
    primary_source = sources.next(kind.value)
    edge_sources = {1: sources.next("edge"), 2: sources.next("edge")}
    loop_source = sources.next("loop")

    extra_curves: tuple[CurveGeometry, ...] = ()
    if kind is CurveKind.ELLIPSE:
        primary_definition = EllipseCurve(
            center=Vector3(0.0, 0.0, 0.0),
            normal=Vector3(0.0, 0.0, 1.0),
            x_axis=Vector3(1.0, 0.0, 0.0),
            major_radius=8.0,
            minor_radius=4.0,
        )
        positions: tuple[Vector3, Vector3] | None = None
    elif kind is CurveKind.PARABOLA:
        primary_definition = ParabolaCurve(
            origin=Vector3(0.0, 0.0, 0.0),
            normal=Vector3(0.0, 0.0, 1.0),
            x_axis=Vector3(1.0, 0.0, 0.0),
            focal_length=2.0,
        )
        positions = (Vector3(2.0, -4.0, 0.0), Vector3(2.0, 4.0, 0.0))
    elif kind is CurveKind.HYPERBOLA:
        parameter = 0.8
        primary_definition = HyperbolaCurve(
            origin=Vector3(0.0, 0.0, 0.0),
            normal=Vector3(0.0, 0.0, 1.0),
            x_axis=Vector3(1.0, 0.0, 0.0),
            transverse_radius=3.0,
            conjugate_radius=2.0,
        )
        positions = (
            Vector3(3.0 * cosh(parameter), -2.0 * sinh(parameter), 0.0),
            Vector3(3.0 * cosh(parameter), 2.0 * sinh(parameter), 0.0),
        )
    elif kind is CurveKind.TRIMMED:
        radius = 5.0
        primary_definition = TrimmedCurve(
            basis_curve=100,
            start_point=Vector3(radius, 0.0, 0.0),
            end_point=Vector3(-radius, 0.0, 0.0),
            start_parameter=0.0,
            end_parameter=pi,
        )
        positions = (primary_definition.start_point, primary_definition.end_point)
        extra_curves = (
            CurveGeometry(
                id=100,
                sense=Sense.POSITIVE,
                owner=edge_sources[1],
                kind=CurveKind.CIRCLE,
                definition=CircleCurve(
                    center=Vector3(0.0, 0.0, 0.0),
                    normal=Vector3(0.0, 0.0, 1.0),
                    x_axis=Vector3(1.0, 0.0, 0.0),
                    radius=radius,
                ),
                source=sources.next("circle"),
            ),
        )
    elif kind is CurveKind.NURBS:
        primary_definition = NurbsCurve(
            degree=2,
            control_vertex_count=3,
            vertex_dimension=3,
            knot_type=1,
            periodic=False,
            closed=False,
            rational=False,
            curve_form=0,
            control_vertices=((0.0, 0.0, 0.0), (5.0, 5.0, 0.0), (10.0, 0.0, 0.0)),
            knots=(0.0, 1.0),
            knot_multiplicities=(3, 3),
            sources=(sources.next("nurbs_curve"),),
        )
        positions = (Vector3(0.0, 0.0, 0.0), Vector3(10.0, 0.0, 0.0))
    else:
        raise ValueError(f"unsupported analytic curve fixture kind: {kind.value}")

    primary = CurveGeometry(
        id=1,
        sense=Sense.POSITIVE,
        owner=edge_sources[1],
        kind=kind,
        definition=primary_definition,
        source=primary_source,
    )
    surface = SurfaceGeometry(
        id=1,
        sense=Sense.POSITIVE,
        owner=face_source,
        kind=SurfaceKind.PLANE,
        definition=PlaneSurface(
            point=Vector3(0.0, 0.0, 0.0),
            normal=Vector3(0.0, 0.0, 1.0),
            x_axis=Vector3(1.0, 0.0, 0.0),
        ),
        source=surface_source,
    )
    if positions is None:
        half_edge_source = sources.next("fin")
        curves = (primary, *extra_curves)
        edges = (Edge(1, body_source, (1,), None, None, 1, None, edge_sources[1]),)
        half_edges = (
            HalfEdge(
                1,
                1,
                1,
                1,
                None,
                None,
                1,
                1,
                Sense.POSITIVE,
                False,
                half_edge_source,
            ),
        )
        points: tuple[PointGeometry, ...] = ()
        vertices: tuple[Vertex, ...] = ()
        body_edges = (1,)
        body_vertices: tuple[int, ...] = ()
    else:
        point_sources = (sources.next("point"), sources.next("point"))
        vertex_sources = (sources.next("vertex"), sources.next("vertex"))
        half_edge_sources = (sources.next("fin"), sources.next("fin"))
        line_source = sources.next("line")
        line = CurveGeometry(
            id=2,
            sense=Sense.POSITIVE,
            owner=edge_sources[2],
            kind=CurveKind.LINE,
            definition=LineCurve(
                point=positions[1],
                direction=Vector3(
                    positions[0].x - positions[1].x,
                    positions[0].y - positions[1].y,
                    positions[0].z - positions[1].z,
                ),
            ),
            source=line_source,
        )
        curves = (primary, line, *extra_curves)
        points = tuple(
            PointGeometry(index, position, vertex_sources[index - 1], point_sources[index - 1])
            for index, position in enumerate(positions, 1)
        )
        vertices = tuple(
            Vertex(index, index, None, body_source, vertex_sources[index - 1]) for index in (1, 2)
        )
        edges = (
            Edge(1, body_source, (1,), 1, 2, 1, None, edge_sources[1]),
            Edge(2, body_source, (2,), 2, 1, 2, None, edge_sources[2]),
        )
        half_edges = (
            HalfEdge(
                1,
                1,
                2,
                2,
                2,
                None,
                1,
                1,
                Sense.POSITIVE,
                False,
                half_edge_sources[0],
            ),
            HalfEdge(
                2,
                1,
                1,
                1,
                1,
                None,
                2,
                2,
                Sense.POSITIVE,
                False,
                half_edge_sources[1],
            ),
        )
        body_edges = (1, 2)
        body_vertices = (1, 2)

    return BrepModel(
        source_format="text",
        schema_key=f"synthetic-{kind.value}-sheet",
        complete=True,
        bodies=(
            Body(1, BodyKind.SHEET, 1.0e-6, 1.0e-8, (1,), body_edges, body_vertices, body_source),
        ),
        regions=(Region(1, RegionKind.SOLID, 1, (1,), region_source),),
        shells=(Shell(1, 1, (), (1,), (), None, shell_source),),
        faces=(Face(1, 1, 1, (1,), 1, Sense.POSITIVE, face_source),),
        loops=(Loop(1, 1, tuple(item.id for item in half_edges), loop_source),),
        half_edges=half_edges,
        edges=edges,
        vertices=vertices,
        points=points,
        curves=curves,
        surfaces=(surface,),
        topology=TopologyValidation(True, 1, len(edges), 1),
        metrics=BrepMetrics(None, None, None),
        diagnostics=(),
    )


def make_cone_frustum_model(
    lower_radius: float = 8.0,
    upper_radius: float = 4.0,
    height: float = 12.0,
) -> BrepModel:
    """Return a closed exact cone-frustum fixture with two planar caps."""

    sources = _Sources()
    body_source = sources.next("body")
    region_source = sources.next("region")
    shell_source = sources.next("shell")
    edge_sources = (sources.next("edge"), sources.next("edge"))
    curve_sources = (sources.next("circle"), sources.next("circle"))
    face_sources = tuple(sources.next("face") for _ in range(3))
    surface_sources = tuple(sources.next("surface") for _ in range(3))
    loop_sources = tuple(sources.next("loop") for _ in range(4))
    half_edge_sources = tuple(sources.next("fin") for _ in range(4))
    tangent = (upper_radius - lower_radius) / height
    cosine = 1.0 / (1.0 + tangent**2) ** 0.5
    sine = tangent * cosine
    curves = (
        CurveGeometry(
            1,
            Sense.POSITIVE,
            edge_sources[0],
            CurveKind.CIRCLE,
            CircleCurve(
                Vector3(0.0, 0.0, 0.0),
                Vector3(0.0, 0.0, 1.0),
                Vector3(1.0, 0.0, 0.0),
                lower_radius,
            ),
            curve_sources[0],
        ),
        CurveGeometry(
            2,
            Sense.POSITIVE,
            edge_sources[1],
            CurveKind.CIRCLE,
            CircleCurve(
                Vector3(0.0, 0.0, height),
                Vector3(0.0, 0.0, 1.0),
                Vector3(1.0, 0.0, 0.0),
                upper_radius,
            ),
            curve_sources[1],
        ),
    )
    edges = (
        Edge(1, body_source, (1, 4), None, None, 1, None, edge_sources[0]),
        Edge(2, body_source, (2, 3), None, None, 2, None, edge_sources[1]),
    )
    half_edges = (
        HalfEdge(1, 1, 1, 1, None, 4, 1, 1, Sense.POSITIVE, False, half_edge_sources[0]),
        HalfEdge(2, 2, 2, 2, None, 3, 2, 2, Sense.POSITIVE, False, half_edge_sources[1]),
        HalfEdge(3, 3, 3, 3, None, 2, 2, 2, Sense.POSITIVE, False, half_edge_sources[2]),
        HalfEdge(4, 4, 4, 4, None, 1, 1, 1, Sense.POSITIVE, False, half_edge_sources[3]),
    )
    surfaces = (
        SurfaceGeometry(
            1,
            Sense.POSITIVE,
            face_sources[0],
            SurfaceKind.CONE,
            ConeSurface(
                Vector3(0.0, 0.0, 0.0),
                Vector3(0.0, 0.0, 1.0),
                lower_radius,
                sine,
                cosine,
                Vector3(1.0, 0.0, 0.0),
            ),
            surface_sources[0],
        ),
        SurfaceGeometry(
            2,
            Sense.POSITIVE,
            face_sources[1],
            SurfaceKind.PLANE,
            PlaneSurface(
                Vector3(0.0, 0.0, height),
                Vector3(0.0, 0.0, 1.0),
                Vector3(1.0, 0.0, 0.0),
            ),
            surface_sources[1],
        ),
        SurfaceGeometry(
            3,
            Sense.POSITIVE,
            face_sources[2],
            SurfaceKind.PLANE,
            PlaneSurface(
                Vector3(0.0, 0.0, 0.0),
                Vector3(0.0, 0.0, 1.0),
                Vector3(1.0, 0.0, 0.0),
            ),
            surface_sources[2],
        ),
    )
    volume = pi * height * (lower_radius**2 + lower_radius * upper_radius + upper_radius**2) / 3.0
    return BrepModel(
        source_format="text",
        schema_key="synthetic-cone-frustum",
        complete=True,
        bodies=(Body(1, BodyKind.SOLID, 1.0e-6, 1.0e-8, (1,), (1, 2), (), body_source),),
        regions=(Region(1, RegionKind.SOLID, 1, (1,), region_source),),
        shells=(Shell(1, 1, (), (1, 2, 3), (), None, shell_source),),
        faces=(
            Face(1, 1, 1, (1, 2), 1, Sense.POSITIVE, face_sources[0]),
            Face(2, 1, 1, (3,), 2, Sense.POSITIVE, face_sources[1]),
            Face(3, 1, 1, (4,), 3, Sense.NEGATIVE, face_sources[2]),
        ),
        loops=tuple(
            Loop(index, 1 if index < 3 else index - 1, (index,), loop_sources[index - 1])
            for index in range(1, 5)
        ),
        half_edges=half_edges,
        edges=edges,
        vertices=(),
        points=(),
        curves=curves,
        surfaces=surfaces,
        topology=TopologyValidation(True, 4, 2, 2),
        metrics=BrepMetrics(
            BoundingBox(
                Vector3(-lower_radius, -lower_radius, 0.0),
                Vector3(lower_radius, lower_radius, height),
            ),
            None,
            volume,
        ),
        diagnostics=(),
    )


def make_closed_analytic_surface_model(kind: SurfaceKind) -> BrepModel:
    """Return an untrimmed sphere or ring-torus solid with no inferred source seam."""

    sources = _Sources()
    body_source = sources.next("body")
    region_source = sources.next("region")
    shell_source = sources.next("shell")
    face_source = sources.next("face")
    surface_source = sources.next(kind.value)
    if kind is SurfaceKind.SPHERE:
        radius = 6.0
        definition = SphereSurface(
            Vector3(0.0, 0.0, 0.0),
            radius,
            Vector3(0.0, 0.0, 1.0),
            Vector3(1.0, 0.0, 0.0),
        )
        bounds = BoundingBox(Vector3(-radius, -radius, -radius), Vector3(radius, radius, radius))
        area = 4.0 * pi * radius**2
        volume = 4.0 / 3.0 * pi * radius**3
    elif kind is SurfaceKind.TORUS:
        major = 8.0
        minor = 2.0
        definition = TorusSurface(
            Vector3(0.0, 0.0, 0.0),
            Vector3(0.0, 0.0, 1.0),
            major,
            minor,
            Vector3(1.0, 0.0, 0.0),
        )
        bounds = BoundingBox(
            Vector3(-major - minor, -major - minor, -minor),
            Vector3(major + minor, major + minor, minor),
        )
        area = 4.0 * pi**2 * major * minor
        volume = 2.0 * pi**2 * major * minor**2
    else:
        raise ValueError(f"unsupported closed analytic fixture kind: {kind.value}")
    return BrepModel(
        source_format="binary",
        schema_key=f"synthetic-{kind.value}",
        complete=True,
        bodies=(Body(1, BodyKind.SOLID, 1.0e-6, 1.0e-8, (1,), (), (), body_source),),
        regions=(Region(1, RegionKind.SOLID, 1, (1,), region_source),),
        shells=(Shell(1, 1, (), (1,), (), None, shell_source),),
        faces=(Face(1, 1, 1, (), 1, Sense.POSITIVE, face_source),),
        loops=(),
        half_edges=(),
        edges=(),
        vertices=(),
        points=(),
        curves=(),
        surfaces=(
            SurfaceGeometry(
                1,
                Sense.POSITIVE,
                face_source,
                kind,
                definition,
                surface_source,
            ),
        ),
        topology=TopologyValidation(True, 0, 0, 2),
        metrics=BrepMetrics(bounds, area, volume),
        diagnostics=(),
    )


def make_nurbs_surface_model(*, offset: float | None = None) -> BrepModel:
    """Return a finite non-rational NURBS sheet, optionally through OFFSET_SURF."""

    sources = _Sources()
    body_source = sources.next("body")
    region_source = sources.next("region")
    shell_source = sources.next("shell")
    face_source = sources.next("face")
    basis_source = sources.next("b_surface")
    nurbs = NurbsSurface(
        u_degree=1,
        v_degree=1,
        u_control_vertex_count=2,
        v_control_vertex_count=2,
        vertex_dimension=3,
        u_knot_type=1,
        v_knot_type=1,
        u_periodic=False,
        v_periodic=False,
        u_closed=False,
        v_closed=False,
        rational=False,
        surface_form=0,
        control_vertices=(
            (0.0, 0.0, 0.0),
            (0.0, 10.0, 0.0),
            (10.0, 0.0, 0.0),
            (10.0, 10.0, 0.0),
        ),
        u_knots=(0.0, 1.0),
        v_knots=(0.0, 1.0),
        u_knot_multiplicities=(2, 2),
        v_knot_multiplicities=(2, 2),
        sources=(sources.next("nurbs_surface"),),
    )
    basis = SurfaceGeometry(
        1,
        Sense.POSITIVE,
        face_source,
        SurfaceKind.NURBS,
        nurbs,
        basis_source,
    )
    if offset is None:
        surfaces = (basis,)
        face_surface = 1
        schema_key = "synthetic-nurbs-surface"
    else:
        offset_source = sources.next("offset_surface")
        surfaces = (
            basis,
            SurfaceGeometry(
                2,
                Sense.POSITIVE,
                face_source,
                SurfaceKind.OFFSET,
                OffsetSurface(1, offset),
                offset_source,
            ),
        )
        face_surface = 2
        schema_key = "synthetic-offset-surface"
    return BrepModel(
        source_format="text",
        schema_key=schema_key,
        complete=True,
        bodies=(Body(1, BodyKind.SHEET, 1.0e-6, 1.0e-8, (1,), (), (), body_source),),
        regions=(Region(1, RegionKind.SOLID, 1, (1,), region_source),),
        shells=(Shell(1, 1, (), (1,), (), None, shell_source),),
        faces=(Face(1, 1, 1, (), face_surface, Sense.POSITIVE, face_source),),
        loops=(),
        half_edges=(),
        edges=(),
        vertices=(),
        points=(),
        curves=(),
        surfaces=surfaces,
        topology=TopologyValidation(True, 0, 0, 1),
        metrics=BrepMetrics(None, None, None),
        diagnostics=(),
    )
