//! Opt-in decoder for named-schema XT/XB polylines.

use crate::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, ErrorDetails, ErrorKind, FieldType, FieldValue,
    ParseError, RawField, RawNode, SchemaKey, SchemaProvider,
};
use std::collections::BTreeMap;

use super::model::{Sense, SourceNodeRef, Vector3};

/// Lossless geometric data directly stored by one named-schema `POLYLINE`.
#[derive(Debug, Clone, PartialEq)]
pub struct PolylineData {
    /// Ordered source point vectors. No parameter spacing is inferred.
    pub points: Vec<Vector3>,
    /// Whether the connected segment chain closes.
    pub closed: bool,
    /// Orientation stored on the common `POLYLINE` curve fields.
    pub sense: Sense,
    /// Parameter of the first source point vector.
    pub base_parameter: f64,
    /// Provenance of the `POLYLINE` node.
    pub source: SourceNodeRef,
}

impl PolylineData {
    /// Decode one polyline using an exact compiled `TopSolid` schema profile.
    ///
    /// The supplied key must be one of the exact built-in `TopSolid` v6 keys;
    /// the profile's node definitions must match the parsed nodes. No node
    /// numbers are assigned semantic roles by this decoder.
    ///
    /// # Errors
    /// Returns an error for an unsupported key, an uncovered profile type, a
    /// schema-layout mismatch, or malformed polyline data.
    pub fn decode_builtin_topsolid(
        nodes: &[RawNode],
        schema_key: &str,
        polyline_index: u32,
    ) -> Result<Self, ParseError> {
        decode_builtin_topsolid_polyline(nodes, schema_key, polyline_index)
    }
}

/// Decode one named-schema `POLYLINE` and its linked data and point values.
///
/// This is opt-in because built-in numeric profiles do not currently assign
/// reviewed B-Rep roles to polyline node numbers.
///
/// # Errors
/// Returns an error for a non-`POLYLINE` source, missing or malformed fields,
/// unresolved or wrongly named pointer targets, invalid point vectors, or a
/// mismatch between `n_pvecs` and the target value count.
pub fn decode_named_polyline(
    nodes: &[RawNode],
    polyline_index: u32,
) -> Result<PolylineData, ParseError> {
    decode_named_polyline_with_lookup(polyline_index, |index| node_by_index(nodes, index))
}

/// Indexed mapper path. `duplicates` stores the second occurrence of every
/// duplicated index so errors retain the slice decoder's byte offset.
pub(super) fn decode_named_polyline_indexed<'a>(
    polyline_index: u32,
    nodes: &BTreeMap<u32, &'a RawNode>,
    duplicates: &BTreeMap<u32, &'a RawNode>,
) -> Result<PolylineData, ParseError> {
    decode_named_polyline_with_lookup(polyline_index, |index| {
        indexed_node(nodes, duplicates, index)
    })
}

fn decode_named_polyline_with_lookup<'a>(
    polyline_index: u32,
    mut lookup: impl FnMut(u32) -> Result<Option<&'a RawNode>, ParseError>,
) -> Result<PolylineData, ParseError> {
    let polyline = lookup(polyline_index)?.ok_or_else(|| {
        ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            format!("polyline node {polyline_index} does not exist"),
            ErrorDetails::NodeIndex {
                node_index: polyline_index,
            },
        )
    })?;
    if polyline.definition.name != "POLYLINE" {
        return Err(invalid_field(
            polyline,
            "node_type",
            "node is not named POLYLINE",
        ));
    }

    let data_index = pointer(polyline, "data")?;
    let data = target_with_lookup(&mut lookup, polyline, "data", data_index, "POLYLINE_DATA")?;
    decode_named_polyline_fields(polyline, data, None, &mut lookup)
}

fn decode_named_polyline_fields<'a>(
    polyline: &'a RawNode,
    data: &'a RawNode,
    prevalidated_point_values: Option<&'a RawNode>,
    lookup: &mut impl FnMut(u32) -> Result<Option<&'a RawNode>, ParseError>,
) -> Result<PolylineData, ParseError> {
    let sense = match value(polyline, "sense")? {
        FieldValue::Character(b'+') => Sense::Positive,
        FieldValue::Character(b'-') => Sense::Negative,
        FieldValue::Character(b'?') => Sense::Unknown,
        FieldValue::Character(_) => {
            return Err(invalid_field(
                polyline,
                "sense",
                "sense must be '+', '-', or '?'",
            ));
        }
        _ => return Err(invalid_field(polyline, "sense", "sense is not a character")),
    };
    let count = match value(data, "n_pvecs")? {
        FieldValue::Integer(Some(count)) if *count >= 0 => usize::try_from(*count)
            .map_err(|_| invalid_field(data, "n_pvecs", "point count exceeds addressable size"))?,
        FieldValue::Integer(_) => {
            return Err(invalid_field(
                data,
                "n_pvecs",
                "point count must be non-negative",
            ));
        }
        _ => {
            return Err(invalid_field(
                data,
                "n_pvecs",
                "point count is not an integer",
            ));
        }
    };
    let closed = match value(data, "closed")? {
        FieldValue::Logical(closed) => *closed,
        _ => return Err(invalid_field(data, "closed", "closed flag is not logical")),
    };
    let base_parameter = match value(data, "base_parm")? {
        FieldValue::Double(Some(parameter)) if parameter.is_finite() => *parameter,
        _ => {
            return Err(invalid_field(
                data,
                "base_parm",
                "base parameter is missing or non-finite",
            ));
        }
    };
    let point_values = if let Some(point_values) = prevalidated_point_values {
        point_values
    } else {
        let points_index = pointer(data, "pvec")?;
        target_with_lookup(lookup, data, "pvec", points_index, "POINT_VALUES")?
    };
    let field = required_field(point_values, "values")?;
    if field.definition.field_type != FieldType::Vector {
        return Err(invalid_field(
            point_values,
            "values",
            "point values are not declared as vectors",
        ));
    }
    if field.values.len() != count {
        return Err(ParseError::new(
            ErrorKind::InvalidBrepField,
            data.byte_range.start,
            format!(
                "n_pvecs is {count}, but POINT_VALUES contains {} vectors",
                field.values.len()
            ),
            ErrorDetails::CountMismatch {
                field: "n_pvecs",
                expected: count,
                actual: field.values.len(),
            },
        ));
    }
    let points = field
        .values
        .iter()
        .map(|value| match value {
            FieldValue::Vector([Some(x), Some(y), Some(z)])
                if x.is_finite() && y.is_finite() && z.is_finite() =>
            {
                Ok(Vector3::from_array([*x, *y, *z]))
            }
            _ => Err(invalid_field(
                point_values,
                "values",
                "point vector is incomplete or non-finite",
            )),
        })
        .collect::<Result<_, _>>()?;

    Ok(PolylineData {
        points,
        closed,
        sense,
        base_parameter,
        source: source(polyline),
    })
}

fn decode_builtin_topsolid_polyline(
    nodes: &[RawNode],
    schema_key: &str,
    polyline_index: u32,
) -> Result<PolylineData, ParseError> {
    decode_builtin_topsolid_polyline_with_lookup(schema_key, polyline_index, |index| {
        node_by_index(nodes, index)
    })
}

pub(super) fn decode_builtin_topsolid_polyline_indexed<'a>(
    schema_key: &str,
    polyline_index: u32,
    nodes: &BTreeMap<u32, &'a RawNode>,
    duplicates: &BTreeMap<u32, &'a RawNode>,
) -> Result<PolylineData, ParseError> {
    decode_builtin_topsolid_polyline_with_lookup(schema_key, polyline_index, |index| {
        indexed_node(nodes, duplicates, index)
    })
}

fn decode_builtin_topsolid_polyline_with_lookup<'a>(
    schema_key: &str,
    polyline_index: u32,
    mut lookup: impl FnMut(u32) -> Result<Option<&'a RawNode>, ParseError>,
) -> Result<PolylineData, ParseError> {
    let key = SchemaKey::parse(schema_key)?;
    if !crate::schema::profiles::topsolid::SUPPORTED_SCHEMA_KEYS.contains(&schema_key) {
        return Err(unsupported_builtin_key(schema_key));
    }
    let registry = BuiltinProfileRegistry::compiled()?;
    let provider = registry
        .provider_for_key(&key)
        .ok_or_else(|| unsupported_builtin_key(schema_key))?;
    let profile = provider.profile();
    if profile.metadata().coverage != BuiltinProfileCoverage::VerifiedSubset
        || profile.metadata().producer_scope != "TopSolid v6 observed X_T streams"
        || profile.metadata().provider_schema != key.provider_schema()
        || !profile
            .accepted_schema_keys()
            .any(|accepted| accepted.raw() == schema_key)
    {
        return Err(unsupported_builtin_key(schema_key));
    }

    let polyline = lookup(polyline_index)?.ok_or_else(|| {
        ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            format!("polyline node {polyline_index} does not exist"),
            ErrorDetails::NodeIndex {
                node_index: polyline_index,
            },
        )
    })?;
    validate_builtin_node(&provider, &key, polyline, "POLYLINE")?;
    let data_index = pointer(polyline, "data")?;
    let data = target_with_lookup(&mut lookup, polyline, "data", data_index, "POLYLINE_DATA")?;
    validate_builtin_node(&provider, &key, data, "POLYLINE_DATA")?;
    let points_index = pointer(data, "pvec")?;
    let point_values = target_with_lookup(&mut lookup, data, "pvec", points_index, "POINT_VALUES")?;
    validate_builtin_node(&provider, &key, point_values, "POINT_VALUES")?;

    decode_named_polyline_fields(polyline, data, Some(point_values), &mut lookup)
}

fn validate_builtin_node(
    provider: &impl SchemaProvider,
    key: &SchemaKey,
    node: &RawNode,
    expected_name: &'static str,
) -> Result<(), ParseError> {
    let schema = key.effective();
    let expected = provider
        .type_definition(schema, node.node_type)
        .ok_or_else(|| {
            ParseError::new(
                ErrorKind::BuiltinProfileUncoveredType,
                node.byte_range.start,
                format!("built-in TopSolid profile does not define {expected_name}"),
                ErrorDetails::SchemaLookup {
                    schema: schema.to_owned(),
                    node_type: node.node_type,
                },
            )
        })?;
    if expected.name != expected_name
        || node.definition.node_type != expected.node_type
        || node.definition.name != expected.name
        || node.definition.variable != expected.variable
        || node.definition.fields != expected.fields
        || node.fields.len() != expected.fields.len()
        || node
            .fields
            .iter()
            .zip(&expected.fields)
            .any(|(actual, expected)| actual.definition.as_ref() != expected)
    {
        return Err(invalid_field(
            node,
            "schema_layout",
            format!("node does not match the reviewed {expected_name} layout"),
        ));
    }
    Ok(())
}

fn unsupported_builtin_key(schema_key: &str) -> ParseError {
    ParseError::new(
        ErrorKind::UnsupportedBuiltinSchemaKey,
        0,
        "polyline decoding requires an exact compiled TopSolid v6 schema key",
        ErrorDetails::InvalidText {
            field: "schema_key",
            value: schema_key.to_owned(),
        },
    )
}

fn node_by_index(nodes: &[RawNode], index: u32) -> Result<Option<&RawNode>, ParseError> {
    let mut matching = nodes.iter().filter(|node| node.index == index);
    let Some(node) = matching.next() else {
        return Ok(None);
    };
    if let Some(duplicate) = matching.next() {
        return Err(duplicate_node_index(index, duplicate));
    }
    Ok(Some(node))
}

fn indexed_node<'a>(
    nodes: &BTreeMap<u32, &'a RawNode>,
    duplicates: &BTreeMap<u32, &'a RawNode>,
    index: u32,
) -> Result<Option<&'a RawNode>, ParseError> {
    if let Some(duplicate) = duplicates.get(&index) {
        return Err(duplicate_node_index(index, duplicate));
    }
    Ok(nodes.get(&index).copied())
}

fn duplicate_node_index(index: u32, duplicate: &RawNode) -> ParseError {
    ParseError::new(
        ErrorKind::DuplicateNodeIndex,
        duplicate.byte_range.start,
        format!("node index {index} is ambiguous"),
        ErrorDetails::NodeIndex { node_index: index },
    )
}

fn required_field<'a>(node: &'a RawNode, name: &'static str) -> Result<&'a RawField, ParseError> {
    let mut matching = node
        .fields
        .iter()
        .filter(|field| field.definition.name == name);
    let field = matching
        .next()
        .ok_or_else(|| invalid_field(node, name, "required field is missing"))?;
    if matching.next().is_some() {
        return Err(invalid_field(node, name, "field name is ambiguous"));
    }
    Ok(field)
}

fn value<'a>(node: &'a RawNode, name: &'static str) -> Result<&'a FieldValue, ParseError> {
    let field = required_field(node, name)?;
    if field.values.len() != 1 {
        return Err(invalid_field(
            node,
            name,
            "field must contain exactly one value",
        ));
    }
    Ok(&field.values[0])
}

fn pointer(node: &RawNode, name: &'static str) -> Result<u32, ParseError> {
    match value(node, name)? {
        FieldValue::PointerIndex(index) if *index != 0 => Ok(*index),
        FieldValue::PointerIndex(_) => Err(invalid_reference(
            node,
            name,
            0,
            "linked node",
            "pointer is null",
        )),
        _ => Err(invalid_field(node, name, "field is not a pointer")),
    }
}

fn target_with_lookup<'a>(
    lookup: &mut impl FnMut(u32) -> Result<Option<&'a RawNode>, ParseError>,
    source: &RawNode,
    field: &'static str,
    index: u32,
    expected: &'static str,
) -> Result<&'a RawNode, ParseError> {
    let node = lookup(index)?.ok_or_else(|| {
        invalid_reference(
            source,
            field,
            index,
            expected,
            "pointer target is unresolved",
        )
    })?;
    if node.definition.name != expected {
        return Err(invalid_reference(
            source,
            field,
            index,
            expected,
            format!(
                "pointer targets {} rather than {expected}",
                node.definition.name
            ),
        ));
    }
    Ok(node)
}

fn source(node: &RawNode) -> SourceNodeRef {
    let node_id = node
        .fields
        .iter()
        .find(|field| field.definition.name == "node_id")
        .and_then(|field| field.values.first())
        .and_then(|value| match value {
            FieldValue::Integer(value) => *value,
            _ => None,
        });
    SourceNodeRef {
        node_index: node.index,
        node_type: node.node_type,
        type_name: node.definition.name.clone(),
        node_id,
        byte_range: node.byte_range.clone(),
    }
}

fn invalid_field(node: &RawNode, field: &'static str, message: impl Into<String>) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBrepField,
        node.byte_range.start,
        message,
        ErrorDetails::BrepField {
            node_index: node.index,
            field,
        },
    )
}

fn invalid_reference(
    node: &RawNode,
    field: &'static str,
    target_index: u32,
    expected_type: &'static str,
    message: impl Into<String>,
) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBrepReference,
        node.byte_range.start,
        message,
        ErrorDetails::BrepReference {
            node_index: node.index,
            field,
            target_index,
            expected_type,
        },
    )
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::unwrap_used,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::*;
    use crate::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};

    fn node(index: u32, node_type: u16, name: &str, fields: Vec<(&str, FieldValue)>) -> RawNode {
        RawNode {
            node_type,
            index,
            variable_length: None,
            definition: TypeDefinition::from_fields(
                node_type,
                name,
                name,
                fields
                    .iter()
                    .map(|(name, value)| FieldDefinition {
                        name: (*name).to_owned(),
                        field_type: value.field_type(),
                        pointer_class: 0,
                        element_count: 0,
                        transmitted: true,
                    })
                    .collect(),
                SchemaSource::Base,
            )
            .into(),
            first_schema: None,
            fields: fields
                .into_iter()
                .map(|(name, value)| RawField {
                    definition: FieldDefinition {
                        name: name.to_owned(),
                        field_type: value.field_type(),
                        pointer_class: 0,
                        element_count: 0,
                        transmitted: true,
                    }
                    .into(),
                    values: vec![value],
                    byte_range: 0..1,
                })
                .collect(),
            byte_range: std::ops::Range {
                start: index as usize,
                end: index as usize + 1,
            },
            user_fields: Vec::new(),
        }
    }

    fn fixture(point_values: Vec<FieldValue>, count: i32) -> Vec<RawNode> {
        let mut values_node = node(3, 902, "POINT_VALUES", vec![]);
        let values_definition = FieldDefinition {
            name: "values".into(),
            field_type: FieldType::Vector,
            pointer_class: 0,
            element_count: 1,
            transmitted: true,
        };
        std::sync::Arc::make_mut(&mut values_node.definition)
            .fields
            .push(values_definition.clone());
        values_node.fields.push(RawField {
            definition: values_definition.into(),
            values: point_values,
            byte_range: 0..1,
        });
        vec![
            node(
                1,
                900,
                "POLYLINE",
                vec![
                    ("node_id", FieldValue::Integer(Some(77))),
                    ("sense", FieldValue::Character(b'+')),
                    ("data", FieldValue::PointerIndex(2)),
                ],
            ),
            node(
                2,
                901,
                "POLYLINE_DATA",
                vec![
                    ("n_pvecs", FieldValue::Integer(Some(count))),
                    ("closed", FieldValue::Logical(true)),
                    ("base_parm", FieldValue::Double(Some(-2.5))),
                    ("pvec", FieldValue::PointerIndex(3)),
                ],
            ),
            values_node,
        ]
    }

    fn builtin_type(key: &SchemaKey, name: &str) -> Option<TypeDefinition> {
        let provider = BuiltinProfileRegistry::compiled()
            .unwrap()
            .provider_for_key(key)
            .unwrap();
        (0..=u16::MAX).find_map(|node_type| {
            provider
                .type_definition(key.effective(), node_type)
                .filter(|definition| definition.name == name)
                .cloned()
        })
    }

    fn builtin_node(
        definition: TypeDefinition,
        index: u32,
        overrides: &[(&str, Vec<FieldValue>)],
    ) -> RawNode {
        let values = definition
            .fields
            .iter()
            .map(|field| {
                overrides
                    .iter()
                    .find(|(name, _)| *name == field.name)
                    .map_or_else(
                        || vec![default_value(field.field_type)],
                        |(_, values)| values.clone(),
                    )
            })
            .collect::<Vec<_>>();
        let variable_length = definition
            .variable
            .then(|| u32::try_from(values.last().map_or(0, Vec::len)).unwrap());
        RawNode {
            node_type: definition.node_type,
            index,
            variable_length,
            fields: definition
                .fields
                .iter()
                .cloned()
                .zip(values)
                .map(|(definition, values)| RawField {
                    definition: definition.into(),
                    values,
                    byte_range: 0..1,
                })
                .collect(),
            definition: definition.into(),
            first_schema: None,
            byte_range: std::ops::Range {
                start: index as usize,
                end: index as usize + 1,
            },
            user_fields: Vec::new(),
        }
    }

    fn default_value(field_type: FieldType) -> FieldValue {
        match field_type {
            FieldType::UnsignedByte => FieldValue::UnsignedByte(0),
            FieldType::Character => FieldValue::Character(b'+'),
            FieldType::Logical => FieldValue::Logical(false),
            FieldType::ShortInteger => FieldValue::ShortInteger(Some(0)),
            FieldType::UnicodeCharacter => FieldValue::UnicodeCharacter(0),
            FieldType::Integer => FieldValue::Integer(Some(0)),
            FieldType::PointerIndex => FieldValue::PointerIndex(0),
            FieldType::OpaquePointer => unreachable!("opaque pointers are not transmitted"),
            FieldType::Tag => FieldValue::Tag(Some(0)),
            FieldType::Double => FieldValue::Double(Some(0.0)),
            FieldType::Interval => FieldValue::Interval([Some(0.0); 2]),
            FieldType::Vector => FieldValue::Vector([Some(0.0); 3]),
            FieldType::Box3 => FieldValue::Box3([Some(0.0); 6]),
            FieldType::IntersectionPoint => FieldValue::IntersectionPoint([Some(0.0); 3]),
        }
    }

    fn builtin_fixture(key: &SchemaKey) -> Vec<RawNode> {
        vec![
            builtin_node(
                builtin_type(key, "POLYLINE").unwrap(),
                1,
                &[
                    ("node_id", vec![FieldValue::Integer(Some(77))]),
                    ("sense", vec![FieldValue::Character(b'+')]),
                    ("data", vec![FieldValue::PointerIndex(2)]),
                ],
            ),
            builtin_node(
                builtin_type(key, "POLYLINE_DATA").unwrap(),
                2,
                &[
                    ("n_pvecs", vec![FieldValue::Integer(Some(2))]),
                    ("closed", vec![FieldValue::Logical(true)]),
                    ("base_parm", vec![FieldValue::Double(Some(-2.5))]),
                    ("pvec", vec![FieldValue::PointerIndex(3)]),
                ],
            ),
            builtin_node(
                builtin_type(key, "POINT_VALUES").unwrap(),
                3,
                &[(
                    "values",
                    vec![
                        FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)]),
                        FieldValue::Vector([Some(4.0), Some(5.0), Some(6.0)]),
                    ],
                )],
            ),
        ]
    }

    #[test]
    fn decodes_ordered_points_and_preserves_first_parameter_and_source() {
        let nodes = fixture(
            vec![
                FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)]),
                FieldValue::Vector([Some(4.0), Some(5.0), Some(6.0)]),
            ],
            2,
        );
        let polyline = decode_named_polyline(&nodes, 1).unwrap();
        assert_eq!(
            polyline.points,
            [
                Vector3::from_array([1.0, 2.0, 3.0]),
                Vector3::from_array([4.0, 5.0, 6.0])
            ]
        );
        assert!(polyline.closed);
        assert_eq!(polyline.sense, Sense::Positive);
        assert_eq!(polyline.base_parameter, -2.5);
        assert_eq!(polyline.source.node_index, 1);
        assert_eq!(polyline.source.node_type, 900);
        assert_eq!(polyline.source.node_id, Some(77));
        assert_eq!(polyline.source.byte_range, 1..2);
    }

    #[test]
    fn validates_target_name_and_point_count_without_numeric_type_guessing() {
        let mut nodes = fixture(
            vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
            1,
        );
        std::sync::Arc::make_mut(&mut nodes[1].definition).name = "OTHER".into();
        assert_eq!(
            decode_named_polyline(&nodes, 1).unwrap_err().kind(),
            ErrorKind::InvalidBrepReference
        );

        let nodes = fixture(
            vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
            2,
        );
        assert!(matches!(
            decode_named_polyline(&nodes, 1).unwrap_err().details(),
            ErrorDetails::CountMismatch {
                expected: 2,
                actual: 1,
                ..
            }
        ));

        let mut nodes = fixture(vec![], 0);
        std::sync::Arc::make_mut(&mut nodes[0].definition).name = "UNKNOWN".into();
        assert_eq!(
            decode_named_polyline(&nodes, 1).unwrap_err().kind(),
            ErrorKind::InvalidBrepField
        );

        let mut nodes = fixture(
            vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
            1,
        );
        std::sync::Arc::make_mut(&mut nodes[2].fields[0].definition).field_type =
            FieldType::Integer;
        assert_eq!(
            decode_named_polyline(&nodes, 1).unwrap_err().kind(),
            ErrorKind::InvalidBrepField
        );
    }

    #[test]
    fn rejects_ambiguous_source_and_linked_node_indices() {
        let mut duplicate_source = fixture(
            vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
            1,
        );
        duplicate_source.push(duplicate_source[0].clone());
        assert_eq!(
            decode_named_polyline(&duplicate_source, 1)
                .unwrap_err()
                .kind(),
            ErrorKind::DuplicateNodeIndex
        );

        let mut duplicate_data = fixture(
            vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
            1,
        );
        duplicate_data.push(duplicate_data[1].clone());
        assert_eq!(
            decode_named_polyline(&duplicate_data, 1)
                .unwrap_err()
                .kind(),
            ErrorKind::DuplicateNodeIndex
        );

        let mut duplicate_points = fixture(
            vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
            1,
        );
        duplicate_points.push(duplicate_points[2].clone());
        assert_eq!(
            decode_named_polyline(&duplicate_points, 1)
                .unwrap_err()
                .kind(),
            ErrorKind::DuplicateNodeIndex
        );
    }

    #[test]
    fn indexed_decode_matches_slice_duplicate_errors() {
        let nodes = fixture(
            vec![FieldValue::Vector([Some(1.0), Some(2.0), Some(3.0)])],
            1,
        );
        let index = nodes.iter().map(|node| (node.index, node)).collect();
        let duplicates = BTreeMap::new();
        assert!(decode_named_polyline_indexed(1, &index, &duplicates).is_ok());

        let mut duplicate_nodes = nodes.clone();
        duplicate_nodes.push(nodes[1].clone());
        duplicate_nodes.last_mut().unwrap().byte_range = 40..41;
        let index = duplicate_nodes
            .iter()
            .map(|node| (node.index, node))
            .collect();
        let duplicates = BTreeMap::from([(2, &duplicate_nodes[3])]);
        let indexed_error = decode_named_polyline_indexed(1, &index, &duplicates).unwrap_err();
        let slice_error = decode_named_polyline(&duplicate_nodes, 1).unwrap_err();
        assert_eq!(indexed_error.kind(), slice_error.kind());
        assert_eq!(indexed_error.details(), slice_error.details());
        assert_eq!(indexed_error.offset(), slice_error.offset());

        let mut unrelated_duplicate = nodes.clone();
        unrelated_duplicate.push(node(99, 999, "OTHER", vec![]));
        unrelated_duplicate.push(unrelated_duplicate[3].clone());
        let index = unrelated_duplicate
            .iter()
            .map(|node| (node.index, node))
            .collect();
        let duplicates = BTreeMap::from([(99, &unrelated_duplicate[4])]);
        assert!(decode_named_polyline_indexed(1, &index, &duplicates).is_ok());
        assert!(decode_named_polyline(&unrelated_duplicate, 1).is_ok());
    }

    #[test]
    fn exact_topsolid_profiles_accept_only_their_reviewed_layouts() {
        for schema_key in crate::schema::profiles::topsolid::SUPPORTED_SCHEMA_KEYS {
            let key = SchemaKey::parse(schema_key).unwrap();
            if builtin_type(&key, "POLYLINE").is_none() {
                // The 3600188 key exposes only base 13006 definitions.
                let provider = BuiltinProfileRegistry::compiled()
                    .unwrap()
                    .provider_for_key(&key)
                    .unwrap();
                let missing_type = (0..=u16::MAX)
                    .find(|&node_type| {
                        provider
                            .type_definition(key.effective(), node_type)
                            .is_none()
                    })
                    .unwrap();
                let nodes = vec![node(1, missing_type, "POLYLINE", vec![])];
                assert_eq!(
                    PolylineData::decode_builtin_topsolid(&nodes, schema_key, 1)
                        .unwrap_err()
                        .kind(),
                    ErrorKind::BuiltinProfileUncoveredType,
                    "{schema_key}"
                );
                continue;
            }
            let nodes = builtin_fixture(&key);
            let polyline = PolylineData::decode_builtin_topsolid(&nodes, schema_key, 1).unwrap();
            assert_eq!(polyline.points.len(), 2, "{schema_key}");
            assert_eq!(polyline.base_parameter, -2.5, "{schema_key}");
        }

        let key = SchemaKey::parse("SCH_2701192_26105_13006").unwrap();
        let mut nodes = builtin_fixture(&key);
        std::sync::Arc::make_mut(&mut nodes[0].definition).fields[0].name = "unexpected".into();
        assert_eq!(
            PolylineData::decode_builtin_topsolid(&nodes, key.raw(), 1)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidBrepField
        );
        assert_eq!(
            PolylineData::decode_builtin_topsolid(&nodes, "SCH_1_26105_13006", 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedBuiltinSchemaKey
        );
    }
}
