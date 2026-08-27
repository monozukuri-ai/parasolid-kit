//! Mechanical conversion of the Rust B-Rep source model to private Python values.

#![allow(clippy::too_many_lines)]

use parasolid_core::brep::{
    Body, BrepDiagnostic, BrepModel, BrepSourceFormat, CurveGeometry, CurveKind, Edge, Face,
    HalfEdge, Loop, NurbsCurve, NurbsSurface, PointGeometry, Region, Shell, SourceNodeRef,
    SurfaceGeometry, SurfaceKind, Vector3, Vertex, map_xb_brep_with_diagnostic_limit,
    map_xt_brep_with_diagnostic_limit,
};
use pyo3::{
    Bound, PyRef, PyResult, Python, pyfunction,
    types::{PyDict, PyDictMethods, PyList, PyListMethods},
};

use super::{NativeXbDocument, NativeXtDocument, error_to_python};

#[pyfunction(name = "_map_xb_brep")]
#[allow(clippy::needless_pass_by_value)]
pub(super) fn map_xb_brep_native<'py>(
    py: Python<'py>,
    document: PyRef<'_, NativeXbDocument>,
    max_diagnostics: usize,
) -> PyResult<Bound<'py, PyDict>> {
    brep_response(
        py,
        map_xb_brep_with_diagnostic_limit(&document.document, max_diagnostics),
    )
}

#[pyfunction(name = "_map_xt_brep")]
#[allow(clippy::needless_pass_by_value)]
pub(super) fn map_xt_brep_native<'py>(
    py: Python<'py>,
    document: PyRef<'_, NativeXtDocument>,
    max_diagnostics: usize,
) -> PyResult<Bound<'py, PyDict>> {
    brep_response(
        py,
        map_xt_brep_with_diagnostic_limit(&document.document, max_diagnostics),
    )
}

fn brep_response(
    py: Python<'_>,
    result: Result<BrepModel, parasolid_core::ParseError>,
) -> PyResult<Bound<'_, PyDict>> {
    let response = PyDict::new(py);
    match result {
        Ok(model) => {
            response.set_item("ok", true)?;
            response.set_item("value", model_to_python(py, &model)?)?;
        }
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("error", error_to_python(py, &error)?)?;
        }
    }
    Ok(response)
}

fn model_to_python<'py>(py: Python<'py>, model: &BrepModel) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item(
        "source_format",
        match model.source_format {
            BrepSourceFormat::Binary => "binary",
            BrepSourceFormat::Text => "text",
        },
    )?;
    value.set_item("schema_key", &model.schema_key)?;
    value.set_item("complete", model.complete)?;
    value.set_item("bodies", bodies_to_python(py, &model.bodies)?)?;
    value.set_item("regions", regions_to_python(py, &model.regions)?)?;
    value.set_item("shells", shells_to_python(py, &model.shells)?)?;
    value.set_item("faces", faces_to_python(py, &model.faces)?)?;
    value.set_item("loops", loops_to_python(py, &model.loops)?)?;
    value.set_item("half_edges", half_edges_to_python(py, &model.half_edges)?)?;
    value.set_item("edges", edges_to_python(py, &model.edges)?)?;
    value.set_item("vertices", vertices_to_python(py, &model.vertices)?)?;
    value.set_item("points", points_to_python(py, &model.points)?)?;
    value.set_item("curves", curves_to_python(py, &model.curves)?)?;
    value.set_item("surfaces", surfaces_to_python(py, &model.surfaces)?)?;

    let topology = PyDict::new(py);
    topology.set_item("valid", model.topology.valid)?;
    topology.set_item("closed_loop_count", model.topology.closed_loop_count)?;
    topology.set_item(
        "closed_edge_ring_count",
        model.topology.closed_edge_ring_count,
    )?;
    topology.set_item("euler_characteristic", model.topology.euler_characteristic)?;
    value.set_item("topology", topology)?;

    let metrics = PyDict::new(py);
    metrics.set_item(
        "bounding_box",
        model
            .metrics
            .bounding_box
            .map(|bounds| {
                let result = PyDict::new(py);
                result.set_item("minimum", vector_tuple(bounds.minimum))?;
                result.set_item("maximum", vector_tuple(bounds.maximum))?;
                Ok::<_, pyo3::PyErr>(result)
            })
            .transpose()?,
    )?;
    metrics.set_item("surface_area", model.metrics.surface_area)?;
    metrics.set_item("volume", model.metrics.volume)?;
    value.set_item("metrics", metrics)?;

    let diagnostics = PyList::empty(py);
    for diagnostic in &model.diagnostics {
        diagnostics.append(diagnostic_to_python(py, diagnostic)?)?;
    }
    value.set_item("diagnostics", diagnostics)?;
    Ok(value)
}

fn bodies_to_python<'py>(py: Python<'py>, values: &[Body]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for body in values {
        let value = PyDict::new(py);
        value.set_item("id", body.id)?;
        value.set_item("kind", body.kind.as_str())?;
        value.set_item("size_resolution", body.size_resolution)?;
        value.set_item("linear_resolution", body.linear_resolution)?;
        value.set_item("regions", &body.regions)?;
        value.set_item("edges", &body.edges)?;
        value.set_item("vertices", &body.vertices)?;
        value.set_item("source", source_to_python(py, &body.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn regions_to_python<'py>(py: Python<'py>, values: &[Region]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for region in values {
        let value = PyDict::new(py);
        value.set_item("id", region.id)?;
        value.set_item("kind", region.kind.as_str())?;
        value.set_item("body", region.body)?;
        value.set_item("shells", &region.shells)?;
        value.set_item("source", source_to_python(py, &region.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn shells_to_python<'py>(py: Python<'py>, values: &[Shell]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for shell in values {
        let value = PyDict::new(py);
        value.set_item("id", shell.id)?;
        value.set_item("region", shell.region)?;
        value.set_item("back_faces", &shell.back_faces)?;
        value.set_item("front_faces", &shell.front_faces)?;
        value.set_item("wire_edges", &shell.wire_edges)?;
        value.set_item("isolated_vertex", shell.isolated_vertex)?;
        value.set_item("source", source_to_python(py, &shell.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn faces_to_python<'py>(py: Python<'py>, values: &[Face]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for face in values {
        let value = PyDict::new(py);
        value.set_item("id", face.id)?;
        value.set_item("back_shell", face.back_shell)?;
        value.set_item("front_shell", face.front_shell)?;
        value.set_item("loops", &face.loops)?;
        value.set_item("surface", face.surface)?;
        value.set_item("sense", face.sense.as_str())?;
        value.set_item("source", source_to_python(py, &face.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn loops_to_python<'py>(py: Python<'py>, values: &[Loop]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for loop_value in values {
        let value = PyDict::new(py);
        value.set_item("id", loop_value.id)?;
        value.set_item("face", loop_value.face)?;
        value.set_item("half_edges", &loop_value.half_edges)?;
        value.set_item("source", source_to_python(py, &loop_value.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn half_edges_to_python<'py>(py: Python<'py>, values: &[HalfEdge]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for half_edge in values {
        let value = PyDict::new(py);
        value.set_item("id", half_edge.id)?;
        value.set_item("loop", half_edge.loop_id)?;
        value.set_item("forward", half_edge.forward)?;
        value.set_item("backward", half_edge.backward)?;
        value.set_item("vertex", half_edge.vertex)?;
        value.set_item("other", half_edge.other)?;
        value.set_item("edge", half_edge.edge)?;
        value.set_item("curve", half_edge.curve)?;
        value.set_item("sense", half_edge.sense.as_str())?;
        value.set_item("dummy", half_edge.dummy)?;
        value.set_item("source", source_to_python(py, &half_edge.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn edges_to_python<'py>(py: Python<'py>, values: &[Edge]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for edge in values {
        let value = PyDict::new(py);
        value.set_item("id", edge.id)?;
        value.set_item("owner", source_to_python(py, &edge.owner)?)?;
        value.set_item("half_edges", &edge.half_edges)?;
        value.set_item("start_vertex", edge.start_vertex)?;
        value.set_item("end_vertex", edge.end_vertex)?;
        value.set_item("curve", edge.curve)?;
        value.set_item("tolerance", edge.tolerance)?;
        value.set_item("source", source_to_python(py, &edge.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn vertices_to_python<'py>(py: Python<'py>, values: &[Vertex]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for vertex in values {
        let value = PyDict::new(py);
        value.set_item("id", vertex.id)?;
        value.set_item("point", vertex.point)?;
        value.set_item("tolerance", vertex.tolerance)?;
        value.set_item("owner", source_to_python(py, &vertex.owner)?)?;
        value.set_item("source", source_to_python(py, &vertex.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn points_to_python<'py>(
    py: Python<'py>,
    values: &[PointGeometry],
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for point in values {
        let value = PyDict::new(py);
        value.set_item("id", point.id)?;
        value.set_item("position", vector_tuple(point.position))?;
        value.set_item(
            "owner",
            point
                .owner
                .as_ref()
                .map(|source| source_to_python(py, source))
                .transpose()?,
        )?;
        value.set_item("source", source_to_python(py, &point.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn curves_to_python<'py>(
    py: Python<'py>,
    values: &[CurveGeometry],
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for curve in values {
        let value = PyDict::new(py);
        value.set_item("id", curve.id)?;
        value.set_item("sense", curve.sense.as_str())?;
        value.set_item(
            "owner",
            curve
                .owner
                .as_ref()
                .map(|source| source_to_python(py, source))
                .transpose()?,
        )?;
        value.set_item("kind", curve.kind.as_str())?;
        value.set_item("parameters", curve_kind_to_python(py, &curve.kind)?)?;
        value.set_item("source", source_to_python(py, &curve.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn curve_kind_to_python<'py>(py: Python<'py>, kind: &CurveKind) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    match kind {
        CurveKind::Line { point, direction } => {
            value.set_item("point", vector_tuple(*point))?;
            value.set_item("direction", vector_tuple(*direction))?;
        }
        CurveKind::Circle {
            center,
            normal,
            x_axis,
            radius,
        } => {
            value.set_item("center", vector_tuple(*center))?;
            value.set_item("normal", vector_tuple(*normal))?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
            value.set_item("radius", radius)?;
        }
        CurveKind::Ellipse {
            center,
            normal,
            x_axis,
            major_radius,
            minor_radius,
        } => {
            value.set_item("center", vector_tuple(*center))?;
            value.set_item("normal", vector_tuple(*normal))?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
            value.set_item("major_radius", major_radius)?;
            value.set_item("minor_radius", minor_radius)?;
        }
        CurveKind::Parabola {
            origin,
            normal,
            x_axis,
            focal_length,
        } => {
            value.set_item("origin", vector_tuple(*origin))?;
            value.set_item("normal", vector_tuple(*normal))?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
            value.set_item("focal_length", focal_length)?;
        }
        CurveKind::Hyperbola {
            origin,
            normal,
            x_axis,
            transverse_radius,
            conjugate_radius,
        } => {
            value.set_item("origin", vector_tuple(*origin))?;
            value.set_item("normal", vector_tuple(*normal))?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
            value.set_item("transverse_radius", transverse_radius)?;
            value.set_item("conjugate_radius", conjugate_radius)?;
        }
        CurveKind::Trimmed {
            basis_curve,
            start_point,
            end_point,
            start_parameter,
            end_parameter,
        } => {
            value.set_item("basis_curve", basis_curve)?;
            value.set_item("start_point", vector_tuple(*start_point))?;
            value.set_item("end_point", vector_tuple(*end_point))?;
            value.set_item("start_parameter", start_parameter)?;
            value.set_item("end_parameter", end_parameter)?;
        }
        CurveKind::Nurbs(nurbs) => nurbs_curve_to_python(py, &value, nurbs)?,
        CurveKind::SurfaceParametric {
            surface,
            parameter_curve,
            original_curve,
            tolerance_to_original,
        } => {
            value.set_item("surface", surface)?;
            value.set_item("parameter_curve", parameter_curve)?;
            value.set_item("original_curve", original_curve)?;
            value.set_item("tolerance_to_original", tolerance_to_original)?;
        }
        CurveKind::Intersection {
            surfaces,
            chart,
            start,
            end,
            intersection_data,
        } => {
            value.set_item("surfaces", (surfaces[0], surfaces[1]))?;
            value.set_item("chart", source_to_python(py, chart)?)?;
            value.set_item("start", source_to_python(py, start)?)?;
            value.set_item("end", source_to_python(py, end)?)?;
            value.set_item(
                "intersection_data",
                intersection_data
                    .as_ref()
                    .map(|source| source_to_python(py, source))
                    .transpose()?,
            )?;
        }
        CurveKind::Unsupported { type_name } => value.set_item("type_name", type_name)?,
    }
    Ok(value)
}

fn nurbs_curve_to_python(
    py: Python<'_>,
    value: &Bound<'_, PyDict>,
    nurbs: &NurbsCurve,
) -> PyResult<()> {
    value.set_item("degree", nurbs.degree)?;
    value.set_item("control_vertex_count", nurbs.control_vertex_count)?;
    value.set_item("vertex_dimension", nurbs.vertex_dimension)?;
    value.set_item("knot_type", nurbs.knot_type)?;
    value.set_item("periodic", nurbs.periodic)?;
    value.set_item("closed", nurbs.closed)?;
    value.set_item("rational", nurbs.rational)?;
    value.set_item("curve_form", nurbs.curve_form)?;
    value.set_item("control_vertices", &nurbs.control_vertices)?;
    value.set_item("knots", &nurbs.knots)?;
    value.set_item("knot_multiplicities", &nurbs.knot_multiplicities)?;
    let sources = PyList::empty(py);
    for source in &nurbs.sources {
        sources.append(source_to_python(py, source)?)?;
    }
    value.set_item("sources", sources)?;
    Ok(())
}

fn surfaces_to_python<'py>(
    py: Python<'py>,
    values: &[SurfaceGeometry],
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for surface in values {
        let value = PyDict::new(py);
        value.set_item("id", surface.id)?;
        value.set_item("sense", surface.sense.as_str())?;
        value.set_item(
            "owner",
            surface
                .owner
                .as_ref()
                .map(|source| source_to_python(py, source))
                .transpose()?,
        )?;
        value.set_item("kind", surface.kind.as_str())?;
        value.set_item("parameters", surface_kind_to_python(py, &surface.kind)?)?;
        value.set_item("source", source_to_python(py, &surface.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn surface_kind_to_python<'py>(
    py: Python<'py>,
    kind: &SurfaceKind,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    match kind {
        SurfaceKind::Plane {
            point,
            normal,
            x_axis,
        } => {
            value.set_item("point", vector_tuple(*point))?;
            value.set_item("normal", vector_tuple(*normal))?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
        }
        SurfaceKind::Cylinder {
            point,
            axis,
            radius,
            x_axis,
        } => {
            value.set_item("point", vector_tuple(*point))?;
            value.set_item("axis", vector_tuple(*axis))?;
            value.set_item("radius", radius)?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
        }
        SurfaceKind::Cone {
            point,
            axis,
            radius,
            sin_half_angle,
            cos_half_angle,
            x_axis,
        } => {
            value.set_item("point", vector_tuple(*point))?;
            value.set_item("axis", vector_tuple(*axis))?;
            value.set_item("radius", radius)?;
            value.set_item("sin_half_angle", sin_half_angle)?;
            value.set_item("cos_half_angle", cos_half_angle)?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
        }
        SurfaceKind::Sphere {
            center,
            radius,
            axis,
            x_axis,
        } => {
            value.set_item("center", vector_tuple(*center))?;
            value.set_item("radius", radius)?;
            value.set_item("axis", vector_tuple(*axis))?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
        }
        SurfaceKind::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            x_axis,
        } => {
            value.set_item("center", vector_tuple(*center))?;
            value.set_item("axis", vector_tuple(*axis))?;
            value.set_item("major_radius", major_radius)?;
            value.set_item("minor_radius", minor_radius)?;
            value.set_item("x_axis", vector_tuple(*x_axis))?;
        }
        SurfaceKind::BlendedEdge {
            blend_type,
            supporting_surfaces,
            spine_curve,
            ranges,
            thumb_weights,
            boundary_surfaces,
            start,
            end,
        } => {
            value.set_item("blend_type", blend_type.as_str())?;
            value.set_item(
                "supporting_surfaces",
                (supporting_surfaces[0], supporting_surfaces[1]),
            )?;
            value.set_item("spine_curve", spine_curve)?;
            value.set_item("ranges", (ranges[0], ranges[1]))?;
            value.set_item("thumb_weights", (thumb_weights[0], thumb_weights[1]))?;
            value.set_item(
                "boundary_surfaces",
                (boundary_surfaces[0], boundary_surfaces[1]),
            )?;
            value.set_item(
                "start",
                start
                    .as_ref()
                    .map(|source| source_to_python(py, source))
                    .transpose()?,
            )?;
            value.set_item(
                "end",
                end.as_ref()
                    .map(|source| source_to_python(py, source))
                    .transpose()?,
            )?;
        }
        SurfaceKind::BlendBoundary {
            boundary_index,
            blend_surface,
        } => {
            value.set_item("boundary_index", boundary_index)?;
            value.set_item("blend_surface", blend_surface)?;
        }
        SurfaceKind::Offset {
            basis_surface,
            offset,
        } => {
            value.set_item("basis_surface", basis_surface)?;
            value.set_item("offset", offset)?;
        }
        SurfaceKind::Nurbs(nurbs) => nurbs_surface_to_python(py, &value, nurbs)?,
        SurfaceKind::Unsupported { type_name } => value.set_item("type_name", type_name)?,
    }
    Ok(value)
}

fn nurbs_surface_to_python(
    py: Python<'_>,
    value: &Bound<'_, PyDict>,
    nurbs: &NurbsSurface,
) -> PyResult<()> {
    value.set_item("u_degree", nurbs.u_degree)?;
    value.set_item("v_degree", nurbs.v_degree)?;
    value.set_item("u_control_vertex_count", nurbs.u_control_vertex_count)?;
    value.set_item("v_control_vertex_count", nurbs.v_control_vertex_count)?;
    value.set_item("vertex_dimension", nurbs.vertex_dimension)?;
    value.set_item("u_knot_type", nurbs.u_knot_type)?;
    value.set_item("v_knot_type", nurbs.v_knot_type)?;
    value.set_item("u_periodic", nurbs.u_periodic)?;
    value.set_item("v_periodic", nurbs.v_periodic)?;
    value.set_item("u_closed", nurbs.u_closed)?;
    value.set_item("v_closed", nurbs.v_closed)?;
    value.set_item("rational", nurbs.rational)?;
    value.set_item("surface_form", nurbs.surface_form)?;
    value.set_item("control_vertices", &nurbs.control_vertices)?;
    value.set_item("u_knots", &nurbs.u_knots)?;
    value.set_item("v_knots", &nurbs.v_knots)?;
    value.set_item("u_knot_multiplicities", &nurbs.u_knot_multiplicities)?;
    value.set_item("v_knot_multiplicities", &nurbs.v_knot_multiplicities)?;
    let sources = PyList::empty(py);
    for source in &nurbs.sources {
        sources.append(source_to_python(py, source)?)?;
    }
    value.set_item("sources", sources)?;
    Ok(())
}

fn diagnostic_to_python<'py>(
    py: Python<'py>,
    diagnostic: &BrepDiagnostic,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("code", diagnostic.code)?;
    value.set_item("message", &diagnostic.message)?;
    value.set_item("role", diagnostic.role)?;
    value.set_item("source", source_to_python(py, &diagnostic.source)?)?;
    Ok(value)
}

fn source_to_python<'py>(py: Python<'py>, source: &SourceNodeRef) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("node_index", source.node_index)?;
    value.set_item("node_type", source.node_type)?;
    value.set_item("type_name", &source.type_name)?;
    value.set_item("node_id", source.node_id)?;
    value.set_item(
        "byte_range",
        (source.byte_range.start, source.byte_range.end),
    )?;
    Ok(value)
}

const fn vector_tuple(value: Vector3) -> (f64, f64, f64) {
    (value.x, value.y, value.z)
}
