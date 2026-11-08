//! Mechanical conversion of the Rust B-Rep source model to private Python values.

#![allow(clippy::too_many_lines)]

use parasolid_core::brep::{
    Assembly, Attribute, AttributeField, AttributeOwner, Body, BrepDiagnostic, BrepModel,
    BrepSourceFormat, CurveGeometry, CurveKind, Edge, Face, HalfEdge, Instance, InstancePart,
    IntersectionData, IntersectionLimitKind, IntersectionUvType, Loop, NurbsCurve, NurbsSurface,
    PointGeometry, PolylineData, RawIntegerPool, RawMeshData, RawVectorPool, Region, Shell,
    SourceNodeRef, SurfaceGeometry, SurfaceKind, Transform, TransformNode, Vector3, Vertex,
    map_xb_brep_with_diagnostic_limit, map_xt_brep_with_diagnostic_limit,
};
use pyo3::{
    Bound, Py, PyRef, PyResult, Python, pyfunction,
    types::{PyDict, PyDictMethods, PyList, PyListMethods},
};

use super::{CatalogType, NativeXbDocument, NativeXtDocument, error_to_python};

/// Parse and map an `X_B` stream in one call, without materializing its nodes.
///
/// The response carries the document summary (with the native handle), the
/// B-Rep model and, on failure, the `stage` (`parse` or `brep`) of the error.
#[pyfunction(name = "_read_xb_brep", signature = (data, schema_id, definitions, max_file_size, max_nodes, max_schema_types, max_fields_per_type, max_string_bytes, max_variable_elements, max_diagnostics, allow_builtin=false))]
#[allow(clippy::too_many_arguments)]
pub(super) fn read_xb_brep_native<'py>(
    py: Python<'py>,
    data: &[u8],
    schema_id: Option<String>,
    definitions: Option<Vec<CatalogType>>,
    max_file_size: usize,
    max_nodes: usize,
    max_schema_types: usize,
    max_fields_per_type: usize,
    max_string_bytes: usize,
    max_variable_elements: usize,
    max_diagnostics: usize,
    allow_builtin: bool,
) -> PyResult<Bound<'py, PyDict>> {
    let limits = super::document_limits(
        max_file_size,
        max_nodes,
        max_schema_types,
        max_fields_per_type,
        max_string_bytes,
        max_variable_elements,
    );
    let response = PyDict::new(py);
    match super::parse_xb_outcome(py, data, schema_id, definitions, limits, allow_builtin)? {
        Ok(document) => match map_xb_brep_with_diagnostic_limit(&document, max_diagnostics) {
            Ok(model) => {
                let value = PyDict::new(py);
                let summary = super::document_to_python(py, &document)?;
                value.set_item("brep", model_to_python(py, &model)?)?;
                summary.set_item(
                    "native_document",
                    Py::new(py, NativeXbDocument { document })?,
                )?;
                value.set_item("document", summary)?;
                response.set_item("ok", true)?;
                response.set_item("value", value)?;
            }
            Err(error) => {
                response.set_item("ok", false)?;
                response.set_item("stage", "brep")?;
                response.set_item("error", error_to_python(py, &error)?)?;
            }
        },
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("stage", "parse")?;
            response.set_item("error", error)?;
        }
    }
    Ok(response)
}

/// Parse and map an `X_T` stream like [`read_xb_brep_native`].
#[pyfunction(name = "_read_xt_brep", signature = (data, schema_id, definitions, max_file_size, max_nodes, max_schema_types, max_fields_per_type, max_string_bytes, max_variable_elements, max_diagnostics, allow_builtin=false))]
#[allow(clippy::too_many_arguments)]
pub(super) fn read_xt_brep_native<'py>(
    py: Python<'py>,
    data: &[u8],
    schema_id: Option<String>,
    definitions: Option<Vec<CatalogType>>,
    max_file_size: usize,
    max_nodes: usize,
    max_schema_types: usize,
    max_fields_per_type: usize,
    max_string_bytes: usize,
    max_variable_elements: usize,
    max_diagnostics: usize,
    allow_builtin: bool,
) -> PyResult<Bound<'py, PyDict>> {
    let limits = super::document_limits(
        max_file_size,
        max_nodes,
        max_schema_types,
        max_fields_per_type,
        max_string_bytes,
        max_variable_elements,
    );
    let response = PyDict::new(py);
    match super::parse_xt_outcome(py, data, schema_id, definitions, limits, allow_builtin)? {
        Ok(document) => match map_xt_brep_with_diagnostic_limit(&document, max_diagnostics) {
            Ok(model) => {
                let value = PyDict::new(py);
                let summary = super::xt_document_to_python(py, &document)?;
                value.set_item("brep", model_to_python(py, &model)?)?;
                summary.set_item(
                    "native_document",
                    Py::new(py, NativeXtDocument { document })?,
                )?;
                value.set_item("document", summary)?;
                response.set_item("ok", true)?;
                response.set_item("value", value)?;
            }
            Err(error) => {
                response.set_item("ok", false)?;
                response.set_item("stage", "brep")?;
                response.set_item("error", error_to_python(py, &error)?)?;
            }
        },
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("stage", "parse")?;
            response.set_item("error", error)?;
        }
    }
    Ok(response)
}

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
    value.set_item("assemblies", assemblies_to_python(py, &model.assemblies)?)?;
    value.set_item("instances", instances_to_python(py, &model.instances)?)?;
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
    value.set_item("attributes", attributes_to_python(py, &model.attributes)?)?;

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

fn assemblies_to_python<'py>(py: Python<'py>, values: &[Assembly]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for assembly in values {
        let value = PyDict::new(py);
        value.set_item("id", assembly.id)?;
        value.set_item("kind", assembly.kind.as_str())?;
        value.set_item("size_resolution", assembly.size_resolution)?;
        value.set_item("linear_resolution", assembly.linear_resolution)?;
        value.set_item("instances", &assembly.instances)?;
        value.set_item("referencing_instances", &assembly.referencing_instances)?;
        value.set_item("source", source_to_python(py, &assembly.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn instances_to_python<'py>(py: Python<'py>, values: &[Instance]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for instance in values {
        let value = PyDict::new(py);
        value.set_item("id", instance.id)?;
        value.set_item("negative", instance.negative)?;
        let (part_kind, part) = match instance.part {
            InstancePart::Body(id) => ("body", id),
            InstancePart::Assembly(id) => ("assembly", id),
        };
        value.set_item("part_kind", part_kind)?;
        value.set_item("part", part)?;
        value.set_item(
            "transform",
            instance
                .transform
                .as_ref()
                .map(|transform| transform_to_python(py, transform))
                .transpose()?,
        )?;
        value.set_item("assembly", instance.assembly)?;
        value.set_item("source", source_to_python(py, &instance.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn transform_to_python<'py>(
    py: Python<'py>,
    transform: &Transform,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("rotation", transform.rotation)?;
    value.set_item("translation", vector_tuple(transform.translation))?;
    value.set_item("scale", transform.scale)?;
    value.set_item("flags", transform.flags)?;
    value.set_item("perspective", vector_tuple(transform.perspective))?;
    value.set_item("source", source_to_python(py, &transform.source)?)?;
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
        value.set_item("attributes", &body.attributes)?;
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
        value.set_item("attributes", &face.attributes)?;
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
        CurveKind::Polyline(polyline) => polyline_to_python(py, &value, polyline)?,
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
            chart_parameters,
            chart_points,
            start,
            start_limit_kind,
            start_term_use,
            start_points,
            end,
            end_limit_kind,
            end_term_use,
            end_points,
            intersection_uv,
            intersection_data,
        } => {
            value.set_item("surfaces", (surfaces[0], surfaces[1]))?;
            value.set_item("chart", source_to_python(py, chart)?)?;
            let parameters = PyDict::new(py);
            parameters.set_item("base_parameter", chart_parameters.base_parameter)?;
            parameters.set_item("base_scale", chart_parameters.base_scale)?;
            parameters.set_item("chordal_error", chart_parameters.chordal_error)?;
            parameters.set_item("angular_error", chart_parameters.angular_error)?;
            value.set_item("chart_parameters", parameters)?;
            value.set_item(
                "start_limit_kind",
                start_limit_kind.map(intersection_limit_kind_name),
            )?;
            value.set_item("start_term_use", start_term_use)?;
            value.set_item(
                "end_limit_kind",
                end_limit_kind.map(intersection_limit_kind_name),
            )?;
            value.set_item("end_term_use", end_term_use)?;
            value.set_item(
                "intersection_uv",
                intersection_uv
                    .as_ref()
                    .map(|data| intersection_data_to_python(py, data))
                    .transpose()?,
            )?;
            for (name, points) in [
                ("chart_points", chart_points),
                ("start_points", start_points),
                ("end_points", end_points),
            ] {
                value.set_item(
                    name,
                    points
                        .iter()
                        .map(|point| vector_tuple(*point))
                        .collect::<Vec<_>>(),
                )?;
            }
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

fn polyline_to_python(
    py: Python<'_>,
    value: &Bound<'_, PyDict>,
    polyline: &PolylineData,
) -> PyResult<()> {
    value.set_item(
        "points",
        polyline
            .points
            .iter()
            .map(|point| vector_tuple(*point))
            .collect::<Vec<_>>(),
    )?;
    value.set_item("closed", polyline.closed)?;
    value.set_item("sense", polyline.sense.as_str())?;
    value.set_item("base_parameter", polyline.base_parameter)?;
    value.set_item("source", source_to_python(py, &polyline.source)?)?;
    Ok(())
}

fn intersection_limit_kind_name(kind: IntersectionLimitKind) -> &'static str {
    match kind {
        IntersectionLimitKind::Help => "help",
        IntersectionLimitKind::Limit => "limit",
        IntersectionLimitKind::Terminator => "terminator",
        IntersectionLimitKind::Boundary => "boundary",
    }
}

fn intersection_data_to_python<'py>(
    py: Python<'py>,
    data: &IntersectionData,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    let uv_type = match data.uv_type {
        IntersectionUvType::None => "none".to_owned(),
        IntersectionUvType::First => "first".to_owned(),
        IntersectionUvType::Second => "second".to_owned(),
        IntersectionUvType::Both => "both".to_owned(),
        IntersectionUvType::Unsupported(raw) => format!("unsupported:{raw}"),
    };
    value.set_item("source", source_to_python(py, &data.source)?)?;
    value.set_item("uv_type", uv_type)?;
    value.set_item("raw_values", &data.raw_values)?;
    value.set_item("start_is_terminator", data.start_is_terminator)?;
    value.set_item(
        "start_terminator",
        data.start_terminator
            .map(intersection_coordinates_to_python),
    )?;
    value.set_item("end_is_terminator", data.end_is_terminator)?;
    value.set_item(
        "chart",
        data.chart.as_ref().map(|coords| {
            coords
                .iter()
                .copied()
                .map(intersection_coordinates_to_python)
                .collect::<Vec<_>>()
        }),
    )?;
    value.set_item(
        "end_terminator",
        data.end_terminator.map(intersection_coordinates_to_python),
    )?;
    Ok(value)
}

type PythonSurfaceUv = Option<(Option<f64>, Option<f64>)>;

fn intersection_coordinates_to_python(
    coordinates: parasolid_core::brep::IntersectionUvCoordinates,
) -> (PythonSurfaceUv, PythonSurfaceUv) {
    (
        coordinates.first_surface.map(|[u, v]| (u, v)),
        coordinates.second_surface.map(|[u, v]| (u, v)),
    )
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
        SurfaceKind::XtCone {
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
        SurfaceKind::Spun {
            profile,
            base,
            axis,
            start,
            end,
            start_parameter,
            end_parameter,
            x_axis,
        } => {
            value.set_item("profile", profile)?;
            value.set_item("base", vector_tuple(*base))?;
            value.set_item("axis", vector_tuple(*axis))?;
            value.set_item("start", start.map(vector_tuple))?;
            value.set_item("end", end.map(vector_tuple))?;
            value.set_item("start_parameter", start_parameter)?;
            value.set_item("end_parameter", end_parameter)?;
            value.set_item("x_axis", x_axis.map(vector_tuple))?;
        }
        SurfaceKind::Swept { section, direction } => {
            value.set_item("section", section)?;
            value.set_item("direction", vector_tuple(*direction))?;
        }
        SurfaceKind::SweptWithSource {
            section,
            sweep,
            scale,
        } => {
            value.set_item("section", section)?;
            value.set_item("direction", vector_tuple(*sweep))?;
            value.set_item("scale", scale)?;
        }
        SurfaceKind::Offset {
            basis_surface,
            offset,
        } => {
            value.set_item("basis_surface", basis_surface)?;
            value.set_item("offset", offset)?;
        }
        SurfaceKind::Nurbs(nurbs) => nurbs_surface_to_python(py, &value, nurbs)?,
        SurfaceKind::Mesh {
            source_mesh_index,
            embedded,
            transform_index,
            transform,
            unembedded_source_fields,
        } => {
            value.set_item("source_mesh_index", source_mesh_index)?;
            value.set_item(
                "embedded",
                embedded
                    .as_ref()
                    .map(|mesh| raw_mesh_to_python(py, mesh))
                    .transpose()?,
            )?;
            value.set_item("transform_index", transform_index)?;
            value.set_item(
                "transform",
                transform
                    .as_ref()
                    .map(|transform| transform_node_to_python(py, transform))
                    .transpose()?,
            )?;
            value.set_item(
                "unembedded_source_fields",
                unembedded_source_fields
                    .as_ref()
                    .map(|fields| {
                        fields
                            .iter()
                            .map(|field| super::raw_field_to_python(py, field))
                            .collect::<PyResult<Vec<_>>>()
                    })
                    .transpose()?,
            )?;
        }
        SurfaceKind::Unsupported { type_name } => value.set_item("type_name", type_name)?,
    }
    Ok(value)
}

fn raw_mesh_to_python<'py>(py: Python<'py>, mesh: &RawMeshData) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("mesh_index", mesh.mesh_index)?;
    value.set_item("transform_index", mesh.transform_index)?;
    value.set_item("surface_sense", mesh.surface_sense.as_str())?;
    value.set_item("psm_mesh_index", mesh.psm_mesh_index)?;
    value.set_item("precision", mesh.precision)?;
    value.set_item("normal_type", mesh.normal_type)?;
    value.set_item("positions", raw_vector_pool_to_python(py, &mesh.positions)?)?;
    value.set_item(
        "normals",
        mesh.normals
            .as_ref()
            .map(|pool| raw_vector_pool_to_python(py, pool))
            .transpose()?,
    )?;
    value.set_item(
        "position_indices",
        raw_integer_pool_to_python(py, &mesh.position_indices)?,
    )?;
    value.set_item(
        "normal_indices",
        mesh.normal_indices
            .as_ref()
            .map(|pool| raw_integer_pool_to_python(py, pool))
            .transpose()?,
    )?;
    Ok(value)
}

fn raw_vector_pool_to_python<'py>(
    py: Python<'py>,
    pool: &RawVectorPool,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("encoding", pool.encoding)?;
    value.set_item("vector_count", pool.vector_count)?;
    value.set_item("capacity", pool.capacity)?;
    value.set_item("shift", pool.shift)?;
    value.set_item("values", &pool.values)?;
    Ok(value)
}

fn raw_integer_pool_to_python<'py>(
    py: Python<'py>,
    pool: &RawIntegerPool,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("integer_count", pool.integer_count)?;
    value.set_item("capacity", pool.capacity)?;
    value.set_item("shift", pool.shift)?;
    value.set_item("values", &pool.values)?;
    Ok(value)
}

fn transform_node_to_python<'py>(
    py: Python<'py>,
    transform: &TransformNode,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("index", transform.index)?;
    value.set_item("rotation", transform.rotation)?;
    value.set_item("translation", transform.translation)?;
    value.set_item("scale", transform.scale)?;
    value.set_item("flags", transform.flags)?;
    value.set_item("perspective", transform.perspective)?;
    value.set_item(
        "precision",
        transform
            .precision
            .as_ref()
            .map(|precision| {
                let fields = PyDict::new(py);
                fields.set_item("index", precision.index)?;
                fields.set_item("rotation_correction", precision.rotation_correction)?;
                fields.set_item("translation_correction", precision.translation_correction)?;
                Ok::<_, pyo3::PyErr>(fields)
            })
            .transpose()?,
    )?;
    value.set_item(
        "byte_range",
        (transform.byte_range.start, transform.byte_range.end),
    )?;
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

fn attributes_to_python<'py>(
    py: Python<'py>,
    values: &[Attribute],
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for attribute in values {
        let value = PyDict::new(py);
        value.set_item("id", attribute.id)?;
        value.set_item("identifier", &attribute.identifier)?;
        value.set_item("kind_id", attribute.kind_id)?;
        let owner = PyDict::new(py);
        owner.set_item("kind", attribute.owner.kind())?;
        owner.set_item("id", attribute.owner.id())?;
        owner.set_item(
            "source",
            match &attribute.owner {
                AttributeOwner::Other(source) => Some(source_to_python(py, source)?),
                _ => None,
            },
        )?;
        value.set_item("owner", owner)?;
        let fields = PyList::empty(py);
        for field in &attribute.fields {
            let item = PyDict::new(py);
            match field {
                AttributeField::Integers(values) => {
                    item.set_item("kind", "integers")?;
                    item.set_item("values", values)?;
                }
                AttributeField::Reals(values) => {
                    item.set_item("kind", "reals")?;
                    item.set_item("values", values)?;
                }
                AttributeField::Text(text) => {
                    item.set_item("kind", "text")?;
                    item.set_item("value", text)?;
                }
                AttributeField::Unicode(text) => {
                    item.set_item("kind", "unicode")?;
                    item.set_item("value", text)?;
                }
                AttributeField::Points(values) => {
                    item.set_item("kind", "points")?;
                    item.set_item("values", vector_values(values))?;
                }
                AttributeField::Vectors(values) => {
                    item.set_item("kind", "vectors")?;
                    item.set_item("values", vector_values(values))?;
                }
                AttributeField::Directions(values) => {
                    item.set_item("kind", "directions")?;
                    item.set_item("values", vector_values(values))?;
                }
                AttributeField::Axes(values) => {
                    item.set_item("kind", "axes")?;
                    item.set_item("values", vector_values(values))?;
                }
                AttributeField::Tags(values) => {
                    item.set_item("kind", "tags")?;
                    item.set_item("values", values)?;
                }
                AttributeField::Unsupported {
                    type_name,
                    node_index,
                } => {
                    item.set_item("kind", "unsupported")?;
                    item.set_item("type_name", type_name)?;
                    item.set_item("node_index", node_index)?;
                }
            }
            fields.append(item)?;
        }
        value.set_item("fields", fields)?;
        value.set_item("source", source_to_python(py, &attribute.source)?)?;
        result.append(value)?;
    }
    Ok(result)
}

fn vector_values(values: &[Option<Vector3>]) -> Vec<Option<(f64, f64, f64)>> {
    values.iter().map(|value| value.map(vector_tuple)).collect()
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
