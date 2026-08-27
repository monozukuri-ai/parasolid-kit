//! Private `PyO3` conversion layer for `parasolid_kit`.

mod brep;

use parasolid_core::{
    ComparisonDifference, ComparisonOptions, DocumentComparison, DocumentLimits, ErrorDetails,
    FieldDefinition, FieldType, FieldValue, InMemorySchemaProvider, InspectionLimits, ParseError,
    ParsedSchemaCatalog, RawField, RawNode, SchemaCatalogLimits, SchemaCoverageReport, SchemaEdit,
    SchemaKey, SchemaLimits, SchemaResolution, SchemaSource, TypeDefinition, XbDocument, XbHeader,
    XbTermination, XtDocument, XtHeader, XtTermination, decode_embedded_schema,
};
use pyo3::{
    Bound, Py, PyRef, PyResult, Python,
    exceptions::PyValueError,
    pyclass, pyfunction, pymodule,
    types::{PyBytes, PyDict, PyDictMethods, PyList, PyListMethods, PyModule, PyModuleMethods},
    wrap_pyfunction,
};

type BaseField = (String, String, u16, u32, bool);
type CatalogType = (u16, String, String, Vec<BaseField>);

#[pyclass(name = "_NativeXbDocument", frozen)]
pub(crate) struct NativeXbDocument {
    pub(crate) document: XbDocument,
}

#[pyclass(name = "_NativeXtDocument", frozen)]
pub(crate) struct NativeXtDocument {
    pub(crate) document: XtDocument,
}

#[pyfunction(name = "_inspect_xb")]
fn inspect_xb<'py>(
    py: Python<'py>,
    data: &[u8],
    max_file_size: usize,
    max_string_bytes: usize,
) -> PyResult<Bound<'py, PyDict>> {
    let response = PyDict::new(py);
    match parasolid_core::inspect_xb(
        data,
        InspectionLimits {
            max_file_size,
            max_string_bytes,
        },
    ) {
        Ok(header) => {
            response.set_item("ok", true)?;
            response.set_item("value", header_to_python(py, &header)?)?;
        }
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("error", error_to_python(py, &error)?)?;
        }
    }
    Ok(response)
}

#[pyfunction(name = "_inspect_xt")]
fn inspect_xt<'py>(
    py: Python<'py>,
    data: &[u8],
    max_file_size: usize,
    max_string_bytes: usize,
) -> PyResult<Bound<'py, PyDict>> {
    let response = PyDict::new(py);
    match parasolid_core::inspect_xt(
        data,
        InspectionLimits {
            max_file_size,
            max_string_bytes,
        },
    ) {
        Ok(header) => {
            response.set_item("ok", true)?;
            response.set_item("value", xt_header_to_python(py, &header)?)?;
        }
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("error", error_to_python(py, &error)?)?;
        }
    }
    Ok(response)
}

#[pyfunction(name = "_parse_schema_catalog")]
fn parse_schema_catalog_native<'py>(
    py: Python<'py>,
    data: &[u8],
    max_file_size: usize,
    max_schema_types: usize,
    max_fields_per_type: usize,
    max_string_bytes: usize,
) -> PyResult<Bound<'py, PyDict>> {
    let response = PyDict::new(py);
    match parasolid_core::parse_schema_catalog(
        data,
        SchemaCatalogLimits {
            max_file_size,
            max_schema_types,
            max_fields_per_type,
            max_string_bytes,
        },
    ) {
        Ok(catalog) => {
            response.set_item("ok", true)?;
            response.set_item("value", catalog_to_python(py, &catalog)?)?;
        }
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("error", error_to_python(py, &error)?)?;
        }
    }
    Ok(response)
}

#[pyfunction(name = "_parse_xb")]
#[allow(clippy::too_many_arguments)]
fn parse_xb<'py>(
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
) -> PyResult<Bound<'py, PyDict>> {
    let provider = provider_from_python(schema_id, definitions)?;
    let response = PyDict::new(py);
    match parasolid_core::parse_xb(
        data,
        &provider,
        DocumentLimits {
            max_file_size,
            max_nodes,
            max_schema_types,
            max_fields_per_type,
            max_string_bytes,
            max_variable_elements,
        },
    ) {
        Ok(document) => {
            let value = document_to_python(py, &document)?;
            value.set_item(
                "native_document",
                Py::new(py, NativeXbDocument { document })?,
            )?;
            response.set_item("ok", true)?;
            response.set_item("value", value)?;
        }
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("error", error_to_python(py, &error)?)?;
        }
    }
    Ok(response)
}

#[pyfunction(name = "_parse_xt")]
#[allow(clippy::too_many_arguments)]
fn parse_xt<'py>(
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
) -> PyResult<Bound<'py, PyDict>> {
    let provider = provider_from_python(schema_id, definitions)?;
    let response = PyDict::new(py);
    match parasolid_core::parse_xt(
        data,
        &provider,
        DocumentLimits {
            max_file_size,
            max_nodes,
            max_schema_types,
            max_fields_per_type,
            max_string_bytes,
            max_variable_elements,
        },
    ) {
        Ok(document) => {
            let value = xt_document_to_python(py, &document)?;
            value.set_item(
                "native_document",
                Py::new(py, NativeXtDocument { document })?,
            )?;
            response.set_item("ok", true)?;
            response.set_item("value", value)?;
        }
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("error", error_to_python(py, &error)?)?;
        }
    }
    Ok(response)
}

#[pyfunction(name = "_write_xb")]
#[allow(clippy::needless_pass_by_value)]
fn write_xb<'py>(py: Python<'py>, document: PyRef<'_, NativeXbDocument>) -> Bound<'py, PyBytes> {
    let output = parasolid_core::write_xb(&document.document);
    PyBytes::new(py, &output)
}

#[pyfunction(name = "_compare_xb_xb")]
#[allow(clippy::needless_pass_by_value)]
fn compare_xb_xb<'py>(
    py: Python<'py>,
    left: PyRef<'_, NativeXbDocument>,
    right: PyRef<'_, NativeXbDocument>,
    absolute_tolerance: f64,
    relative_tolerance: f64,
    max_differences: usize,
) -> PyResult<Bound<'py, PyDict>> {
    comparison_response(
        py,
        parasolid_core::compare_xb_documents(
            &left.document,
            &right.document,
            ComparisonOptions {
                absolute_tolerance,
                relative_tolerance,
                max_differences,
            },
        ),
    )
}

#[pyfunction(name = "_compare_xt_xt")]
#[allow(clippy::needless_pass_by_value)]
fn compare_xt_xt<'py>(
    py: Python<'py>,
    left: PyRef<'_, NativeXtDocument>,
    right: PyRef<'_, NativeXtDocument>,
    absolute_tolerance: f64,
    relative_tolerance: f64,
    max_differences: usize,
) -> PyResult<Bound<'py, PyDict>> {
    comparison_response(
        py,
        parasolid_core::compare_xt_documents(
            &left.document,
            &right.document,
            ComparisonOptions {
                absolute_tolerance,
                relative_tolerance,
                max_differences,
            },
        ),
    )
}

#[pyfunction(name = "_compare_xb_xt")]
#[allow(clippy::needless_pass_by_value)]
fn compare_xb_xt<'py>(
    py: Python<'py>,
    left: PyRef<'_, NativeXbDocument>,
    right: PyRef<'_, NativeXtDocument>,
    absolute_tolerance: f64,
    relative_tolerance: f64,
    max_differences: usize,
) -> PyResult<Bound<'py, PyDict>> {
    comparison_response(
        py,
        parasolid_core::compare_xb_xt_documents(
            &left.document,
            &right.document,
            ComparisonOptions {
                absolute_tolerance,
                relative_tolerance,
                max_differences,
            },
        ),
    )
}

#[pyfunction(name = "_compare_xt_xb")]
#[allow(clippy::needless_pass_by_value)]
fn compare_xt_xb<'py>(
    py: Python<'py>,
    left: PyRef<'_, NativeXtDocument>,
    right: PyRef<'_, NativeXbDocument>,
    absolute_tolerance: f64,
    relative_tolerance: f64,
    max_differences: usize,
) -> PyResult<Bound<'py, PyDict>> {
    comparison_response(
        py,
        parasolid_core::compare_xt_xb_documents(
            &left.document,
            &right.document,
            ComparisonOptions {
                absolute_tolerance,
                relative_tolerance,
                max_differences,
            },
        ),
    )
}

fn comparison_response(
    py: Python<'_>,
    result: Result<DocumentComparison, ParseError>,
) -> PyResult<Bound<'_, PyDict>> {
    let response = PyDict::new(py);
    match result {
        Ok(comparison) => {
            response.set_item("ok", true)?;
            response.set_item("value", comparison_to_python(py, &comparison)?)?;
        }
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("error", error_to_python(py, &error)?)?;
        }
    }
    Ok(response)
}

fn comparison_to_python<'py>(
    py: Python<'py>,
    comparison: &DocumentComparison,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("equivalent", comparison.equivalent)?;
    value.set_item("schema_key_equal", comparison.schema_key_equal)?;
    value.set_item("schema_coverage_equal", comparison.schema_coverage_equal)?;
    value.set_item("node_type_counts_equal", comparison.node_type_counts_equal)?;
    value.set_item(
        "node_index_layout_equal",
        comparison.node_index_layout_equal,
    )?;
    value.set_item("topology_equal", comparison.topology_equal)?;
    value.set_item("field_values_equal", comparison.field_values_equal)?;
    value.set_item("left_node_count", comparison.left_node_count)?;
    value.set_item("right_node_count", comparison.right_node_count)?;
    value.set_item("compared_node_count", comparison.compared_node_count)?;
    value.set_item("difference_count", comparison.difference_count)?;
    value.set_item("differences_truncated", comparison.differences_truncated)?;
    let differences = PyList::empty(py);
    for difference in &comparison.differences {
        differences.append(comparison_difference_to_python(py, difference)?)?;
    }
    value.set_item("differences", differences)?;
    Ok(value)
}

fn comparison_difference_to_python<'py>(
    py: Python<'py>,
    difference: &ComparisonDifference,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("code", difference.code)?;
    value.set_item("category", difference.category)?;
    value.set_item("message", &difference.message)?;
    value.set_item("node_type", difference.node_type)?;
    value.set_item("left_node_index", difference.left_node_index)?;
    value.set_item("right_node_index", difference.right_node_index)?;
    value.set_item("field_name", &difference.field_name)?;
    value.set_item("value_index", difference.value_index)?;
    value.set_item("left_value", &difference.left_value)?;
    value.set_item("right_value", &difference.right_value)?;
    Ok(value)
}

fn header_to_python<'py>(py: Python<'py>, header: &XbHeader) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("signature", PyBytes::new(py, &header.signature))?;
    value.set_item("binary_format", header.binary_format.as_str())?;
    value.set_item("modeller_version", &header.modeller_version)?;
    value.set_item("schema_key", &header.schema_key)?;
    value.set_item("user_field_size", header.user_field_size)?;
    value.set_item("schema_max_type", header.schema_max_type)?;
    value.set_item("file_size", header.file_size)?;
    value.set_item(
        "text_header_range",
        header
            .text_header_range
            .as_ref()
            .map(|range| (range.start, range.end)),
    )?;
    value.set_item(
        "binary_header_range",
        (
            header.binary_header_range.start,
            header.binary_header_range.end,
        ),
    )?;
    value.set_item(
        "header_range",
        (header.header_range.start, header.header_range.end),
    )?;
    Ok(value)
}

fn xt_header_to_python<'py>(py: Python<'py>, header: &XtHeader) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("flag", char::from(header.flag).to_string())?;
    value.set_item("modeller_version", &header.modeller_version)?;
    value.set_item("schema_key", &header.schema_key)?;
    value.set_item("user_field_size", header.user_field_size)?;
    value.set_item("schema_max_type", header.schema_max_type)?;
    value.set_item("file_size", header.file_size)?;
    value.set_item(
        "common_header_range",
        header
            .common_header_range
            .as_ref()
            .map(|range| (range.start, range.end)),
    )?;
    value.set_item(
        "text_stream_header_range",
        (
            header.text_stream_header_range.start,
            header.text_stream_header_range.end,
        ),
    )?;
    value.set_item(
        "header_range",
        (header.header_range.start, header.header_range.end),
    )?;
    Ok(value)
}

#[pyfunction(name = "_resolve_schema_blob")]
#[allow(clippy::too_many_arguments)]
fn resolve_schema_blob<'py>(
    py: Python<'py>,
    data: &[u8],
    node_type: u16,
    base_name: Option<String>,
    base_description: Option<String>,
    base_fields: Option<Vec<BaseField>>,
    max_fields_per_type: usize,
    max_string_bytes: usize,
    max_schema_types: usize,
) -> PyResult<Bound<'py, PyDict>> {
    let base = base_definition_from_python(node_type, base_name, base_description, base_fields)?;
    let response = PyDict::new(py);
    match decode_embedded_schema(
        data,
        0,
        node_type,
        base.as_ref(),
        SchemaLimits {
            max_fields_per_type,
            max_string_bytes,
            max_schema_types,
        },
    ) {
        Ok(resolution) => {
            response.set_item("ok", true)?;
            response.set_item("value", schema_resolution_to_python(py, &resolution)?)?;
        }
        Err(error) => {
            response.set_item("ok", false)?;
            response.set_item("error", error_to_python(py, &error)?)?;
        }
    }
    Ok(response)
}

#[pyfunction(name = "_schema_coverage")]
fn schema_coverage(
    py: Python<'_>,
    entries: Vec<(u16, String, usize)>,
) -> PyResult<Bound<'_, PyDict>> {
    let parsed = entries
        .into_iter()
        .map(|(node_type, source, field_count)| {
            SchemaSource::from_name(&source)
                .map(|parsed_source| (node_type, parsed_source, field_count))
                .ok_or_else(|| PyValueError::new_err(format!("invalid schema source {source:?}")))
        })
        .collect::<PyResult<Vec<_>>>()?;
    let report = SchemaCoverageReport::from_entries(&parsed);
    let value = PyDict::new(py);
    value.set_item("node_types", report.node_types)?;
    value.set_item("field_count", report.field_count)?;
    value.set_item("base_count", report.base_count)?;
    value.set_item("unchanged_count", report.unchanged_count)?;
    value.set_item("delta_count", report.delta_count)?;
    value.set_item("full_count", report.full_count)?;
    Ok(value)
}

fn base_definition_from_python(
    node_type: u16,
    name: Option<String>,
    description: Option<String>,
    fields: Option<Vec<BaseField>>,
) -> PyResult<Option<TypeDefinition>> {
    let (name, description, fields) = match (name, description, fields) {
        (None, None, None) => return Ok(None),
        (Some(name), Some(description), Some(fields)) => (name, description, fields),
        _ => {
            return Err(PyValueError::new_err(
                "base name, description, and fields must be supplied together",
            ));
        }
    };
    Ok(Some(type_definition_from_python_fields(
        node_type,
        name,
        description,
        fields,
    )?))
}

fn provider_from_python(
    schema_id: Option<String>,
    definitions: Option<Vec<CatalogType>>,
) -> PyResult<InMemorySchemaProvider> {
    let mut provider = InMemorySchemaProvider::new();
    let (schema_id, definitions) = match (schema_id, definitions) {
        (None, None) => return Ok(provider),
        (Some(schema_id), Some(definitions)) => (schema_id, definitions),
        _ => {
            return Err(PyValueError::new_err(
                "schema id and catalog definitions must be supplied together",
            ));
        }
    };
    provider.add_schema(schema_id.clone());
    for (node_type, name, description, fields) in definitions {
        let definition = type_definition_from_python_fields(node_type, name, description, fields)?;
        if provider.insert(schema_id.clone(), definition).is_some() {
            return Err(PyValueError::new_err(format!(
                "schema catalog contains duplicate node type {node_type}"
            )));
        }
    }
    Ok(provider)
}

fn type_definition_from_python_fields(
    node_type: u16,
    name: String,
    description: String,
    fields: Vec<BaseField>,
) -> PyResult<TypeDefinition> {
    let fields = fields
        .into_iter()
        .map(
            |(field_name, type_code, pointer_class, element_count, transmitted)| {
                let field_type = FieldType::from_code(&type_code).ok_or_else(|| {
                    PyValueError::new_err(format!("unsupported base field type {type_code:?}"))
                })?;
                if pointer_class != 0 && field_type != FieldType::PointerIndex {
                    return Err(PyValueError::new_err(
                        "non-zero pointer class requires pointer field type",
                    ));
                }
                Ok(FieldDefinition {
                    name: field_name,
                    field_type,
                    pointer_class,
                    element_count,
                    transmitted,
                })
            },
        )
        .collect::<PyResult<Vec<_>>>()?;
    Ok(TypeDefinition::from_fields(
        node_type,
        name,
        description,
        fields,
        SchemaSource::Base,
    ))
}

fn document_to_python<'py>(py: Python<'py>, document: &XbDocument) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("format", "binary")?;
    value.set_item("header", header_to_python(py, &document.header)?)?;
    value.set_item(
        "schema_key",
        schema_key_to_python(py, &document.schema_key)?,
    )?;

    let schemas = PyList::empty(py);
    for resolution in &document.schemas {
        schemas.append(schema_resolution_to_python(py, resolution)?)?;
    }
    value.set_item("schemas", schemas)?;

    let nodes = PyList::empty(py);
    for node in &document.nodes {
        nodes.append(raw_node_to_python(py, node)?)?;
    }
    value.set_item("nodes", nodes)?;
    value.set_item(
        "terminator",
        termination_to_python(py, &document.terminator)?,
    )?;
    value.set_item(
        "schema_coverage",
        schema_coverage_to_python(py, &document.schema_coverage)?,
    )?;
    value.set_item("raw_bytes", PyBytes::new(py, document.raw_bytes()))?;
    Ok(value)
}

fn xt_document_to_python<'py>(
    py: Python<'py>,
    document: &XtDocument,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("format", "text")?;
    value.set_item("header", xt_header_to_python(py, &document.header)?)?;
    value.set_item(
        "schema_key",
        schema_key_to_python(py, &document.schema_key)?,
    )?;

    let schemas = PyList::empty(py);
    for resolution in &document.schemas {
        schemas.append(schema_resolution_to_python(py, resolution)?)?;
    }
    value.set_item("schemas", schemas)?;

    let nodes = PyList::empty(py);
    for node in &document.nodes {
        nodes.append(raw_node_to_python(py, node)?)?;
    }
    value.set_item("nodes", nodes)?;
    value.set_item(
        "terminator",
        xt_termination_to_python(py, &document.terminator)?,
    )?;
    value.set_item(
        "schema_coverage",
        schema_coverage_to_python(py, &document.schema_coverage)?,
    )?;
    value.set_item("raw_bytes", PyBytes::new(py, document.raw_bytes()))?;
    Ok(value)
}

fn schema_key_to_python<'py>(
    py: Python<'py>,
    schema_key: &SchemaKey,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("raw", schema_key.raw())?;
    value.set_item("modeller", schema_key.modeller())?;
    value.set_item("effective", schema_key.effective())?;
    value.set_item("base", schema_key.base())?;
    value.set_item("provider_schema", schema_key.provider_schema())?;
    Ok(value)
}

fn raw_node_to_python<'py>(py: Python<'py>, node: &RawNode) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("node_type", node.node_type)?;
    value.set_item("index", node.index)?;
    value.set_item("variable_length", node.variable_length)?;
    value.set_item(
        "definition",
        type_definition_to_python(py, &node.definition)?,
    )?;
    value.set_item(
        "first_schema",
        node.first_schema
            .as_ref()
            .map(|schema| schema_resolution_to_python(py, schema))
            .transpose()?,
    )?;
    let fields = PyList::empty(py);
    for field in &node.fields {
        fields.append(raw_field_to_python(py, field)?)?;
    }
    value.set_item("fields", fields)?;
    value.set_item("byte_range", (node.byte_range.start, node.byte_range.end))?;
    Ok(value)
}

fn raw_field_to_python<'py>(py: Python<'py>, field: &RawField) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item(
        "definition",
        field_definition_to_python(py, &field.definition)?,
    )?;
    let values = PyList::empty(py);
    for item in &field.values {
        values.append(field_value_to_python(py, item)?)?;
    }
    value.set_item("values", values)?;
    value.set_item("byte_range", (field.byte_range.start, field.byte_range.end))?;
    Ok(value)
}

fn field_value_to_python<'py>(
    py: Python<'py>,
    field_value: &FieldValue,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("field_type", field_value.field_type().code())?;
    match field_value {
        FieldValue::UnsignedByte(item) | FieldValue::Character(item) => {
            value.set_item("value", item)?;
        }
        FieldValue::Logical(item) => value.set_item("value", item)?,
        FieldValue::ShortInteger(item) => value.set_item("value", item)?,
        FieldValue::UnicodeCharacter(item) => value.set_item("value", item)?,
        FieldValue::Integer(item) | FieldValue::Tag(item) => value.set_item("value", item)?,
        FieldValue::PointerIndex(item) => value.set_item("value", item)?,
        FieldValue::Double(item) => value.set_item("value", item)?,
        FieldValue::Interval(items) => {
            value.set_item("value", nullable_doubles_to_python(py, items)?)?;
        }
        FieldValue::Vector(items) | FieldValue::IntersectionPoint(items) => {
            value.set_item("value", nullable_doubles_to_python(py, items)?)?;
        }
        FieldValue::Box3(items) => {
            value.set_item("value", nullable_doubles_to_python(py, items)?)?;
        }
    }
    Ok(value)
}

fn nullable_doubles_to_python<'py, const COUNT: usize>(
    py: Python<'py>,
    values: &[Option<f64>; COUNT],
) -> PyResult<Bound<'py, PyList>> {
    let output = PyList::empty(py);
    for value in values {
        output.append(value)?;
    }
    Ok(output)
}

fn termination_to_python<'py>(
    py: Python<'py>,
    termination: &XbTermination,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("index", termination.index)?;
    value.set_item(
        "byte_range",
        (termination.byte_range.start, termination.byte_range.end),
    )?;
    Ok(value)
}

fn xt_termination_to_python<'py>(
    py: Python<'py>,
    termination: &XtTermination,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("index", termination.index)?;
    value.set_item(
        "byte_range",
        (termination.byte_range.start, termination.byte_range.end),
    )?;
    Ok(value)
}

fn schema_coverage_to_python<'py>(
    py: Python<'py>,
    report: &SchemaCoverageReport,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("node_types", &report.node_types)?;
    value.set_item("field_count", report.field_count)?;
    value.set_item("base_count", report.base_count)?;
    value.set_item("unchanged_count", report.unchanged_count)?;
    value.set_item("delta_count", report.delta_count)?;
    value.set_item("full_count", report.full_count)?;
    Ok(value)
}

fn schema_resolution_to_python<'py>(
    py: Python<'py>,
    resolution: &SchemaResolution,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item(
        "definition",
        type_definition_to_python(py, &resolution.definition)?,
    )?;
    value.set_item("raw_schema", PyBytes::new(py, &resolution.raw_schema))?;
    value.set_item(
        "byte_range",
        (resolution.byte_range.start, resolution.byte_range.end),
    )?;
    let edits = PyList::empty(py);
    for edit in &resolution.edits {
        edits.append(schema_edit_to_python(py, edit)?)?;
    }
    value.set_item("edits", edits)?;
    Ok(value)
}

fn type_definition_to_python<'py>(
    py: Python<'py>,
    definition: &TypeDefinition,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("node_type", definition.node_type)?;
    value.set_item("name", &definition.name)?;
    value.set_item("description", &definition.description)?;
    value.set_item("variable", definition.variable)?;
    value.set_item("source", definition.source.as_str())?;
    let fields = PyList::empty(py);
    for field in &definition.fields {
        fields.append(field_definition_to_python(py, field)?)?;
    }
    value.set_item("fields", fields)?;
    Ok(value)
}

fn catalog_to_python<'py>(
    py: Python<'py>,
    catalog: &ParsedSchemaCatalog,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("schema_id", &catalog.schema_id)?;
    value.set_item("modeller_version", &catalog.modeller_version)?;
    value.set_item("declared_max_node_type", catalog.declared_max_node_type)?;
    value.set_item("declared_node_count", catalog.declared_node_count)?;
    value.set_item("declared_field_count", catalog.declared_field_count)?;
    value.set_item("declared_auxiliary_count", catalog.declared_auxiliary_count)?;
    let definitions = PyList::empty(py);
    for definition in &catalog.definitions {
        definitions.append(type_definition_to_python(py, definition)?)?;
    }
    value.set_item("definitions", definitions)?;
    Ok(value)
}

fn field_definition_to_python<'py>(
    py: Python<'py>,
    field: &FieldDefinition,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("name", &field.name)?;
    value.set_item("field_type", field.field_type.code())?;
    value.set_item("pointer_class", field.pointer_class)?;
    value.set_item("element_count", field.element_count)?;
    value.set_item("transmitted", field.transmitted)?;
    Ok(value)
}

fn schema_edit_to_python<'py>(py: Python<'py>, edit: &SchemaEdit) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("opcode", char::from(edit.opcode()).to_string())?;
    value.set_item("offset", edit.offset())?;
    if let Some(field) = edit.field() {
        value.set_item("field", field_definition_to_python(py, field)?)?;
    }
    Ok(value)
}

fn error_to_python<'py>(py: Python<'py>, error: &ParseError) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("code", error.kind().code())?;
    value.set_item("message", error.message())?;
    value.set_item("offset", error.offset())?;
    value.set_item("details", error_details_to_python(py, error.details())?)?;
    Ok(value)
}

fn error_details_to_python<'py>(
    py: Python<'py>,
    details: &ErrorDetails,
) -> PyResult<Bound<'py, PyDict>> {
    let value = PyDict::new(py);
    match details {
        ErrorDetails::None => {}
        ErrorDetails::UnexpectedEof { needed, remaining } => {
            value.set_item("needed", needed)?;
            value.set_item("remaining", remaining)?;
        }
        ErrorDetails::LimitExceeded {
            resource,
            actual,
            limit,
        } => {
            value.set_item("resource", resource)?;
            value.set_item("actual", actual)?;
            value.set_item("limit", limit)?;
        }
        ErrorDetails::InvalidLength { field, value: raw } => {
            value.set_item("field", field)?;
            value.set_item("value", raw)?;
        }
        ErrorDetails::InvalidByte { field, value: raw } => {
            value.set_item("field", field)?;
            value.set_item("value", raw)?;
        }
        ErrorDetails::InvalidText { field, value: raw } => {
            value.set_item("field", field)?;
            value.set_item("value", raw)?;
        }
        ErrorDetails::CountMismatch {
            field,
            expected,
            actual,
        } => {
            value.set_item("field", field)?;
            value.set_item("expected", expected)?;
            value.set_item("actual", actual)?;
        }
        ErrorDetails::SchemaLookup { schema, node_type } => {
            value.set_item("schema", schema)?;
            value.set_item("node_type", node_type)?;
        }
        ErrorDetails::NodeType { node_type } => {
            value.set_item("node_type", node_type)?;
        }
        ErrorDetails::NodeIndex { node_index } => {
            value.set_item("node_index", node_index)?;
        }
        ErrorDetails::BrepField { node_index, field } => {
            value.set_item("node_index", node_index)?;
            value.set_item("field", field)?;
        }
        ErrorDetails::BrepReference {
            node_index,
            field,
            target_index,
            expected_type,
        } => {
            value.set_item("node_index", node_index)?;
            value.set_item("field", field)?;
            value.set_item("target_index", target_index)?;
            value.set_item("expected_type", expected_type)?;
        }
        ErrorDetails::BrepInvariant {
            node_index,
            relationship,
        } => {
            value.set_item("node_index", node_index)?;
            value.set_item("relationship", relationship)?;
        }
    }
    Ok(value)
}

#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeXbDocument>()?;
    module.add_class::<NativeXtDocument>()?;
    module.add_function(wrap_pyfunction!(inspect_xb, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_xt, module)?)?;
    module.add_function(wrap_pyfunction!(parse_schema_catalog_native, module)?)?;
    module.add_function(wrap_pyfunction!(parse_xb, module)?)?;
    module.add_function(wrap_pyfunction!(parse_xt, module)?)?;
    module.add_function(wrap_pyfunction!(write_xb, module)?)?;
    module.add_function(wrap_pyfunction!(compare_xb_xb, module)?)?;
    module.add_function(wrap_pyfunction!(compare_xt_xt, module)?)?;
    module.add_function(wrap_pyfunction!(compare_xb_xt, module)?)?;
    module.add_function(wrap_pyfunction!(compare_xt_xb, module)?)?;
    module.add_function(wrap_pyfunction!(brep::map_xb_brep_native, module)?)?;
    module.add_function(wrap_pyfunction!(brep::map_xt_brep_native, module)?)?;
    module.add_function(wrap_pyfunction!(resolve_schema_blob, module)?)?;
    module.add_function(wrap_pyfunction!(schema_coverage, module)?)?;
    module.add("CORE_VERSION", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
