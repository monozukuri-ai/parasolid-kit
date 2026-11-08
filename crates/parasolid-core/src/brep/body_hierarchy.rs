//! Opt-in raw BODY relationship fields for explicitly named schemas.

use std::collections::HashSet;

use crate::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, BuiltinSchemaProvider, ErrorDetails, ErrorKind,
    FieldType, FieldValue, ParseError, RawField, RawNode, SchemaKey, SchemaProvider,
};

use super::model::SourceNodeRef;

/// Explicit mapping from relationship roles to names in one BODY schema.
///
/// Callers select the field names from a reviewed schema definition. This
/// decoder does not infer numeric field positions or walk sibling chains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyHierarchyFieldNames {
    pub child_head: &'static str,
    pub owner: &'static str,
    pub next: &'static str,
    pub previous: &'static str,
    pub lowest_id: &'static str,
    pub highest_id: &'static str,
}

/// Raw hierarchy fields directly declared on one BODY node.
///
/// Pointer indices are retained without assigning an owner type or deciding
/// whether sibling links terminate at null or form a ring.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyHierarchyData {
    pub child_head: Option<u32>,
    pub owner: Option<u32>,
    pub next: Option<u32>,
    pub previous: Option<u32>,
    pub lowest_id: Option<i32>,
    pub highest_id: Option<i32>,
    pub source: SourceNodeRef,
}

impl BodyHierarchyData {
    /// Resolves the child head, treating an absent transmit target as XT null.
    ///
    /// # Errors
    /// Returns an error if node indices are ambiguous.
    pub fn effective_child_head(&self, nodes: &[RawNode]) -> Result<Option<u32>, ParseError> {
        effective_pointer(nodes, self.child_head)
    }

    /// Lists nonzero relationship indices that have no node in the supplied document.
    ///
    /// XT interprets unresolved indices as zero; the original indices remain in
    /// this value and in the source `RawNode` for diagnostics or later checks.
    ///
    /// # Errors
    /// Returns an error if node indices are ambiguous.
    pub fn unresolved_references(
        &self,
        nodes: &[RawNode],
        names: BodyHierarchyFieldNames,
    ) -> Result<Vec<(&'static str, u32)>, ParseError> {
        let mut unresolved = Vec::new();
        for (field, index) in [
            (names.child_head, self.child_head),
            (names.owner, self.owner),
            (names.next, self.next),
            (names.previous, self.previous),
        ] {
            if let Some(index) = index {
                if node_by_index(nodes, index)?.is_none() {
                    unresolved.push((field, index));
                }
            }
        }
        Ok(unresolved)
    }

    /// Decode BODY hierarchy fields under an exact reviewed `TopSolid` v6 profile.
    ///
    /// Only `TopSolid` keys whose compiled effective BODY definition contains
    /// the complete reviewed hierarchy roles are accepted. The owner target's
    /// type remains unclassified.
    ///
    /// # Errors
    /// Returns an error for an unsupported key, uncovered BODY layout, schema
    /// mismatch, malformed relationship fields, or invalid referenced BODY nodes.
    pub fn decode_builtin_topsolid(
        nodes: &[RawNode],
        schema_key: &str,
        body_index: u32,
    ) -> Result<Self, ParseError> {
        with_builtin_topsolid_provider(schema_key, |key, provider| {
            validate_builtin_body_node(provider, key, nodes, body_index)?;
            let data =
                decode_named_body_hierarchy(nodes, body_index, TOPSOLID_BODY_HIERARCHY_FIELDS)?;
            for index in [data.child_head, data.next, data.previous]
                .into_iter()
                .flatten()
            {
                if node_by_index(nodes, index)?.is_some() {
                    validate_builtin_body_node(provider, key, nodes, index)?;
                }
            }
            Ok(data)
        })
    }

    /// Decode ordered child BODY links under an exact reviewed `TopSolid` profile.
    ///
    /// The caller must select whether the chain is null-terminated or circular.
    ///
    /// # Errors
    /// Returns an error for an unsupported key, uncovered BODY layout, schema
    /// mismatch, or an invalid child chain.
    pub fn decode_builtin_topsolid_children(
        nodes: &[RawNode],
        schema_key: &str,
        parent_index: u32,
        termination: BodyChildChainTermination,
    ) -> Result<Vec<u32>, ParseError> {
        with_builtin_topsolid_provider(schema_key, |key, provider| {
            validate_builtin_body_node(provider, key, nodes, parent_index)?;
            let children = decode_named_body_children(
                nodes,
                parent_index,
                TOPSOLID_BODY_HIERARCHY_FIELDS,
                termination,
            )?;
            for index in std::iter::once(parent_index).chain(children.iter().copied()) {
                validate_builtin_body_node(provider, key, nodes, index)?;
                let data =
                    decode_named_body_hierarchy(nodes, index, TOPSOLID_BODY_HIERARCHY_FIELDS)?;
                for target in [data.child_head, data.next, data.previous]
                    .into_iter()
                    .flatten()
                {
                    if node_by_index(nodes, target)?.is_some() {
                        validate_builtin_body_node(provider, key, nodes, target)?;
                    }
                }
            }
            Ok(children)
        })
    }
}

/// Caller-selected sibling-chain termination semantics for ordered traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyChildChainTermination {
    /// The last child's effective `next` and first child's effective `previous` must be null.
    /// XT-unresolved indices are effective nulls but remain visible in raw hierarchy data.
    NullTerminated,
    /// The last child's effective `next` and first child's effective `previous` must point to each other.
    Circular,
}

const TOPSOLID_BODY_HIERARCHY_FIELDS: BodyHierarchyFieldNames = BodyHierarchyFieldNames {
    child_head: "child",
    owner: "owner",
    next: "next",
    previous: "previous",
    lowest_id: "lowest_node_id",
    highest_id: "highest_node_id",
};

fn validate_builtin_body_node(
    provider: &impl SchemaProvider,
    key: &SchemaKey,
    nodes: &[RawNode],
    body_index: u32,
) -> Result<(), ParseError> {
    let body = node_by_index(nodes, body_index)?.ok_or_else(|| {
        ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            format!("BODY node {body_index} does not exist"),
            ErrorDetails::NodeIndex {
                node_index: body_index,
            },
        )
    })?;
    if body.definition.name != "BODY" {
        return Err(invalid_field(body, "node_type", "node is not named BODY"));
    }
    let schema = key.effective();
    let expected = provider
        .type_definition(schema, body.node_type)
        .ok_or_else(|| uncovered_body_layout(schema, body))?;
    if expected.name != "BODY" {
        return Err(invalid_field(
            body,
            "schema_layout",
            "profile does not name this node type BODY",
        ));
    }
    for (name, field_type) in [
        (
            TOPSOLID_BODY_HIERARCHY_FIELDS.child_head,
            FieldType::PointerIndex,
        ),
        (
            TOPSOLID_BODY_HIERARCHY_FIELDS.owner,
            FieldType::PointerIndex,
        ),
        (TOPSOLID_BODY_HIERARCHY_FIELDS.next, FieldType::PointerIndex),
        (
            TOPSOLID_BODY_HIERARCHY_FIELDS.previous,
            FieldType::PointerIndex,
        ),
        (TOPSOLID_BODY_HIERARCHY_FIELDS.lowest_id, FieldType::Integer),
        (
            TOPSOLID_BODY_HIERARCHY_FIELDS.highest_id,
            FieldType::Integer,
        ),
    ] {
        let mut fields = expected.fields.iter().filter(|field| field.name == name);
        let Some(field) = fields.next() else {
            return Err(uncovered_body_layout(schema, body));
        };
        if fields.next().is_some() || field.field_type != field_type {
            return Err(uncovered_body_layout(schema, body));
        }
    }
    if body.definition.node_type != expected.node_type
        || body.definition.name != expected.name
        || body.definition.variable != expected.variable
        || body.definition.fields != expected.fields
        || body.fields.len() != expected.fields.len()
        || body
            .fields
            .iter()
            .zip(&expected.fields)
            .any(|(actual, expected)| actual.definition.as_ref() != expected)
    {
        return Err(invalid_field(
            body,
            "schema_layout",
            "BODY node does not match the reviewed TopSolid layout",
        ));
    }
    Ok(())
}

fn uncovered_body_layout(schema: &str, body: &RawNode) -> ParseError {
    ParseError::new(
        ErrorKind::BuiltinProfileUncoveredType,
        body.byte_range.start,
        "TopSolid profile does not provide the complete BODY hierarchy layout",
        ErrorDetails::SchemaLookup {
            schema: schema.to_owned(),
            node_type: body.node_type,
        },
    )
}

fn unsupported_builtin_key(schema_key: &str) -> ParseError {
    ParseError::new(
        ErrorKind::UnsupportedBuiltinSchemaKey,
        0,
        "BODY hierarchy decoding requires an exact compiled TopSolid v6 schema key",
        ErrorDetails::InvalidText {
            field: "schema_key",
            value: schema_key.to_owned(),
        },
    )
}

fn with_builtin_topsolid_provider<T>(
    schema_key: &str,
    decode: impl FnOnce(&SchemaKey, &BuiltinSchemaProvider<'_>) -> Result<T, ParseError>,
) -> Result<T, ParseError> {
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
    decode(&key, &provider)
}

/// Decode the named relationship fields from one explicitly described BODY layout.
///
/// Present child, next, and previous targets must be named BODY nodes. A
/// nonzero unresolved index is retained because XT interprets it as zero;
/// [`BodyHierarchyData::unresolved_references`] exposes such indices. The owner
/// target type remains unclassified because ownership can refer to a world or
/// another body. This function does not traverse links or infer chain semantics.
///
/// # Errors
/// Returns an error for duplicate node indices, missing or non-BODY source nodes,
/// duplicate field names, malformed fields, or references to a non-BODY node.
pub fn decode_named_body_hierarchy(
    nodes: &[RawNode],
    body_index: u32,
    names: BodyHierarchyFieldNames,
) -> Result<BodyHierarchyData, ParseError> {
    let role_names = [
        names.child_head,
        names.owner,
        names.next,
        names.previous,
        names.lowest_id,
        names.highest_id,
    ];
    if role_names
        .iter()
        .enumerate()
        .any(|(index, name)| role_names[..index].contains(name))
    {
        return Err(ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            "named BODY hierarchy roles must refer to distinct fields",
            ErrorDetails::InvalidText {
                field: "body_hierarchy_fields",
                value: role_names.join(","),
            },
        ));
    }

    let body = node_by_index(nodes, body_index)?.ok_or_else(|| {
        ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            format!("BODY node {body_index} does not exist"),
            ErrorDetails::NodeIndex {
                node_index: body_index,
            },
        )
    })?;
    if body.definition.name != "BODY" {
        return Err(invalid_field(body, "node_type", "node is not named BODY"));
    }

    let child_head = pointer(body, names.child_head)?;
    let owner = pointer(body, names.owner)?;
    let next = pointer(body, names.next)?;
    let previous = pointer(body, names.previous)?;
    let lowest_id = integer(body, names.lowest_id)?;
    let highest_id = integer(body, names.highest_id)?;

    for (field_name, target_index) in [
        (names.child_head, child_head),
        (names.next, next),
        (names.previous, previous),
    ] {
        if let Some(target_index) = target_index {
            if let Some(target) = node_by_index(nodes, target_index)? {
                if target.definition.name != "BODY" {
                    return Err(invalid_reference(
                        body,
                        field_name,
                        target_index,
                        "BODY",
                        format!(
                            "pointer targets {} rather than BODY",
                            target.definition.name
                        ),
                    ));
                }
            }
        }
    }
    if let Some(owner_index) = owner {
        let _ = node_by_index(nodes, owner_index)?;
    }

    Ok(BodyHierarchyData {
        child_head,
        owner,
        next,
        previous,
        lowest_id,
        highest_id,
        source: source(body),
    })
}

/// Decode a compound's ordered child chain under explicitly selected termination semantics.
///
/// Every visited child must be a unique named BODY whose effective owner points
/// to the requested parent. Each effective `previous` pointer must reciprocate
/// the preceding child's effective `next` pointer. XT-unresolved indices act as
/// null here; no termination convention is selected implicitly.
///
/// # Errors
/// Returns an error for duplicate nodes, invalid BODY references, inconsistent
/// owner or sibling links, a cycle, or a chain that violates the selected
/// termination semantics.
pub fn decode_named_body_children(
    nodes: &[RawNode],
    parent_index: u32,
    names: BodyHierarchyFieldNames,
    termination: BodyChildChainTermination,
) -> Result<Vec<u32>, ParseError> {
    validate_unique_node_indices(nodes)?;
    let parent = node_by_index(nodes, parent_index)?.ok_or_else(|| {
        ParseError::new(
            ErrorKind::InvalidBrepField,
            0,
            format!("BODY node {parent_index} does not exist"),
            ErrorDetails::NodeIndex {
                node_index: parent_index,
            },
        )
    })?;
    let hierarchy = decode_named_body_hierarchy(nodes, parent_index, names)?;
    let Some(head) = hierarchy.effective_child_head(nodes)? else {
        return Ok(Vec::new());
    };

    let mut children = Vec::new();
    let mut current = head;
    let mut previous_index = None;
    let mut traversal = NamedBodyChildTraversal {
        nodes,
        parent,
        parent_index,
        names,
        termination,
        head,
        head_previous: None,
        seen: HashSet::new(),
    };

    loop {
        let (child, child_data) = traversal.validate_child(current, previous_index)?;
        children.push(current);

        let Some(next) = traversal.next_child(child, current, &child_data)? else {
            return Ok(children);
        };
        previous_index = Some(current);
        current = next;
    }
}

struct NamedBodyChildTraversal<'a> {
    nodes: &'a [RawNode],
    parent: &'a RawNode,
    parent_index: u32,
    names: BodyHierarchyFieldNames,
    termination: BodyChildChainTermination,
    head: u32,
    head_previous: Option<u32>,
    seen: HashSet<u32>,
}

impl<'a> NamedBodyChildTraversal<'a> {
    fn validate_child(
        &mut self,
        current: u32,
        previous_index: Option<u32>,
    ) -> Result<(&'a RawNode, BodyHierarchyData), ParseError> {
        if current == self.parent_index {
            return Err(invalid_reference(
                self.parent,
                self.names.child_head,
                current,
                "child BODY distinct from its parent",
                "compound BODY cannot be its own child",
            ));
        }
        if !self.seen.insert(current) {
            return Err(invalid_reference(
                self.parent,
                self.names.child_head,
                current,
                "acyclic BODY child chain",
                "child chain repeats before the caller-selected endpoint",
            ));
        }
        let child = node_by_index(self.nodes, current)?.ok_or_else(|| {
            invalid_reference(
                self.parent,
                self.names.child_head,
                current,
                "BODY",
                "child chain target is unresolved",
            )
        })?;
        let child_data = decode_named_body_hierarchy(self.nodes, current, self.names)?;
        if effective_pointer(self.nodes, child_data.owner)? != Some(self.parent_index) {
            return Err(invalid_reference(
                child,
                self.names.owner,
                child_data.owner.unwrap_or(0),
                "parent BODY",
                "child owner does not point to the requested parent",
            ));
        }
        let child_previous = effective_pointer(self.nodes, child_data.previous)?;
        if let Some(previous_index) = previous_index {
            if child_previous != Some(previous_index) {
                return Err(invalid_reference(
                    child,
                    self.names.previous,
                    child_data.previous.unwrap_or(0),
                    "preceding BODY sibling",
                    "previous link does not reciprocate the preceding child's next link",
                ));
            }
        } else {
            self.head_previous = child_previous;
            if self.termination == BodyChildChainTermination::NullTerminated
                && self.head_previous.is_some()
            {
                return Err(invalid_reference(
                    child,
                    self.names.previous,
                    self.head_previous.unwrap_or(0),
                    "null previous link at child head",
                    "null-terminated child chain has a non-null head previous link",
                ));
            }
        }
        Ok((child, child_data))
    }

    fn next_child(
        &self,
        child: &RawNode,
        current: u32,
        child_data: &BodyHierarchyData,
    ) -> Result<Option<u32>, ParseError> {
        match (
            effective_pointer(self.nodes, child_data.next)?,
            self.termination,
        ) {
            (None, BodyChildChainTermination::NullTerminated) => Ok(None),
            (None, BodyChildChainTermination::Circular) => Err(invalid_reference(
                child,
                self.names.next,
                0,
                "circular BODY sibling chain",
                "circular child chain terminates at null before returning to its head",
            )),
            (Some(next), BodyChildChainTermination::NullTerminated) if next == self.head => {
                Err(invalid_reference(
                    child,
                    self.names.next,
                    next,
                    "null-terminated BODY sibling chain",
                    "null-terminated child chain closes back to its head",
                ))
            }
            (Some(next), _) if next == self.head => {
                if self.head_previous != Some(current) {
                    return Err(invalid_reference(
                        self.parent,
                        self.names.previous,
                        self.head_previous.unwrap_or(0),
                        "last BODY sibling",
                        "circular child head previous link does not point to the last sibling",
                    ));
                }
                Ok(None)
            }
            (Some(next), _) if self.seen.contains(&next) => Err(invalid_reference(
                child,
                self.names.next,
                next,
                "acyclic BODY child chain",
                "child chain contains a cycle that does not return to its head",
            )),
            (Some(next), _) => Ok(Some(next)),
        }
    }
}

fn validate_unique_node_indices(nodes: &[RawNode]) -> Result<(), ParseError> {
    let mut seen = HashSet::with_capacity(nodes.len());
    for node in nodes {
        if !seen.insert(node.index) {
            return Err(ParseError::new(
                ErrorKind::DuplicateNodeIndex,
                node.byte_range.start,
                format!("node index {} is ambiguous", node.index),
                ErrorDetails::NodeIndex {
                    node_index: node.index,
                },
            ));
        }
    }
    Ok(())
}

fn node_by_index(nodes: &[RawNode], index: u32) -> Result<Option<&RawNode>, ParseError> {
    let mut matching = nodes.iter().filter(|node| node.index == index);
    let Some(node) = matching.next() else {
        return Ok(None);
    };
    if let Some(duplicate) = matching.next() {
        return Err(ParseError::new(
            ErrorKind::DuplicateNodeIndex,
            duplicate.byte_range.start,
            format!("node index {index} is ambiguous"),
            ErrorDetails::NodeIndex { node_index: index },
        ));
    }
    Ok(Some(node))
}

fn required_field<'a>(
    node: &'a RawNode,
    name: &'static str,
    expected_type: FieldType,
) -> Result<&'a RawField, ParseError> {
    let mut declarations = node
        .definition
        .fields
        .iter()
        .filter(|field| field.name == name);
    let declaration = declarations
        .next()
        .ok_or_else(|| invalid_field(node, name, "field is not declared by the named layout"))?;
    if declarations.next().is_some() {
        return Err(invalid_field(node, name, "field declaration is ambiguous"));
    }
    if declaration.field_type != expected_type {
        return Err(invalid_field(
            node,
            name,
            "declared field has the wrong type",
        ));
    }

    let mut values = node
        .fields
        .iter()
        .filter(|field| field.definition.name == name);
    let field = values
        .next()
        .ok_or_else(|| invalid_field(node, name, "declared field value is missing"))?;
    if values.next().is_some() {
        return Err(invalid_field(node, name, "field value is ambiguous"));
    }
    if field.definition.as_ref() != declaration || field.values.len() != 1 {
        return Err(invalid_field(
            node,
            name,
            "field value does not match its single declared scalar",
        ));
    }
    if field.values[0].field_type() != expected_type {
        return Err(invalid_field(node, name, "field value has the wrong type"));
    }
    Ok(field)
}

fn pointer(node: &RawNode, name: &'static str) -> Result<Option<u32>, ParseError> {
    let field = required_field(node, name, FieldType::PointerIndex)?;
    match &field.values[0] {
        FieldValue::PointerIndex(index) => Ok((*index != 0).then_some(*index)),
        _ => Err(invalid_field(node, name, "field value is not a pointer")),
    }
}

fn effective_pointer(nodes: &[RawNode], index: Option<u32>) -> Result<Option<u32>, ParseError> {
    let Some(index) = index else {
        return Ok(None);
    };
    Ok(node_by_index(nodes, index)?.map(|_| index))
}

fn integer(node: &RawNode, name: &'static str) -> Result<Option<i32>, ParseError> {
    let field = required_field(node, name, FieldType::Integer)?;
    match &field.values[0] {
        FieldValue::Integer(value) => Ok(*value),
        _ => Err(invalid_field(node, name, "field value is not an integer")),
    }
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
    clippy::too_many_lines,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use std::ops::Range;

    use super::*;
    use crate::{FieldDefinition, SchemaSource, TypeDefinition};

    const NAMES: BodyHierarchyFieldNames = BodyHierarchyFieldNames {
        child_head: "child",
        owner: "owner",
        next: "next",
        previous: "previous",
        lowest_id: "lowest_node_id",
        highest_id: "highest_node_id",
    };

    fn error_kind<T>(result: Result<T, ParseError>) -> Option<ErrorKind> {
        result.err().map(|error| error.kind())
    }

    fn body(index: u32, child: u32, owner: u32, next: u32, previous: u32) -> RawNode {
        node(
            index,
            "BODY",
            vec![
                ("child", FieldValue::PointerIndex(child)),
                ("owner", FieldValue::PointerIndex(owner)),
                ("next", FieldValue::PointerIndex(next)),
                ("previous", FieldValue::PointerIndex(previous)),
                ("lowest_node_id", FieldValue::Integer(Some(10))),
                ("highest_node_id", FieldValue::Integer(Some(20))),
            ],
        )
    }

    fn node(index: u32, name: &str, values: Vec<(&str, FieldValue)>) -> RawNode {
        let definitions = values
            .iter()
            .map(|(name, value)| FieldDefinition {
                name: (*name).to_owned(),
                field_type: value.field_type(),
                pointer_class: 0,
                element_count: 0,
                transmitted: true,
            })
            .collect::<Vec<_>>();
        let fields = definitions
            .iter()
            .cloned()
            .zip(values.into_iter().map(|(_, value)| value))
            .map(|(definition, value)| RawField {
                definition: definition.into(),
                values: vec![value],
                byte_range: 0..1,
            })
            .collect();
        RawNode {
            node_type: 12,
            index,
            variable_length: None,
            definition: TypeDefinition::from_fields(
                12,
                name,
                name,
                definitions,
                SchemaSource::Base,
            )
            .into(),
            first_schema: None,
            fields,
            byte_range: Range {
                start: index as usize,
                end: index as usize + 1,
            },
            user_fields: Vec::new(),
        }
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "Fixture schemas are reviewed compiled profiles and must contain their expected BODY definition."
    )]
    fn builtin_body_definition(key: &SchemaKey) -> TypeDefinition {
        let registry = BuiltinProfileRegistry::compiled().unwrap();
        let provider = registry.provider_for_key(key).unwrap();
        let find_body = |schema| {
            (0..=u16::MAX).find_map(|node_type| {
                provider
                    .type_definition(schema, node_type)
                    .filter(|definition| definition.name == "BODY")
                    .cloned()
            })
        };
        find_body(key.effective())
            .or_else(|| key.base().and_then(find_body))
            .expect("compiled TopSolid BODY definition")
    }

    #[allow(
        clippy::unwrap_used,
        reason = "Test fixture variable-field values are small controlled vectors whose lengths fit the wire u32 field."
    )]
    fn builtin_body(
        key: &SchemaKey,
        index: u32,
        child: u32,
        owner: u32,
        next: u32,
        previous: u32,
    ) -> RawNode {
        let definition = builtin_body_definition(key);
        let overrides = [
            ("child", FieldValue::PointerIndex(child)),
            ("owner", FieldValue::PointerIndex(owner)),
            ("next", FieldValue::PointerIndex(next)),
            ("previous", FieldValue::PointerIndex(previous)),
        ];
        let values = definition
            .fields
            .iter()
            .map(|field| {
                overrides
                    .iter()
                    .find(|(name, _)| *name == field.name)
                    .map_or_else(
                        || vec![builtin_default(field.field_type)],
                        |(_, value)| vec![value.clone()],
                    )
            })
            .collect::<Vec<_>>();
        let variable_length = definition
            .variable
            .then(|| u32::try_from(values.last().map_or(0, Vec::len)).unwrap());
        let fields = definition
            .fields
            .iter()
            .cloned()
            .zip(values)
            .map(|(definition, values)| RawField {
                definition: definition.into(),
                values,
                byte_range: 0..1,
            })
            .collect();
        RawNode {
            node_type: definition.node_type,
            index,
            variable_length,
            fields,
            definition: definition.into(),
            first_schema: None,
            byte_range: Range {
                start: index as usize,
                end: index as usize + 1,
            },
            user_fields: Vec::new(),
        }
    }

    fn builtin_default(field_type: FieldType) -> FieldValue {
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

    #[test]
    fn preserves_raw_links_and_id_values_without_classifying_owner_or_chain()
    -> Result<(), ParseError> {
        let nodes = vec![
            body(1, 2, 3, 0, 0),
            body(2, 0, 1, 4, 0),
            node(3, "WORLD", vec![]),
            body(4, 0, 1, 0, 2),
        ];
        let data = decode_named_body_hierarchy(&nodes, 1, NAMES)?;
        assert_eq!(data.child_head, Some(2));
        assert_eq!(data.owner, Some(3));
        assert_eq!(data.next, None);
        assert_eq!(data.previous, None);
        assert_eq!(data.lowest_id, Some(10));
        assert_eq!(data.highest_id, Some(20));
        assert_eq!(data.source.node_index, 1);
        Ok(())
    }

    #[test]
    fn rejects_missing_or_wrongly_typed_named_fields() {
        let mut missing = body(1, 0, 0, 0, 0);
        missing
            .fields
            .retain(|field| field.definition.name != "child");
        assert_eq!(
            error_kind(decode_named_body_hierarchy(&[missing], 1, NAMES)),
            Some(ErrorKind::InvalidBrepField)
        );

        let mut wrong_type = body(1, 0, 0, 0, 0);
        let field = wrong_type
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "child");
        assert!(field.is_some(), "BODY fixture must contain a child field");
        if let Some(field) = field {
            field.values[0] = FieldValue::Integer(Some(0));
        }
        assert_eq!(
            error_kind(decode_named_body_hierarchy(&[wrong_type], 1, NAMES)),
            Some(ErrorKind::InvalidBrepField)
        );

        let mut duplicate = body(1, 0, 0, 0, 0);
        duplicate.fields.push(duplicate.fields[0].clone());
        assert_eq!(
            error_kind(decode_named_body_hierarchy(&[duplicate], 1, NAMES)),
            Some(ErrorKind::InvalidBrepField)
        );
    }

    #[test]
    fn traverses_null_terminated_children_in_source_link_order() -> Result<(), ParseError> {
        let nodes = vec![
            body(1, 2, 0, 0, 0),
            body(2, 0, 1, 3, 0),
            body(3, 0, 1, 0, 2),
        ];

        let children = decode_named_body_children(
            &nodes,
            1,
            NAMES,
            BodyChildChainTermination::NullTerminated,
        )?;
        assert_eq!(children, vec![2, 3]);
        Ok(())
    }

    #[test]
    fn traverses_circular_children_in_source_link_order() -> Result<(), ParseError> {
        let nodes = vec![
            body(1, 2, 0, 0, 0),
            body(2, 0, 1, 3, 3),
            body(3, 0, 1, 2, 2),
        ];

        let children =
            decode_named_body_children(&nodes, 1, NAMES, BodyChildChainTermination::Circular)?;
        assert_eq!(children, vec![2, 3]);
        Ok(())
    }

    #[test]
    fn rejects_parent_as_child_for_both_chain_termination_modes() {
        let nodes = vec![body(1, 1, 1, 0, 0)];
        for termination in [
            BodyChildChainTermination::NullTerminated,
            BodyChildChainTermination::Circular,
        ] {
            assert_eq!(
                error_kind(decode_named_body_children(&nodes, 1, NAMES, termination)),
                Some(ErrorKind::InvalidBrepReference)
            );
        }
    }

    #[test]
    fn rejects_malformed_child_chains_under_selected_termination() -> Result<(), ParseError> {
        let null_chain = vec![body(1, 2, 0, 0, 0), body(2, 0, 1, 0, 0)];
        assert_eq!(
            error_kind(decode_named_body_children(
                &null_chain,
                1,
                NAMES,
                BodyChildChainTermination::Circular,
            )),
            Some(ErrorKind::InvalidBrepReference)
        );

        let circular_chain = vec![
            body(1, 2, 0, 0, 0),
            body(2, 0, 1, 3, 0),
            body(3, 0, 1, 2, 2),
        ];
        assert_eq!(
            error_kind(decode_named_body_children(
                &circular_chain,
                1,
                NAMES,
                BodyChildChainTermination::NullTerminated,
            )),
            Some(ErrorKind::InvalidBrepReference)
        );

        let bad_previous = vec![
            body(1, 2, 0, 0, 0),
            body(2, 0, 1, 3, 0),
            body(3, 0, 1, 0, 0),
        ];
        assert_eq!(
            error_kind(decode_named_body_children(
                &bad_previous,
                1,
                NAMES,
                BodyChildChainTermination::NullTerminated,
            )),
            Some(ErrorKind::InvalidBrepReference)
        );

        let wrong_owner = vec![body(1, 2, 0, 0, 0), body(2, 0, 99, 0, 0)];
        assert_eq!(
            error_kind(decode_named_body_children(
                &wrong_owner,
                1,
                NAMES,
                BodyChildChainTermination::NullTerminated,
            )),
            Some(ErrorKind::InvalidBrepReference)
        );

        let dangling_next = vec![body(1, 2, 0, 0, 0), body(2, 0, 1, 99, 0)];
        let children = decode_named_body_children(
            &dangling_next,
            1,
            NAMES,
            BodyChildChainTermination::NullTerminated,
        )?;
        assert_eq!(children, vec![2]);
        assert_eq!(
            error_kind(decode_named_body_children(
                &dangling_next,
                1,
                NAMES,
                BodyChildChainTermination::Circular,
            )),
            Some(ErrorKind::InvalidBrepReference)
        );

        let missing_head_previous = vec![body(1, 2, 0, 0, 0), body(2, 0, 1, 0, 99)];
        let children = decode_named_body_children(
            &missing_head_previous,
            1,
            NAMES,
            BodyChildChainTermination::NullTerminated,
        )?;
        assert_eq!(children, vec![2]);
        let missing_sibling_previous = vec![
            body(1, 2, 0, 0, 0),
            body(2, 0, 1, 3, 0),
            body(3, 0, 1, 0, 99),
        ];
        assert_eq!(
            error_kind(decode_named_body_children(
                &missing_sibling_previous,
                1,
                NAMES,
                BodyChildChainTermination::NullTerminated,
            )),
            Some(ErrorKind::InvalidBrepReference)
        );

        let wrong_target_type = vec![body(1, 2, 0, 0, 0), node(2, "EDGE", vec![])];
        assert_eq!(
            error_kind(decode_named_body_children(
                &wrong_target_type,
                1,
                NAMES,
                BodyChildChainTermination::NullTerminated,
            )),
            Some(ErrorKind::InvalidBrepReference)
        );

        let non_head_cycle = vec![
            body(1, 2, 0, 0, 0),
            body(2, 0, 1, 3, 0),
            body(3, 0, 1, 4, 2),
            body(4, 0, 1, 3, 3),
        ];
        assert_eq!(
            error_kind(decode_named_body_children(
                &non_head_cycle,
                1,
                NAMES,
                BodyChildChainTermination::Circular,
            )),
            Some(ErrorKind::InvalidBrepReference)
        );

        let duplicate_index = vec![
            body(1, 2, 0, 0, 0),
            body(2, 0, 1, 0, 0),
            body(2, 0, 1, 0, 0),
        ];
        assert_eq!(
            error_kind(decode_named_body_children(
                &duplicate_index,
                1,
                NAMES,
                BodyChildChainTermination::NullTerminated,
            )),
            Some(ErrorKind::DuplicateNodeIndex)
        );
        Ok(())
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "This test asserts both duplicate-target errors and retained unresolved source indices."
    )]
    fn rejects_ambiguous_source_or_link_targets_but_retains_unresolved_indices() {
        let duplicate_source = vec![body(1, 0, 0, 0, 0), body(1, 0, 0, 0, 0)];
        assert_eq!(
            decode_named_body_hierarchy(&duplicate_source, 1, NAMES)
                .unwrap_err()
                .kind(),
            ErrorKind::DuplicateNodeIndex
        );

        let duplicate_target = vec![
            body(1, 2, 0, 0, 0),
            body(2, 0, 0, 0, 0),
            body(2, 0, 0, 0, 0),
        ];
        assert_eq!(
            decode_named_body_hierarchy(&duplicate_target, 1, NAMES)
                .unwrap_err()
                .kind(),
            ErrorKind::DuplicateNodeIndex
        );

        let unresolved = vec![body(1, 2, 3, 4, 5)];
        let decoded = decode_named_body_hierarchy(&unresolved, 1, NAMES).unwrap();
        assert_eq!(decoded.child_head, Some(2));
        assert_eq!(decoded.owner, Some(3));
        assert_eq!(decoded.next, Some(4));
        assert_eq!(decoded.previous, Some(5));
        assert_eq!(
            decoded.unresolved_references(&unresolved, NAMES).unwrap(),
            vec![("child", 2), ("owner", 3), ("next", 4), ("previous", 5)]
        );
        assert_eq!(
            decode_named_body_children(
                &unresolved,
                1,
                NAMES,
                BodyChildChainTermination::NullTerminated,
            )
            .unwrap(),
            Vec::<u32>::new()
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "The test uses valid BODY fixtures to verify that owner indices remain unclassified."
    )]
    fn resolves_owner_without_inferring_its_node_type() {
        let nodes = vec![body(1, 0, 2, 0, 0), node(2, "WORLD", vec![])];
        assert_eq!(
            decode_named_body_hierarchy(&nodes, 1, NAMES).unwrap().owner,
            Some(2)
        );
        let nodes = [body(1, 0, 2, 0, 0)];
        let data = decode_named_body_hierarchy(&nodes, 1, NAMES).unwrap();
        assert_eq!(data.owner, Some(2));
        assert_eq!(
            data.unresolved_references(&nodes, NAMES).unwrap(),
            vec![("owner", 2)]
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "Reviewed fixture schemas drive both supported-layout checks and asserted rejection cases."
    )]
    fn exact_topsolid_profiles_gate_body_hierarchy_roles_and_layouts() {
        let supported = [
            "SCH_2701192_26105_13006",
            "SCH_3300259_32001_13006",
            "SCH_3300261_32001_13006",
            "SCH_3700192_36001_13006",
        ];
        let mut covered = Vec::new();
        for schema_key in crate::schema::profiles::topsolid::SUPPORTED_SCHEMA_KEYS {
            let key = SchemaKey::parse(schema_key).unwrap();
            let definition = builtin_body_definition(&key);
            let has_hierarchy_roles = [
                ("child", FieldType::PointerIndex),
                ("owner", FieldType::PointerIndex),
                ("next", FieldType::PointerIndex),
                ("previous", FieldType::PointerIndex),
                ("lowest_node_id", FieldType::Integer),
                ("highest_node_id", FieldType::Integer),
            ]
            .iter()
            .all(|(name, field_type)| {
                definition
                    .fields
                    .iter()
                    .filter(|field| field.name == *name && field.field_type == *field_type)
                    .count()
                    == 1
            });

            if !has_hierarchy_roles {
                let base_only = vec![builtin_node(definition, 1, &[])];
                assert_eq!(
                    BodyHierarchyData::decode_builtin_topsolid(&base_only, schema_key, 1)
                        .unwrap_err()
                        .kind(),
                    ErrorKind::BuiltinProfileUncoveredType,
                    "{schema_key}"
                );
                assert_eq!(
                    BodyHierarchyData::decode_builtin_topsolid_children(
                        &base_only,
                        schema_key,
                        1,
                        BodyChildChainTermination::NullTerminated,
                    )
                    .unwrap_err()
                    .kind(),
                    ErrorKind::BuiltinProfileUncoveredType,
                    "{schema_key} child traversal"
                );
                continue;
            }

            covered.push(schema_key);
            let nodes = vec![
                builtin_body(&key, 1, 2, 3, 0, 0),
                builtin_body(&key, 2, 0, 1, 0, 0),
                node(3, "WORLD", vec![]),
            ];
            let data = BodyHierarchyData::decode_builtin_topsolid(&nodes, schema_key, 1).unwrap();
            assert_eq!(data.child_head, Some(2), "{schema_key}");
            assert_eq!(
                data.owner,
                Some(3),
                "owner stays unclassified for {schema_key}"
            );
            assert_eq!(
                BodyHierarchyData::decode_builtin_topsolid_children(
                    &nodes,
                    schema_key,
                    1,
                    BodyChildChainTermination::NullTerminated,
                )
                .unwrap(),
                vec![2],
                "{schema_key}"
            );
        }

        assert_eq!(covered, supported);

        let key = SchemaKey::parse("SCH_3700192_36001_13006").unwrap();
        let nodes = vec![builtin_body(&key, 18, 0, 0, 29, 0)];
        let data =
            BodyHierarchyData::decode_builtin_topsolid(&nodes, "SCH_3700192_36001_13006", 18)
                .unwrap();
        assert_eq!(data.next, Some(29));
        assert_eq!(
            data.unresolved_references(&nodes, NAMES).unwrap(),
            vec![("next", 29)]
        );
        let child_nodes = vec![builtin_body(&key, 18, 29, 0, 0, 0)];
        assert_eq!(
            BodyHierarchyData::decode_builtin_topsolid_children(
                &child_nodes,
                "SCH_3700192_36001_13006",
                18,
                BodyChildChainTermination::NullTerminated,
            )
            .unwrap(),
            Vec::<u32>::new()
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "A reviewed schema fixture is mutated deliberately to assert raw-layout and key rejection."
    )]
    fn builtin_topsolid_rejects_raw_body_layout_mismatch_and_unreviewed_key() {
        let schema_key = "SCH_3700192_36001_13006";
        let key = SchemaKey::parse(schema_key).unwrap();
        let mut nodes = vec![builtin_body(&key, 1, 0, 3, 0, 0), node(3, "WORLD", vec![])];
        let child_field = std::sync::Arc::make_mut(&mut nodes[0].definition)
            .fields
            .iter_mut()
            .find(|field| field.name == "child")
            .unwrap();
        child_field.name = "unreviewed_child".into();
        assert_eq!(
            BodyHierarchyData::decode_builtin_topsolid(&nodes, schema_key, 1)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidBrepField
        );
        assert_eq!(
            BodyHierarchyData::decode_builtin_topsolid(&nodes, "SCH_1_36001_13006", 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedBuiltinSchemaKey
        );
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
                        || vec![builtin_default(field.field_type)],
                        |(_, values)| values.clone(),
                    )
            })
            .collect::<Vec<_>>();
        let fields = definition
            .fields
            .iter()
            .cloned()
            .zip(values)
            .map(|(definition, values)| RawField {
                definition: definition.into(),
                values,
                byte_range: 0..1,
            })
            .collect();
        RawNode {
            node_type: definition.node_type,
            index,
            variable_length: None,
            definition: definition.into(),
            first_schema: None,
            fields,
            byte_range: 0..1,
            user_fields: Vec::new(),
        }
    }
}
