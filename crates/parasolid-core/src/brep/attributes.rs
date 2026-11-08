//! Lossless decoding of native Parasolid attributes for both mapping APIs.

use crate::{
    ErrorDetails, ErrorKind, FieldValue, ParseError, RawField, RawNode, SchemaProviderResolution,
    SchemaResolution, XbDocument, XtDocument,
};

use super::{model::SourceNodeRef, profile_roles::RoleAccess};

/// One value node referenced by an attribute field. Its schema-decoded values
/// retain the source codecs and nullable values without assigning semantics.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeValueNode {
    /// Source node provenance.
    pub source: SourceNodeRef,
    /// Exact schema-framed fields, including their `FieldType` and `FieldValue`s.
    pub fields: Vec<RawField>,
}

/// One attribute field in definition order, including explicit empty values.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeFieldValue {
    /// The raw `ATTRIB_DEF.fields` type code, when transmitted.
    pub declared_kind: Option<u8>,
    /// Null pointers retain an empty typed value; non-null pointers resolve to one typed value.
    pub value: AttributeFieldData,
}

/// Attribute value data decoded from the exact built-in value node type.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeFieldData {
    /// Integer values, preserving unset sentinels.
    Integers(Vec<Option<i32>>),
    /// Real values, preserving unset sentinels.
    Reals(Vec<Option<f64>>),
    /// Transmitted character bytes.
    Text(Vec<u8>),
    /// Point vectors in source order.
    Points(Vec<[Option<f64>; 3]>),
    /// Vector values in source order.
    Vectors(Vec<[Option<f64>; 3]>),
    /// Direction vectors in source order.
    Directions(Vec<[Option<f64>; 3]>),
    /// Axis vectors in source order, retaining each two-vector axis as two entries.
    Axes(Vec<[Option<f64>; 3]>),
    /// Tags, preserving unset sentinels.
    Tags(Vec<Option<i32>>),
    /// UTF-16 code units, preserving their original values.
    Unicode(Vec<u16>),
    /// Value node type is not one of the currently decoded attribute codecs.
    /// The corresponding raw fields remain available in `value_nodes`.
    Unsupported { node_type: u16 },
    /// Null pointer with its declared attribute kind.
    Empty,
}

/// One `ATTRIBUTE` record with its raw owner, definition, and values.
#[derive(Debug, Clone, PartialEq)]
pub struct BrepAttribute {
    /// Attribute node provenance.
    pub source: SourceNodeRef,
    /// Exact raw `ATTRIBUTE.node_id` value.
    pub node_id: FieldValue,
    /// Exact raw `ATTRIB_DEF.type_id` value.
    pub kind_id: FieldValue,
    /// Identifier bytes from the linked `ATT_DEF_ID`; bytes are not normalized.
    pub identifier: Vec<u8>,
    /// Raw owner node provenance, even when the owner is outside mapped B-Rep entities.
    pub owner: SourceNodeRef,
    /// Definition node provenance.
    pub definition: SourceNodeRef,
    /// Attribute values in the order transmitted by `ATTRIBUTE.fields`.
    pub fields: Vec<AttributeFieldValue>,
    /// Raw value nodes, retained alongside typed fields for lossless inspection.
    pub value_nodes: Vec<Option<AttributeValueNode>>,
}

/// Standalone result containing all decoded attribute records in source order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BrepAttributes {
    /// Decoded attributes in document order.
    pub attributes: Vec<BrepAttribute>,
}

/// Extract B-Rep attributes from a complete `X_T` document.
///
/// Only exact built-in profile revisions with reviewed attribute layouts are
/// accepted. Attribute names and custom schema labels are never used to infer
/// record roles.
///
/// # Errors
/// Returns a parse error when the attribute schema is unsupported or the
/// document contains malformed attribute references or values.
pub fn map_xt_brep_attributes(document: &XtDocument) -> Result<BrepAttributes, ParseError> {
    extract_attributes(
        &document.nodes,
        &document.schema_provider,
        document.schema_key.raw(),
        &document.schemas,
    )
}

/// Extract B-Rep attributes from a complete `X_B` document.
///
/// # Errors
/// Returns a parse error when the attribute schema is unsupported or the
/// document contains malformed attribute references or values.
pub fn map_xb_brep_attributes(document: &XbDocument) -> Result<BrepAttributes, ParseError> {
    extract_attributes(
        &document.nodes,
        &document.schema_provider,
        document.schema_key.raw(),
        &document.schemas,
    )
}

fn extract_attributes(
    nodes: &[RawNode],
    provenance: &SchemaProviderResolution,
    key: &str,
    schemas: &[SchemaResolution],
) -> Result<BrepAttributes, ParseError> {
    let roles = attribute_roles(nodes, provenance, key, schemas)?;
    let by_index = nodes
        .iter()
        .map(|node| (node.index, node))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut attributes = Vec::new();
    for node in nodes
        .iter()
        .filter(|node| roles.type_name(node) == "ATTRIBUTE")
    {
        require_field(node, 0, crate::FieldType::Integer, 0)?;
        require_field(node, 1, crate::FieldType::PointerIndex, 80)?;
        require_field(node, 2, crate::FieldType::PointerIndex, 1015)?;
        require_field(node, 7, crate::FieldType::PointerIndex, 1018)?;
        let (definition, owner) = (
            linked_node(node, 1, &by_index, "definition")?,
            linked_node(node, 2, &by_index, "owner")?,
        );
        let identifier_node = linked_node(definition, 1, &by_index, "identifier")?;
        if roles.type_name(identifier_node) != "ATT_DEF_ID" {
            return Err(invalid(
                node,
                "attribute definition identifier has the wrong node type",
            ));
        }
        require_field(identifier_node, 0, crate::FieldType::Character, 0)?;
        require_field(definition, 1, crate::FieldType::PointerIndex, 79)?;
        require_field(definition, 2, crate::FieldType::Integer, 0)?;
        let value_kinds = value_kinds_field(definition)?;
        let identifier = character_bytes(field(identifier_node, 0, "string")?, node)?;
        let kind_id = field(definition, 2, "type_id")?
            .values
            .first()
            .cloned()
            .ok_or_else(|| invalid(definition, "attribute definition is missing its type id"))?;
        let declared = decode_declared_kinds(definition, value_kinds)?;
        let value_links = pointer_values(field(node, 7, "fields")?, node)?;
        if value_links.len() > declared.len() {
            return Err(invalid(
                node,
                "attribute value count does not match its definition",
            ));
        }
        let padding = &declared[value_links.len()..];
        if (!padding.is_empty() && definition.user_fields.is_empty())
            || padding.iter().any(|kind| *kind != 0)
        {
            return Err(invalid(
                definition,
                "attribute definition has nonzero or unexpected trailing value kinds",
            ));
        }
        let mut value_nodes = Vec::new();
        let fields = declared
            .into_iter()
            .take(value_links.len())
            .zip(value_links)
            .map(|(declared_kind, target)| {
                let value = target
                    .map(|index| {
                        let value_node = by_index.get(&index).copied().ok_or_else(|| {
                            invalid(node, "attribute value pointer is unresolved")
                        })?;
                        let raw = AttributeValueNode {
                            source: source_ref(value_node),
                            fields: value_node.fields.clone(),
                        };
                        let typed = decode_value(value_node, roles.type_name(value_node))?;
                        Ok((typed, raw))
                    })
                    .transpose()?;
                let (value, raw_node) = value
                    .map_or((AttributeFieldData::Empty, None), |(value, raw)| {
                        (value, Some(raw))
                    });
                value_nodes.push(raw_node);
                Ok(AttributeFieldValue {
                    declared_kind: Some(declared_kind),
                    value,
                })
            })
            .collect::<Result<Vec<_>, ParseError>>()?;
        attributes.push(build_attribute(
            node,
            owner,
            definition,
            identifier,
            kind_id,
            fields,
            value_nodes,
        )?);
    }
    Ok(BrepAttributes { attributes })
}

fn attribute_roles(
    nodes: &[RawNode],
    provenance: &SchemaProviderResolution,
    key: &str,
    schemas: &[SchemaResolution],
) -> Result<RoleAccess, ParseError> {
    if matches!(provenance, SchemaProviderResolution::CallerSupplied) {
        super::profile_roles::validate_named_attribute_roles(key, schemas)?;
    }
    let roles = RoleAccess::select(provenance, key, schemas)?;
    if nodes
        .iter()
        .any(|node| node.node_type == 81 && roles.type_name(node) != "ATTRIBUTE")
    {
        return Err(ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            "profile contains attribute nodes without a reviewed attribute role",
            ErrorDetails::None,
        ));
    }
    Ok(roles)
}

fn value_kinds_field(definition: &RawNode) -> Result<&RawField, ParseError> {
    let value_kinds = definition.fields.last().ok_or_else(|| {
        invalid(
            definition,
            "attribute definition is missing its value kinds",
        )
    })?;
    if value_kinds.definition.field_type != crate::FieldType::UnsignedByte
        || value_kinds.definition.pointer_class != 0
    {
        return Err(invalid(
            definition,
            "attribute definition value kinds have the wrong schema role",
        ));
    }
    Ok(value_kinds)
}

fn decode_declared_kinds(
    definition: &RawNode,
    value_kinds: &RawField,
) -> Result<Vec<u8>, ParseError> {
    let declared = value_kinds
        .values
        .iter()
        .map(|value| match value {
            FieldValue::UnsignedByte(kind) => Ok(*kind),
            _ => Err(invalid(
                definition,
                "attribute field kind is not an unsigned byte",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(declared)
}

fn build_attribute(
    node: &RawNode,
    owner: &RawNode,
    definition: &RawNode,
    identifier: Vec<u8>,
    kind_id: FieldValue,
    fields: Vec<AttributeFieldValue>,
    value_nodes: Vec<Option<AttributeValueNode>>,
) -> Result<BrepAttribute, ParseError> {
    let node_id = field(node, 0, "node_id")?
        .values
        .first()
        .cloned()
        .ok_or_else(|| invalid(node, "attribute is missing its node id"))?;
    Ok(BrepAttribute {
        source: source_ref(node),
        node_id,
        kind_id,
        identifier,
        owner: source_ref(owner),
        definition: source_ref(definition),
        fields,
        value_nodes,
    })
}

fn decode_value(node: &RawNode, node_type: &str) -> Result<AttributeFieldData, ParseError> {
    let field = node
        .fields
        .first()
        .ok_or_else(|| invalid(node, "attribute value node is missing its values field"))?;
    let mismatch = || {
        invalid(
            node,
            "attribute declared kind does not match its value node type or codec",
        )
    };
    match node_type {
        "INT_VALUES" => field
            .values
            .iter()
            .map(|v| match v {
                FieldValue::Integer(v) => Ok(*v),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(AttributeFieldData::Integers),
        "REAL_VALUES" => field
            .values
            .iter()
            .map(|v| match v {
                FieldValue::Double(v) => Ok(*v),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(AttributeFieldData::Reals),
        "CHAR_VALUES" => field
            .values
            .iter()
            .map(|v| match v {
                FieldValue::Character(v) => Ok(*v),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(AttributeFieldData::Text),
        "POINT_VALUES" => vectors(field, node).map(AttributeFieldData::Points),
        "VECTOR_VALUES" => vectors(field, node).map(AttributeFieldData::Vectors),
        "DIRECTION_VALUES" => vectors(field, node).map(AttributeFieldData::Directions),
        "AXIS_VALUES" => vectors(field, node).map(AttributeFieldData::Axes),
        "TAG_VALUES" => field
            .values
            .iter()
            .map(|v| match v {
                FieldValue::Tag(v) => Ok(*v),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(AttributeFieldData::Tags),
        "UNICODE_VALUES" => field
            .values
            .iter()
            .map(|v| match v {
                FieldValue::UnicodeCharacter(v) => Ok(*v),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(AttributeFieldData::Unicode),
        _ => Ok(AttributeFieldData::Unsupported {
            node_type: node.node_type,
        }),
    }
}

fn vectors(field: &RawField, node: &RawNode) -> Result<Vec<[Option<f64>; 3]>, ParseError> {
    field
        .values
        .iter()
        .map(|value| match value {
            FieldValue::Vector(vector) => Ok(*vector),
            _ => Err(invalid(node, "attribute vector value has the wrong codec")),
        })
        .collect()
}

fn field<'a>(node: &'a RawNode, ordinal: usize, role: &str) -> Result<&'a RawField, ParseError> {
    node.fields
        .get(ordinal)
        .ok_or_else(|| invalid(node, &format!("missing attribute {role} field")))
}

fn require_field(
    node: &RawNode,
    ordinal: usize,
    kind: crate::FieldType,
    pointer_class: u16,
) -> Result<(), ParseError> {
    let field = field(node, ordinal, "schema")?;
    if field.definition.field_type != kind || field.definition.pointer_class != pointer_class {
        return Err(invalid(
            node,
            "attribute field does not match the reviewed schema role",
        ));
    }
    Ok(())
}

fn linked_node<'a>(
    node: &RawNode,
    ordinal: usize,
    nodes: &'a std::collections::BTreeMap<u32, &'a RawNode>,
    role: &str,
) -> Result<&'a RawNode, ParseError> {
    let target = match field(node, ordinal, role)?.values.first() {
        Some(FieldValue::PointerIndex(index)) if *index != 0 => *index,
        _ => {
            return Err(invalid(
                node,
                &format!("attribute {role} link is null or malformed"),
            ));
        }
    };
    nodes
        .get(&target)
        .copied()
        .ok_or_else(|| invalid(node, &format!("attribute {role} link is unresolved")))
}

fn pointer_values(field: &RawField, node: &RawNode) -> Result<Vec<Option<u32>>, ParseError> {
    field
        .values
        .iter()
        .map(|value| match value {
            FieldValue::PointerIndex(0) => Ok(None),
            FieldValue::PointerIndex(index) => Ok(Some(*index)),
            _ => Err(invalid(node, "attribute field link is not a pointer")),
        })
        .collect()
}

fn character_bytes(field: &RawField, node: &RawNode) -> Result<Vec<u8>, ParseError> {
    field
        .values
        .iter()
        .map(|value| match value {
            FieldValue::Character(byte) => Ok(*byte),
            _ => Err(invalid(
                node,
                "attribute identifier contains a non-character value",
            )),
        })
        .collect()
}

fn source_ref(node: &RawNode) -> SourceNodeRef {
    SourceNodeRef {
        node_index: node.index,
        node_type: node.node_type,
        type_name: node.definition.name.clone(),
        node_id: None,
        byte_range: node.byte_range.clone(),
    }
}

fn invalid(node: &RawNode, message: &str) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBrepField,
        node.byte_range.start,
        message,
        ErrorDetails::NodeIndex {
            node_index: node.index,
        },
    )
}
