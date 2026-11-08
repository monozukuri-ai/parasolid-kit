"""Validated conversion from private native B-Rep mappings."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from typing import Any, cast

from ..binary.header import ByteRange
from ..diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity, SourceLocation
from .geometry import (
    BlendBoundarySurface,
    BlendedEdgeSurface,
    BlendType,
    CircleCurve,
    ConeSurface,
    CurveDefinition,
    CurveGeometry,
    CurveKind,
    CylinderSurface,
    EllipseCurve,
    HyperbolaCurve,
    IntersectionChartParameters,
    IntersectionCurve,
    IntersectionData,
    IntersectionUvCoordinates,
    LineCurve,
    MeshSurface,
    MeshTransformNode,
    MeshTransformPrecision,
    NurbsCurve,
    NurbsSurface,
    OffsetSurface,
    ParabolaCurve,
    PlaneSurface,
    PointGeometry,
    PolylineCurve,
    RawIntegerPool,
    RawMeshData,
    RawVectorPool,
    SphereSurface,
    SpunSurface,
    SurfaceDefinition,
    SurfaceGeometry,
    SurfaceKind,
    SurfaceParametricCurve,
    SweptSurface,
    TorusSurface,
    TrimmedCurve,
    UnsupportedGeometry,
    XtConeSurface,
)
from .model import BrepMetrics, BrepModel, BrepSourceFormat, TopologyValidation
from .topology import (
    Assembly,
    AssemblyKind,
    Attribute,
    AttributeField,
    AttributeOwner,
    AttributeOwnerKind,
    Body,
    BodyKind,
    BoundingBox,
    Edge,
    Face,
    HalfEdge,
    Instance,
    InstancePartKind,
    Loop,
    Region,
    RegionKind,
    Sense,
    Shell,
    SourceNodeRef,
    Transform,
    Vector3,
    Vertex,
)


def brep_from_native(value: Mapping[str, Any]) -> BrepModel:
    """Build an immutable public model from one native response."""

    source_format = _str(value, "source_format")
    if source_format not in {"binary", "text"}:
        raise RuntimeError("native B-Rep source format is invalid")
    topology = _mapping(value.get("topology"), "topology")
    metrics = _mapping(value.get("metrics"), "metrics")
    bounds_value = metrics.get("bounding_box")
    bounds = None
    if bounds_value is not None:
        bounds_mapping = _mapping(bounds_value, "bounding box")
        bounds = BoundingBox(
            minimum=_vector(bounds_mapping, "minimum"),
            maximum=_vector(bounds_mapping, "maximum"),
        )
    return BrepModel(
        source_format=cast(BrepSourceFormat, source_format),
        schema_key=_str(value, "schema_key"),
        complete=_bool(value, "complete"),
        assemblies=tuple(_assembly(item) for item in _list(value, "assemblies")),
        instances=tuple(_instance(item) for item in _list(value, "instances")),
        bodies=tuple(_body(item) for item in _list(value, "bodies")),
        regions=tuple(_region(item) for item in _list(value, "regions")),
        shells=tuple(_shell(item) for item in _list(value, "shells")),
        faces=tuple(_face(item) for item in _list(value, "faces")),
        loops=tuple(_loop(item) for item in _list(value, "loops")),
        half_edges=tuple(_half_edge(item) for item in _list(value, "half_edges")),
        edges=tuple(_edge(item) for item in _list(value, "edges")),
        vertices=tuple(_vertex(item) for item in _list(value, "vertices")),
        points=tuple(_point(item) for item in _list(value, "points")),
        curves=tuple(_curve(item) for item in _list(value, "curves")),
        surfaces=tuple(_surface(item) for item in _list(value, "surfaces")),
        attributes=(
            tuple(_attribute(item) for item in _list(value, "attributes"))
            if "attributes" in value
            else ()
        ),
        topology=TopologyValidation(
            valid=_bool(topology, "valid"),
            closed_loop_count=_int(topology, "closed_loop_count"),
            closed_edge_ring_count=_int(topology, "closed_edge_ring_count"),
            euler_characteristic=_int(topology, "euler_characteristic"),
        ),
        metrics=BrepMetrics(
            bounding_box=bounds,
            surface_area=_optional_float(metrics, "surface_area"),
            volume=_optional_float(metrics, "volume"),
        ),
        diagnostics=tuple(
            _diagnostic(item, _str(value, "schema_key")) for item in _list(value, "diagnostics")
        ),
    )


def _assembly(value: object) -> Assembly:
    item = _mapping(value, "assembly")
    return Assembly(
        id=_int(item, "id"),
        kind=AssemblyKind(_str(item, "kind")),
        size_resolution=_float(item, "size_resolution"),
        linear_resolution=_float(item, "linear_resolution"),
        instances=_int_tuple(item, "instances"),
        referencing_instances=_int_tuple(item, "referencing_instances"),
        source=_source(item.get("source")),
    )


def _attribute(value: object) -> Attribute:
    item = _mapping(value, "attribute")
    owner = _mapping(item.get("owner"), "attribute owner")
    owner_source = owner.get("source")
    fields = []
    for raw in _list(item, "fields"):
        field = _mapping(raw, "attribute field")
        kind = _str(field, "kind")
        if kind in {"integers", "reals", "tags", "points", "vectors", "directions", "axes"}:
            values = field.get("values")
            if not isinstance(values, list):
                raise RuntimeError("native attribute values are not a list")
            fields.append(AttributeField(kind=kind, values=tuple(values)))
        elif kind in {"text", "unicode"}:
            fields.append(AttributeField(kind=kind, value=_str(field, "value")))
        else:
            fields.append(
                AttributeField(
                    kind="unsupported",
                    type_name=_str(field, "type_name"),
                    node_index=_int(field, "node_index"),
                )
            )
    return Attribute(
        id=_int(item, "id"),
        identifier=_str(item, "identifier"),
        kind_id=_optional_int(item, "kind_id"),
        owner=AttributeOwner(
            kind=AttributeOwnerKind(_str(owner, "kind")),
            id=_optional_int(owner, "id"),
            source=None if owner_source is None else _source(owner_source),
        ),
        fields=tuple(fields),
        source=_source(item.get("source")),
    )


def _instance(value: object) -> Instance:
    item = _mapping(value, "instance")
    transform = item.get("transform")
    return Instance(
        id=_int(item, "id"),
        negative=_bool(item, "negative"),
        part_kind=InstancePartKind(_str(item, "part_kind")),
        part=_int(item, "part"),
        transform=None if transform is None else _transform(transform),
        assembly=_int(item, "assembly"),
        source=_source(item.get("source")),
    )


def _transform(value: object) -> Transform:
    item = _mapping(value, "transform")
    rows = _float_matrix(item, "rotation")
    if len(rows) != 3 or any(len(row) != 3 for row in rows):
        raise RuntimeError("native transform rotation is not a 3x3 matrix")
    first, second, third = rows
    return Transform(
        rotation=(
            (first[0], first[1], first[2]),
            (second[0], second[1], second[2]),
            (third[0], third[1], third[2]),
        ),
        translation=_vector(item, "translation"),
        scale=_float(item, "scale"),
        flags=_int(item, "flags"),
        perspective=_vector(item, "perspective"),
        source=_source(item.get("source")),
    )


def _body(value: object) -> Body:
    item = _mapping(value, "body")
    return Body(
        id=_int(item, "id"),
        kind=BodyKind(_str(item, "kind")),
        size_resolution=_float(item, "size_resolution"),
        linear_resolution=_float(item, "linear_resolution"),
        regions=_int_tuple(item, "regions"),
        edges=_int_tuple(item, "edges"),
        vertices=_int_tuple(item, "vertices"),
        source=_source(item.get("source")),
        attributes=_int_tuple(item, "attributes") if "attributes" in item else (),
    )


def _region(value: object) -> Region:
    item = _mapping(value, "region")
    return Region(
        id=_int(item, "id"),
        kind=RegionKind(_str(item, "kind")),
        body=_int(item, "body"),
        shells=_int_tuple(item, "shells"),
        source=_source(item.get("source")),
    )


def _shell(value: object) -> Shell:
    item = _mapping(value, "shell")
    return Shell(
        id=_int(item, "id"),
        region=_int(item, "region"),
        back_faces=_int_tuple(item, "back_faces"),
        front_faces=_int_tuple(item, "front_faces"),
        wire_edges=_int_tuple(item, "wire_edges"),
        isolated_vertex=_optional_int(item, "isolated_vertex"),
        source=_source(item.get("source")),
    )


def _face(value: object) -> Face:
    item = _mapping(value, "face")
    return Face(
        id=_int(item, "id"),
        back_shell=_int(item, "back_shell"),
        front_shell=_int(item, "front_shell"),
        loops=_int_tuple(item, "loops"),
        surface=_optional_int(item, "surface"),
        sense=Sense(_str(item, "sense")),
        source=_source(item.get("source")),
        attributes=_int_tuple(item, "attributes") if "attributes" in item else (),
    )


def _loop(value: object) -> Loop:
    item = _mapping(value, "loop")
    return Loop(
        id=_int(item, "id"),
        face=_int(item, "face"),
        half_edges=_int_tuple(item, "half_edges"),
        source=_source(item.get("source")),
    )


def _half_edge(value: object) -> HalfEdge:
    item = _mapping(value, "half-edge")
    return HalfEdge(
        id=_int(item, "id"),
        loop=_optional_int(item, "loop"),
        forward=_optional_int(item, "forward"),
        backward=_optional_int(item, "backward"),
        vertex=_optional_int(item, "vertex"),
        other=_optional_int(item, "other"),
        edge=_optional_int(item, "edge"),
        curve=_optional_int(item, "curve"),
        sense=Sense(_str(item, "sense")),
        dummy=_bool(item, "dummy"),
        source=_source(item.get("source")),
    )


def _edge(value: object) -> Edge:
    item = _mapping(value, "edge")
    return Edge(
        id=_int(item, "id"),
        owner=_source(item.get("owner")),
        half_edges=_int_tuple(item, "half_edges"),
        start_vertex=_optional_int(item, "start_vertex"),
        end_vertex=_optional_int(item, "end_vertex"),
        curve=_optional_int(item, "curve"),
        tolerance=_optional_float(item, "tolerance"),
        source=_source(item.get("source")),
    )


def _vertex(value: object) -> Vertex:
    item = _mapping(value, "vertex")
    return Vertex(
        id=_int(item, "id"),
        point=_int(item, "point"),
        tolerance=_optional_float(item, "tolerance"),
        owner=_source(item.get("owner")),
        source=_source(item.get("source")),
    )


def _point(value: object) -> PointGeometry:
    item = _mapping(value, "point")
    owner = item.get("owner")
    return PointGeometry(
        id=_int(item, "id"),
        position=_vector(item, "position"),
        owner=None if owner is None else _source(owner),
        source=_source(item.get("source")),
    )


def _curve(value: object) -> CurveGeometry:
    item = _mapping(value, "curve")
    owner = item.get("owner")
    kind = CurveKind(_str(item, "kind"))
    parameters = _mapping(item.get("parameters"), "curve parameters")
    return CurveGeometry(
        id=_int(item, "id"),
        sense=Sense(_str(item, "sense")),
        owner=None if owner is None else _source(owner),
        kind=kind,
        definition=_curve_definition(kind, parameters),
        source=_source(item.get("source")),
    )


def _curve_definition(kind: CurveKind, value: Mapping[str, Any]) -> CurveDefinition:
    if kind is CurveKind.LINE:
        return LineCurve(point=_vector(value, "point"), direction=_vector(value, "direction"))
    if kind is CurveKind.CIRCLE:
        return CircleCurve(
            center=_vector(value, "center"),
            normal=_vector(value, "normal"),
            x_axis=_vector(value, "x_axis"),
            radius=_float(value, "radius"),
        )
    if kind is CurveKind.ELLIPSE:
        return EllipseCurve(
            center=_vector(value, "center"),
            normal=_vector(value, "normal"),
            x_axis=_vector(value, "x_axis"),
            major_radius=_float(value, "major_radius"),
            minor_radius=_float(value, "minor_radius"),
        )
    if kind is CurveKind.PARABOLA:
        return ParabolaCurve(
            origin=_vector(value, "origin"),
            normal=_vector(value, "normal"),
            x_axis=_vector(value, "x_axis"),
            focal_length=_float(value, "focal_length"),
        )
    if kind is CurveKind.HYPERBOLA:
        return HyperbolaCurve(
            origin=_vector(value, "origin"),
            normal=_vector(value, "normal"),
            x_axis=_vector(value, "x_axis"),
            transverse_radius=_float(value, "transverse_radius"),
            conjugate_radius=_float(value, "conjugate_radius"),
        )
    if kind is CurveKind.TRIMMED:
        return TrimmedCurve(
            basis_curve=_int(value, "basis_curve"),
            start_point=_vector(value, "start_point"),
            end_point=_vector(value, "end_point"),
            start_parameter=_float(value, "start_parameter"),
            end_parameter=_float(value, "end_parameter"),
        )
    if kind is CurveKind.NURBS:
        return _nurbs_curve(value)
    if kind is CurveKind.POLYLINE:
        return PolylineCurve(
            points=tuple(_vector({"point": point}, "point") for point in _list(value, "points")),
            closed=_bool(value, "closed"),
            sense=Sense(_str(value, "sense")),
            base_parameter=_float(value, "base_parameter"),
            source=_source(value.get("source")),
        )
    if kind is CurveKind.SURFACE_PARAMETRIC:
        return SurfaceParametricCurve(
            surface=_int(value, "surface"),
            parameter_curve=_int(value, "parameter_curve"),
            original_curve=_optional_int(value, "original_curve"),
            tolerance_to_original=_optional_float(value, "tolerance_to_original"),
        )
    if kind is CurveKind.INTERSECTION:
        surfaces = _int_tuple(value, "surfaces")
        if len(surfaces) != 2:
            raise RuntimeError("native intersection curve does not contain two surfaces")
        return IntersectionCurve(
            surfaces=(surfaces[0], surfaces[1]),
            chart=_source(value.get("chart")),
            chart_points=tuple(
                _vector({"point": point}, "point") for point in value["chart_points"]
            ),
            start_points=tuple(
                _vector({"point": point}, "point") for point in value["start_points"]
            ),
            end_points=tuple(_vector({"point": point}, "point") for point in value["end_points"]),
            start=_source(value.get("start")),
            end=_source(value.get("end")),
            intersection_data=(
                None
                if value.get("intersection_data") is None
                else _source(value.get("intersection_data"))
            ),
            chart_parameters=(
                None
                if value.get("chart_parameters") is None
                else _intersection_chart_parameters(value.get("chart_parameters"))
            ),
            start_limit_kind=_optional_str(value, "start_limit_kind"),
            start_term_use=_optional_int(value, "start_term_use"),
            end_limit_kind=_optional_str(value, "end_limit_kind"),
            end_term_use=_optional_int(value, "end_term_use"),
            intersection_uv=(
                None
                if value.get("intersection_uv") is None
                else _intersection_data(value.get("intersection_uv"))
            ),
        )
    return UnsupportedGeometry(type_name=_str(value, "type_name"))


def _nurbs_curve(value: Mapping[str, Any]) -> NurbsCurve:
    return NurbsCurve(
        degree=_int(value, "degree"),
        control_vertex_count=_int(value, "control_vertex_count"),
        vertex_dimension=_int(value, "vertex_dimension"),
        knot_type=_int(value, "knot_type"),
        periodic=_bool(value, "periodic"),
        closed=_bool(value, "closed"),
        rational=_bool(value, "rational"),
        curve_form=_int(value, "curve_form"),
        control_vertices=_float_matrix(value, "control_vertices"),
        knots=_float_tuple(value, "knots"),
        knot_multiplicities=_int_tuple(value, "knot_multiplicities"),
        sources=tuple(_source(item) for item in _list(value, "sources")),
    )


def _intersection_chart_parameters(value: object) -> IntersectionChartParameters:
    item = _mapping(value, "intersection chart parameters")
    return IntersectionChartParameters(
        base_parameter=_optional_float(item, "base_parameter"),
        base_scale=_optional_float(item, "base_scale"),
        chordal_error=_optional_float(item, "chordal_error"),
        angular_error=_optional_float(item, "angular_error"),
    )


def _intersection_data(value: object) -> IntersectionData:
    item = _mapping(value, "intersection UV data")
    raw_values = item.get("raw_values")
    if not isinstance(raw_values, list) or any(
        part is not None and (isinstance(part, bool) or not isinstance(part, float))
        for part in raw_values
    ):
        raise RuntimeError("native intersection UV raw_values is not a float-or-None list")
    chart = item.get("chart")
    if chart is not None and not isinstance(chart, list):
        raise RuntimeError("native intersection UV chart is not a list or None")
    return IntersectionData(
        source=_source(item.get("source")),
        uv_type=_str(item, "uv_type"),
        raw_values=tuple(raw_values),
        start_is_terminator=_bool(item, "start_is_terminator"),
        start_terminator=(
            None
            if item.get("start_terminator") is None
            else _intersection_uv_coordinates(item.get("start_terminator"))
        ),
        end_is_terminator=_bool(item, "end_is_terminator"),
        chart=(
            None if chart is None else tuple(_intersection_uv_coordinates(value) for value in chart)
        ),
        end_terminator=(
            None
            if item.get("end_terminator") is None
            else _intersection_uv_coordinates(item.get("end_terminator"))
        ),
    )


def _intersection_uv_coordinates(value: object) -> IntersectionUvCoordinates:
    if not isinstance(value, tuple) or len(value) != 2:
        raise RuntimeError("native intersection UV coordinates are not a pair")
    pairs = []
    for pair in value:
        if pair is None:
            pairs.append(None)
        elif (
            isinstance(pair, tuple)
            and len(pair) == 2
            and all(
                part is None or (isinstance(part, float) and not isinstance(part, bool))
                for part in pair
            )
        ):
            pairs.append(pair)
        else:
            raise RuntimeError("native intersection UV coordinate pair is invalid")
    return IntersectionUvCoordinates(first_surface=pairs[0], second_surface=pairs[1])


def _surface(value: object) -> SurfaceGeometry:
    item = _mapping(value, "surface")
    owner = item.get("owner")
    kind = SurfaceKind(_str(item, "kind"))
    parameters = _mapping(item.get("parameters"), "surface parameters")
    return SurfaceGeometry(
        id=_int(item, "id"),
        sense=Sense(_str(item, "sense")),
        owner=None if owner is None else _source(owner),
        kind=kind,
        definition=_surface_definition(kind, parameters),
        source=_source(item.get("source")),
    )


def _surface_definition(kind: SurfaceKind, value: Mapping[str, Any]) -> SurfaceDefinition:
    if kind is SurfaceKind.PLANE:
        return PlaneSurface(
            point=_vector(value, "point"),
            normal=_vector(value, "normal"),
            x_axis=_vector(value, "x_axis"),
        )
    if kind is SurfaceKind.CYLINDER:
        return CylinderSurface(
            point=_vector(value, "point"),
            axis=_vector(value, "axis"),
            radius=_float(value, "radius"),
            x_axis=_vector(value, "x_axis"),
        )
    if kind is SurfaceKind.CONE:
        return ConeSurface(
            point=_vector(value, "point"),
            axis=_vector(value, "axis"),
            radius=_float(value, "radius"),
            sin_half_angle=_float(value, "sin_half_angle"),
            cos_half_angle=_float(value, "cos_half_angle"),
            x_axis=_vector(value, "x_axis"),
        )
    if kind is SurfaceKind.XT_CONE:
        return XtConeSurface(
            point=_vector(value, "point"),
            axis=_vector(value, "axis"),
            radius=_float(value, "radius"),
            sin_half_angle=_float(value, "sin_half_angle"),
            cos_half_angle=_float(value, "cos_half_angle"),
            x_axis=_vector(value, "x_axis"),
        )
    if kind is SurfaceKind.MESH:
        source_fields = value.get("unembedded_source_fields")
        if source_fields is not None:
            if not isinstance(source_fields, list):
                raise RuntimeError("native mesh source fields are not a list or None")
            source_fields = tuple(
                dict(_mapping(field, "unembedded mesh source field")) for field in source_fields
            )
        embedded = value.get("embedded")
        transform = value.get("transform")
        return MeshSurface(
            source_mesh_index=_int(value, "source_mesh_index"),
            embedded=None if embedded is None else _raw_mesh(_mapping(embedded, "embedded mesh")),
            transform_index=_optional_int(value, "transform_index"),
            transform=(
                None
                if transform is None
                else _mesh_transform(_mapping(transform, "mesh transform"))
            ),
            unembedded_source_fields=source_fields,
        )
    if kind is SurfaceKind.SPHERE:
        return SphereSurface(
            center=_vector(value, "center"),
            radius=_float(value, "radius"),
            axis=_vector(value, "axis"),
            x_axis=_vector(value, "x_axis"),
        )
    if kind is SurfaceKind.TORUS:
        return TorusSurface(
            center=_vector(value, "center"),
            axis=_vector(value, "axis"),
            major_radius=_float(value, "major_radius"),
            minor_radius=_float(value, "minor_radius"),
            x_axis=_vector(value, "x_axis"),
        )
    if kind is SurfaceKind.SWEPT:
        return SweptSurface(section=_int(value, "section"), direction=_vector(value, "direction"))
    if kind is SurfaceKind.SPUN:
        return SpunSurface(
            profile=_int(value, "profile"),
            base=_vector(value, "base"),
            axis=_vector(value, "axis"),
            start=_optional_vector(value, "start"),
            end=_optional_vector(value, "end"),
            start_parameter=_optional_float(value, "start_parameter"),
            end_parameter=_optional_float(value, "end_parameter"),
            x_axis=_optional_vector(value, "x_axis"),
        )
    if kind is SurfaceKind.BLENDED_EDGE:
        supporting_surfaces = _int_tuple(value, "supporting_surfaces")
        if len(supporting_surfaces) != 2:
            raise RuntimeError("native blend does not contain two supporting surfaces")
        return BlendedEdgeSurface(
            blend_type=BlendType(_str(value, "blend_type")),
            supporting_surfaces=(supporting_surfaces[0], supporting_surfaces[1]),
            spine_curve=_int(value, "spine_curve"),
            ranges=_float_pair(value, "ranges"),
            thumb_weights=_float_pair(value, "thumb_weights"),
            boundary_surfaces=_optional_int_pair(value, "boundary_surfaces"),
            start=None if value.get("start") is None else _source(value.get("start")),
            end=None if value.get("end") is None else _source(value.get("end")),
        )
    if kind is SurfaceKind.BLEND_BOUNDARY:
        return BlendBoundarySurface(
            boundary_index=_int(value, "boundary_index"),
            blend_surface=_int(value, "blend_surface"),
        )
    if kind is SurfaceKind.OFFSET:
        return OffsetSurface(
            basis_surface=_int(value, "basis_surface"), offset=_float(value, "offset")
        )
    if kind is SurfaceKind.NURBS:
        return _nurbs_surface(value)
    return UnsupportedGeometry(type_name=_str(value, "type_name"))


def _nurbs_surface(value: Mapping[str, Any]) -> NurbsSurface:
    return NurbsSurface(
        u_degree=_int(value, "u_degree"),
        v_degree=_int(value, "v_degree"),
        u_control_vertex_count=_int(value, "u_control_vertex_count"),
        v_control_vertex_count=_int(value, "v_control_vertex_count"),
        vertex_dimension=_int(value, "vertex_dimension"),
        u_knot_type=_int(value, "u_knot_type"),
        v_knot_type=_int(value, "v_knot_type"),
        u_periodic=_bool(value, "u_periodic"),
        v_periodic=_bool(value, "v_periodic"),
        u_closed=_bool(value, "u_closed"),
        v_closed=_bool(value, "v_closed"),
        rational=_bool(value, "rational"),
        surface_form=_int(value, "surface_form"),
        control_vertices=_float_matrix(value, "control_vertices"),
        u_knots=_float_tuple(value, "u_knots"),
        v_knots=_float_tuple(value, "v_knots"),
        u_knot_multiplicities=_int_tuple(value, "u_knot_multiplicities"),
        v_knot_multiplicities=_int_tuple(value, "v_knot_multiplicities"),
        sources=tuple(_source(item) for item in _list(value, "sources")),
    )


def _raw_mesh(value: Mapping[str, Any]) -> RawMeshData:
    normals = value.get("normals")
    normal_indices = value.get("normal_indices")
    return RawMeshData(
        mesh_index=_int(value, "mesh_index"),
        transform_index=_optional_int(value, "transform_index"),
        surface_sense=Sense(_str(value, "surface_sense")),
        psm_mesh_index=_int(value, "psm_mesh_index"),
        precision=_int(value, "precision"),
        normal_type=_int(value, "normal_type"),
        positions=_raw_vector_pool(_mapping(value.get("positions"), "mesh position pool")),
        normals=(
            None if normals is None else _raw_vector_pool(_mapping(normals, "mesh normal pool"))
        ),
        position_indices=_raw_integer_pool(
            _mapping(value.get("position_indices"), "mesh position index pool")
        ),
        normal_indices=(
            None
            if normal_indices is None
            else _raw_integer_pool(_mapping(normal_indices, "mesh normal index pool"))
        ),
    )


def _raw_vector_pool(value: Mapping[str, Any]) -> RawVectorPool:
    return RawVectorPool(
        encoding=_int(value, "encoding"),
        vector_count=_int(value, "vector_count"),
        capacity=_int(value, "capacity"),
        shift=_int(value, "shift"),
        values=_float_tuple(value, "values"),
    )


def _raw_integer_pool(value: Mapping[str, Any]) -> RawIntegerPool:
    return RawIntegerPool(
        integer_count=_int(value, "integer_count"),
        capacity=_int(value, "capacity"),
        shift=_int(value, "shift"),
        values=_int_tuple(value, "values"),
    )


def _mesh_transform(value: Mapping[str, Any]) -> MeshTransformNode:
    rotation = _float_tuple(value, "rotation")
    if len(rotation) != 9:
        raise RuntimeError("native mesh transform rotation is not nine floats")
    perspective = _optional_float_triplet(value, "perspective")
    raw_range = _int_tuple(value, "byte_range")
    if len(raw_range) != 2:
        raise RuntimeError("native mesh transform byte range is not a pair")
    precision = value.get("precision")
    parsed_precision = None
    if precision is not None:
        item = _mapping(precision, "mesh transform precision")
        rotation_correction = _float_tuple(item, "rotation_correction")
        translation_correction = _float_tuple(item, "translation_correction")
        if len(rotation_correction) != 9 or len(translation_correction) != 3:
            raise RuntimeError("native mesh transform precision has invalid correction sizes")
        parsed_precision = MeshTransformPrecision(
            index=_int(item, "index"),
            rotation_correction=rotation_correction,
            translation_correction=translation_correction,
        )
    return MeshTransformNode(
        index=_int(value, "index"),
        rotation=rotation,
        translation=_vector(value, "translation"),
        scale=_float(value, "scale"),
        flags=_int(value, "flags"),
        perspective=perspective,
        precision=parsed_precision,
        byte_range=(raw_range[0], raw_range[1]),
    )


def _optional_float_triplet(
    value: Mapping[str, Any], key: str
) -> tuple[float | None, float | None, float | None]:
    item = value.get(key)
    if (
        not isinstance(item, (tuple, list))
        or len(item) != 3
        or any(
            part is not None and (isinstance(part, bool) or not isinstance(part, float))
            for part in item
        )
    ):
        raise RuntimeError(f"native B-Rep field {key!r} is not an optional-float triple")
    return (item[0], item[1], item[2])


def _diagnostic(value: object, schema_key: str) -> Diagnostic:
    item = _mapping(value, "B-Rep diagnostic")
    source = _source(item.get("source"))
    details: dict[str, str | int | float | bool | None] = {
        "role": _str(item, "role"),
        "source_node_index": source.node_index,
        "source_type_name": source.type_name,
        "source_byte_end": source.byte_range.end,
    }
    if source.node_id is not None:
        details["source_node_id"] = source.node_id
    return Diagnostic(
        code=_str(item, "code"),
        severity=DiagnosticSeverity.WARNING,
        kind=DiagnosticKind.UNSUPPORTED,
        message=_str(item, "message"),
        location=SourceLocation(byte_offset=source.byte_range.start),
        node_type=source.node_type,
        node_id=source.node_id if source.node_id is not None and source.node_id >= 0 else None,
        schema_key=schema_key,
        fatal=False,
        details=details,
    )


def _source(value: object) -> SourceNodeRef:
    item = _mapping(value, "source node")
    byte_range = item.get("byte_range")
    if (
        not isinstance(byte_range, tuple)
        or len(byte_range) != 2
        or any(isinstance(part, bool) or not isinstance(part, int) for part in byte_range)
    ):
        raise RuntimeError("native source node byte range is invalid")
    return SourceNodeRef(
        node_index=_int(item, "node_index"),
        node_type=_int(item, "node_type"),
        type_name=_str(item, "type_name"),
        node_id=_optional_int(item, "node_id"),
        byte_range=ByteRange(start=byte_range[0], end=byte_range[1]),
    )


def _vector(value: Mapping[str, Any], key: str) -> Vector3:
    item = value.get(key)
    if (
        not isinstance(item, tuple)
        or len(item) != 3
        or any(isinstance(part, bool) or not isinstance(part, float) for part in item)
    ):
        raise RuntimeError(f"native B-Rep field {key!r} is not a three-float tuple")
    return Vector3(item[0], item[1], item[2])


def _optional_vector(value: Mapping[str, Any], key: str) -> Vector3 | None:
    if value.get(key) is None:
        return None
    return _vector(value, key)


def _mapping(value: object, name: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise RuntimeError(f"native B-Rep {name} is not a mapping")
    return value


def _list(value: Mapping[str, Any], key: str) -> list[object]:
    item = value.get(key)
    if not isinstance(item, list):
        raise RuntimeError(f"native B-Rep field {key!r} is not a list")
    return item


def _str(value: Mapping[str, Any], key: str) -> str:
    item = value.get(key)
    if not isinstance(item, str):
        raise RuntimeError(f"native B-Rep field {key!r} is not a string")
    return item


def _optional_str(value: Mapping[str, Any], key: str) -> str | None:
    item = value.get(key)
    if item is None:
        return None
    if not isinstance(item, str):
        raise RuntimeError(f"native B-Rep field {key!r} is not a string or None")
    return item


def _bool(value: Mapping[str, Any], key: str) -> bool:
    item = value.get(key)
    if not isinstance(item, bool):
        raise RuntimeError(f"native B-Rep field {key!r} is not a boolean")
    return item


def _int(value: Mapping[str, Any], key: str) -> int:
    item = value.get(key)
    if isinstance(item, bool) or not isinstance(item, int):
        raise RuntimeError(f"native B-Rep field {key!r} is not an integer")
    return item


def _optional_int(value: Mapping[str, Any], key: str) -> int | None:
    item = value.get(key)
    if item is None:
        return None
    if isinstance(item, bool) or not isinstance(item, int):
        raise RuntimeError(f"native B-Rep field {key!r} is not an integer or None")
    return item


def _float(value: Mapping[str, Any], key: str) -> float:
    item = value.get(key)
    if isinstance(item, bool) or not isinstance(item, float):
        raise RuntimeError(f"native B-Rep field {key!r} is not a float")
    return item


def _optional_float(value: Mapping[str, Any], key: str) -> float | None:
    item = value.get(key)
    if item is None:
        return None
    if isinstance(item, bool) or not isinstance(item, float):
        raise RuntimeError(f"native B-Rep field {key!r} is not a float or None")
    return item


def _int_tuple(value: Mapping[str, Any], key: str) -> tuple[int, ...]:
    item = value.get(key)
    if not isinstance(item, Sequence) or isinstance(item, (str, bytes, bytearray)):
        raise RuntimeError(f"native B-Rep field {key!r} is not an integer sequence")
    if any(isinstance(part, bool) or not isinstance(part, int) for part in item):
        raise RuntimeError(f"native B-Rep field {key!r} contains a non-integer")
    return tuple(item)


def _float_tuple(value: Mapping[str, Any], key: str) -> tuple[float, ...]:
    item = value.get(key)
    if not isinstance(item, list) or any(
        isinstance(part, bool) or not isinstance(part, float) for part in item
    ):
        raise RuntimeError(f"native B-Rep field {key!r} is not a float list")
    return tuple(item)


def _float_pair(value: Mapping[str, Any], key: str) -> tuple[float, float]:
    item = value.get(key)
    if (
        not isinstance(item, tuple)
        or len(item) != 2
        or any(isinstance(part, bool) or not isinstance(part, float) for part in item)
    ):
        raise RuntimeError(f"native B-Rep field {key!r} is not a two-float tuple")
    return (item[0], item[1])


def _optional_int_pair(value: Mapping[str, Any], key: str) -> tuple[int | None, int | None]:
    item = value.get(key)
    if (
        not isinstance(item, tuple)
        or len(item) != 2
        or any(
            part is not None and (isinstance(part, bool) or not isinstance(part, int))
            for part in item
        )
    ):
        raise RuntimeError(f"native B-Rep field {key!r} is not an optional-integer pair")
    return (item[0], item[1])


def _float_matrix(value: Mapping[str, Any], key: str) -> tuple[tuple[float, ...], ...]:
    rows = _list(value, key)
    result: list[tuple[float, ...]] = []
    for row in rows:
        if not isinstance(row, list) or any(
            isinstance(part, bool) or not isinstance(part, float) for part in row
        ):
            raise RuntimeError(f"native B-Rep field {key!r} is not a float matrix")
        result.append(tuple(row))
    return tuple(result)
