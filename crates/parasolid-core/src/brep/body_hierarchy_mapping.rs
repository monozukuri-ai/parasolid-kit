//! Opt-in `X_T`/`X_B` B-Rep mapping with source-keyed BODY hierarchy sidecars.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, ErrorDetails, ErrorKind, FieldType, ParseError,
    SchemaKey, SchemaProvider, XbDocument, XtDocument,
};

use super::{
    Body, BodyChildChainTermination, BodyHierarchyData, BodyKind, BrepId, BrepModel, map_xb_brep,
    map_xt_brep,
};

/// Direct hierarchy fields for one mapped BODY, joined by its raw source index.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedBodyHierarchy {
    /// Document-local B-Rep ID assigned to the matching BODY.
    pub brep_body_id: BrepId,
    /// Raw transmit index used to join this record to the BODY.
    pub source_node_index: u32,
    /// Direct serialized BODY hierarchy fields. Pointer values remain transmit indices.
    pub data: BodyHierarchyData,
}

/// An ordinary mapped B-Rep model plus opt-in direct BODY hierarchy records.
#[derive(Debug, Clone, PartialEq)]
pub struct BrepModelWithBodyHierarchy {
    /// The unchanged mapped B-Rep model.
    pub model: BrepModel,
    /// Direct hierarchy records in the model's BODY source order.
    pub body_hierarchies: Vec<MappedBodyHierarchy>,
}

/// Ordered child BODY IDs for one mapped compound BODY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedBodyChildren {
    /// Document-local B-Rep ID assigned to the compound BODY.
    pub parent_brep_body_id: BrepId,
    /// Raw transmit index of the compound BODY.
    pub parent_source_node_index: u32,
    /// Document-local B-Rep IDs in the serialized child-chain order.
    pub child_brep_body_ids: Vec<BrepId>,
}

/// A mapped B-Rep with raw hierarchy links and resolved compound child IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct BrepModelWithResolvedBodyHierarchy {
    /// The unchanged mapped B-Rep model.
    pub model: BrepModel,
    /// Raw hierarchy records, retaining transmit-index pointers.
    pub body_hierarchies: Vec<MappedBodyHierarchy>,
    /// Non-empty compound child chains resolved to document-local B-Rep IDs.
    pub body_children: Vec<MappedBodyChildren>,
}

/// Map an `X_T` B-Rep and retain its direct BODY hierarchy fields as a sidecar.
///
/// This opt-in accepts only the exact reviewed `TopSolid` v6 profiles supported
/// by [`BodyHierarchyData::decode_builtin_topsolid`]. It preserves raw transmit
/// links and does not choose child-chain termination or flatten child bodies
/// into their parents. Ordinary [`map_xt_brep`] behavior is unchanged.
///
/// # Errors
/// Returns the ordinary `X_T` B-Rep mapping errors, or the exact-profile,
/// uncovered-layout, duplicate-index, or reference errors from the hierarchy
/// decoder and source-index join.
pub fn map_xt_brep_with_body_hierarchy(
    document: &XtDocument,
) -> Result<BrepModelWithBodyHierarchy, ParseError> {
    validate_topsolid_hierarchy_profile(document.schema_key.raw())?;
    let model = map_xt_brep(document)?;
    map_brep_with_body_hierarchy(document.schema_key.raw(), &document.nodes, model)
}

/// Map an `X_B` B-Rep and retain its direct BODY hierarchy fields as a sidecar.
///
/// This accepts only the exact reviewed `TopSolid` v6 profiles with the complete
/// effective BODY hierarchy layout. Raw transmit links remain in the sidecar.
///
/// # Errors
/// Returns ordinary `X_B` mapping errors, or errors from exact-profile validation,
/// hierarchy decoding, duplicate-index checks, or the source-index join.
pub fn map_xb_brep_with_body_hierarchy(
    document: &XbDocument,
) -> Result<BrepModelWithBodyHierarchy, ParseError> {
    validate_topsolid_hierarchy_profile(document.schema_key.raw())?;
    let model = map_xb_brep(document)?;
    map_brep_with_body_hierarchy(document.schema_key.raw(), &document.nodes, model)
}

fn map_brep_with_body_hierarchy(
    schema_key: &str,
    nodes: &[crate::RawNode],
    model: BrepModel,
) -> Result<BrepModelWithBodyHierarchy, ParseError> {
    let data = model
        .bodies
        .iter()
        .map(|body| {
            BodyHierarchyData::decode_builtin_topsolid(nodes, schema_key, body.source.node_index)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let body_hierarchies = join_body_hierarchies(&model.bodies, data)?;
    Ok(BrepModelWithBodyHierarchy {
        model,
        body_hierarchies,
    })
}

/// Map an `X_T` B-Rep and resolve compound child chains to mapped BODY IDs.
///
/// The caller selects the sibling-chain termination convention. Raw hierarchy
/// records are retained alongside resolved IDs, and ordinary
/// [`map_xt_brep`] behavior is unchanged.
///
/// # Errors
/// Returns errors from B-Rep mapping, hierarchy decoding, or child-link resolution.
pub fn map_xt_brep_with_resolved_body_hierarchy(
    document: &XtDocument,
    termination: BodyChildChainTermination,
) -> Result<BrepModelWithResolvedBodyHierarchy, ParseError> {
    let mapped = map_xt_brep_with_body_hierarchy(document)?;
    resolve_body_hierarchy(
        document.schema_key.raw(),
        &document.nodes,
        mapped,
        termination,
    )
}

/// Map an `X_B` B-Rep and resolve compound child chains to mapped BODY IDs.
///
/// The caller selects the sibling-chain termination convention. Raw transmit
/// links are retained alongside resolved IDs; ordinary [`map_xb_brep`]
/// behavior is unchanged.
///
/// # Errors
/// Returns errors from B-Rep mapping, hierarchy decoding, or child-link resolution.
pub fn map_xb_brep_with_resolved_body_hierarchy(
    document: &XbDocument,
    termination: BodyChildChainTermination,
) -> Result<BrepModelWithResolvedBodyHierarchy, ParseError> {
    let mapped = map_xb_brep_with_body_hierarchy(document)?;
    resolve_body_hierarchy(
        document.schema_key.raw(),
        &document.nodes,
        mapped,
        termination,
    )
}

fn resolve_body_hierarchy(
    schema_key: &str,
    nodes: &[crate::RawNode],
    mapped: BrepModelWithBodyHierarchy,
    termination: BodyChildChainTermination,
) -> Result<BrepModelWithResolvedBodyHierarchy, ParseError> {
    let mut child_sources = Vec::new();
    for hierarchy in &mapped.body_hierarchies {
        if hierarchy.data.effective_child_head(nodes)?.is_none() {
            continue;
        }
        let children = BodyHierarchyData::decode_builtin_topsolid_children(
            nodes,
            schema_key,
            hierarchy.source_node_index,
            termination,
        )?;
        child_sources.push((hierarchy.source_node_index, children));
    }
    let resolved_child_parents = child_sources.iter().map(|(parent, _)| *parent).collect();
    let body_children = join_body_children(
        &mapped.model.bodies,
        &mapped.body_hierarchies,
        child_sources,
        &resolved_child_parents,
    )?;
    Ok(BrepModelWithResolvedBodyHierarchy {
        model: mapped.model,
        body_hierarchies: mapped.body_hierarchies,
        body_children,
    })
}

fn validate_topsolid_hierarchy_profile(schema_key: &str) -> Result<(), ParseError> {
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

    let find_body = |schema: &str| {
        (0..=u16::MAX).find_map(|node_type| {
            provider
                .type_definition(schema, node_type)
                .filter(|definition| definition.name == "BODY")
        })
    };
    let (schema, body) = find_body(key.effective())
        .map(|body| (key.effective(), body))
        .or_else(|| {
            key.base()
                .and_then(|base| find_body(base).map(|body| (base, body)))
        })
        .ok_or_else(|| uncovered_body_hierarchy_profile(key.effective(), 0))?;
    for (name, field_type) in [
        ("child", FieldType::PointerIndex),
        ("owner", FieldType::PointerIndex),
        ("next", FieldType::PointerIndex),
        ("previous", FieldType::PointerIndex),
        ("lowest_node_id", FieldType::Integer),
        ("highest_node_id", FieldType::Integer),
    ] {
        let mut fields = body.fields.iter().filter(|field| field.name == name);
        let Some(field) = fields.next() else {
            return Err(uncovered_body_hierarchy_profile(schema, body.node_type));
        };
        if fields.next().is_some() || field.field_type != field_type {
            return Err(uncovered_body_hierarchy_profile(schema, body.node_type));
        }
    }
    Ok(())
}

fn unsupported_builtin_key(schema_key: &str) -> ParseError {
    ParseError::new(
        ErrorKind::UnsupportedBuiltinSchemaKey,
        0,
        "BODY hierarchy mapping requires an exact compiled TopSolid v6 schema key",
        ErrorDetails::InvalidText {
            field: "schema_key",
            value: schema_key.to_owned(),
        },
    )
}

fn uncovered_body_hierarchy_profile(schema: &str, node_type: u16) -> ParseError {
    ParseError::new(
        ErrorKind::BuiltinProfileUncoveredType,
        0,
        "TopSolid profile does not provide the complete BODY hierarchy layout",
        ErrorDetails::SchemaLookup {
            schema: schema.to_owned(),
            node_type,
        },
    )
}

fn join_body_hierarchies(
    bodies: &[Body],
    data: Vec<BodyHierarchyData>,
) -> Result<Vec<MappedBodyHierarchy>, ParseError> {
    let mut body_ids = BTreeMap::new();
    for body in bodies {
        if body_ids.insert(body.source.node_index, body.id).is_some() {
            return Err(ParseError::new(
                ErrorKind::DuplicateNodeIndex,
                body.source.byte_range.start,
                format!("BODY source index {} is ambiguous", body.source.node_index),
                ErrorDetails::NodeIndex {
                    node_index: body.source.node_index,
                },
            ));
        }
    }

    let mut seen = BTreeSet::new();
    let mut records = Vec::with_capacity(data.len());
    for hierarchy in data {
        let source_node_index = hierarchy.source.node_index;
        if !seen.insert(source_node_index) {
            return Err(ParseError::new(
                ErrorKind::DuplicateNodeIndex,
                hierarchy.source.byte_range.start,
                format!("BODY hierarchy source index {source_node_index} is ambiguous"),
                ErrorDetails::NodeIndex {
                    node_index: source_node_index,
                },
            ));
        }
        let brep_body_id = body_ids.get(&source_node_index).copied().ok_or_else(|| {
            ParseError::new(
                ErrorKind::InvalidBrepReference,
                hierarchy.source.byte_range.start,
                "BODY hierarchy source index does not match a mapped BODY",
                ErrorDetails::BrepReference {
                    node_index: source_node_index,
                    field: "body_hierarchy_source",
                    target_index: source_node_index,
                    expected_type: "mapped BODY source index",
                },
            )
        })?;
        records.push(MappedBodyHierarchy {
            brep_body_id,
            source_node_index,
            data: hierarchy,
        });
    }

    if let Some(missing) = body_ids
        .keys()
        .find(|source_index| !seen.contains(source_index))
        .copied()
    {
        return Err(ParseError::new(
            ErrorKind::InvalidBrepReference,
            0,
            "mapped BODY is missing its hierarchy sidecar",
            ErrorDetails::BrepReference {
                node_index: missing,
                field: "body_hierarchy_source",
                target_index: missing,
                expected_type: "BODY hierarchy source index",
            },
        ));
    }
    Ok(records)
}

fn join_body_children(
    bodies: &[Body],
    hierarchies: &[MappedBodyHierarchy],
    child_sources: Vec<(u32, Vec<u32>)>,
    resolved_child_parents: &BTreeSet<u32>,
) -> Result<Vec<MappedBodyChildren>, ParseError> {
    let body_ids = bodies
        .iter()
        .map(|body| (body.source.node_index, body.id))
        .collect::<BTreeMap<_, _>>();
    let body_kinds = bodies
        .iter()
        .map(|body| (body.id, body.kind))
        .collect::<BTreeMap<_, _>>();
    let mut children_by_parent = BTreeMap::new();
    for (parent_source, children) in child_sources {
        if children_by_parent.insert(parent_source, children).is_some() {
            return Err(invalid_body_link(
                parent_source,
                parent_source,
                "compound BODY",
            ));
        }
    }

    let mut resolved = Vec::new();
    for hierarchy in hierarchies
        .iter()
        .filter(|hierarchy| resolved_child_parents.contains(&hierarchy.source_node_index))
    {
        let parent_source = hierarchy.source_node_index;
        let children = children_by_parent.remove(&parent_source).ok_or_else(|| {
            invalid_body_link(parent_source, parent_source, "decoded compound child chain")
        })?;
        let child_brep_body_ids = children
            .into_iter()
            .map(|child_source| {
                body_ids.get(&child_source).copied().ok_or_else(|| {
                    invalid_body_link(parent_source, child_source, "mapped child BODY")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(first_child) = child_brep_body_ids.first() {
            let parent_kind = body_kinds[&hierarchy.brep_body_id];
            let first_kind = body_kinds[first_child];
            let same_kind = child_brep_body_ids
                .iter()
                .all(|child| body_kinds[child] == first_kind);
            if (same_kind && parent_kind != first_kind)
                || (!same_kind && parent_kind != BodyKind::General)
            {
                return Err(ParseError::new(
                    ErrorKind::InvalidBrepReference,
                    0,
                    format!("compound BODY {parent_source} body_type conflicts with its children"),
                    ErrorDetails::BrepReference {
                        node_index: parent_source,
                        field: "body_type",
                        target_index: *first_child,
                        expected_type: "compound BODY body_type consistent with child BODY types",
                    },
                ));
            }
        }
        resolved.push(MappedBodyChildren {
            parent_brep_body_id: hierarchy.brep_body_id,
            parent_source_node_index: parent_source,
            child_brep_body_ids,
        });
    }
    if let Some((parent_source, _)) = children_by_parent.into_iter().next() {
        return Err(invalid_body_link(
            parent_source,
            parent_source,
            "mapped compound BODY",
        ));
    }
    ensure_acyclic_body_children(bodies, &resolved)?;
    Ok(resolved)
}

fn ensure_acyclic_body_children(
    bodies: &[Body],
    children: &[MappedBodyChildren],
) -> Result<(), ParseError> {
    let sources = bodies
        .iter()
        .map(|body| (body.id, body.source.node_index))
        .collect::<BTreeMap<_, _>>();
    let mut indegree = bodies
        .iter()
        .map(|body| (body.id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = BTreeMap::<BrepId, &[BrepId]>::new();
    for link in children {
        adjacency.insert(link.parent_brep_body_id, &link.child_brep_body_ids);
        for child in &link.child_brep_body_ids {
            let degree = indegree.get_mut(child).ok_or_else(|| {
                invalid_body_link(
                    link.parent_source_node_index,
                    *child,
                    "mapped child BODY ID",
                )
            })?;
            *degree += 1;
        }
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect::<VecDeque<_>>();
    let mut visited = 0;
    while let Some(parent) = ready.pop_front() {
        visited += 1;
        if let Some(child_ids) = adjacency.get(&parent) {
            for child in *child_ids {
                let degree = indegree.get_mut(child).ok_or_else(|| {
                    invalid_body_link(
                        sources.get(&parent).copied().unwrap_or_default(),
                        *child,
                        "mapped child BODY ID",
                    )
                })?;
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(*child);
                }
            }
        }
    }
    if visited != bodies.len() {
        let source_index = indegree
            .iter()
            .find_map(|(id, degree)| (*degree > 0).then(|| sources.get(id).copied()).flatten())
            .unwrap_or(0);
        return Err(ParseError::new(
            ErrorKind::InvalidBrepReference,
            0,
            format!("compound BODY hierarchy contains a cycle at BODY {source_index}"),
            ErrorDetails::BrepReference {
                node_index: source_index,
                field: "child",
                target_index: source_index,
                expected_type: "acyclic BODY hierarchy",
            },
        ));
    }
    Ok(())
}

fn invalid_body_link(
    parent_source: u32,
    target_source: u32,
    expected_type: &'static str,
) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBrepReference,
        0,
        format!("BODY hierarchy link {target_source} does not resolve to {expected_type}"),
        ErrorDetails::BrepReference {
            node_index: parent_source,
            field: "child",
            target_index: target_source,
            expected_type,
        },
    )
}

#[cfg(test)]
#[allow(
    clippy::match_same_arms,
    reason = "Fixture indices and values are bounded and kept explicit for regression checks."
)]
#[allow(
    clippy::panic,
    clippy::too_many_lines,
    reason = "Tests use explicit fixture failures and exact expected values."
)]
mod tests {
    use super::*;
    use crate::brep::{BodyKind, BrepMetrics, BrepSourceFormat, SourceNodeRef, TopologyValidation};
    use crate::{DocumentLimits, FieldType, FieldValue, SchemaProvider, parse_xt};
    use std::fmt::Write as _;

    const SYNTHETIC_SCHEMA_KEY: &str = "SCH_3300259_32001_13006";

    fn encoded_body_node(
        schema_key: &str,
        index: u32,
        first_of_type: bool,
        values: &[(&str, &str)],
    ) -> String {
        let definition = effective_body_definition(schema_key);
        let mut encoded = if first_of_type {
            format!("12 {}{index} ", encoded_body_delta(schema_key))
        } else {
            format!("12 {index} ")
        };
        for field in &definition.fields {
            if !field.transmitted {
                continue;
            }
            let value = values
                .iter()
                .find_map(|(name, value)| (*name == field.name.as_str()).then_some(*value))
                .unwrap_or_else(|| match field.field_type {
                    FieldType::UnsignedByte
                    | FieldType::ShortInteger
                    | FieldType::Integer
                    | FieldType::PointerIndex => "0",
                    FieldType::Character => "+",
                    FieldType::Logical => "F",
                    FieldType::UnicodeCharacter => "0",
                    FieldType::Tag
                    | FieldType::Double
                    | FieldType::Interval
                    | FieldType::Vector
                    | FieldType::Box3
                    | FieldType::IntersectionPoint => "?",
                    FieldType::OpaquePointer => panic!("BODY has unsupported opaque pointer"),
                });
            encoded.push_str(value);
            if field.field_type != FieldType::Character
                && field.field_type != FieldType::Logical
                && value != "?"
            {
                encoded.push(' ');
            }
        }
        encoded
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "Fixture construction targets a reviewed compiled schema whose BODY definition must exist."
    )]
    fn effective_body_definition(schema_key: &str) -> crate::TypeDefinition {
        let key = SchemaKey::parse(schema_key).unwrap();
        BuiltinProfileRegistry::compiled()
            .unwrap()
            .provider_for_key(&key)
            .unwrap()
            .type_definition(key.effective(), 12)
            .filter(|definition| definition.name == "BODY")
            .expect("exact compiled TopSolid effective schema declares BODY")
            .clone()
    }

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "Fixture delta encoding requires the reviewed compiled base and effective BODY definitions."
    )]
    fn encoded_body_delta(schema_key: &str) -> String {
        let key = SchemaKey::parse(schema_key).unwrap();
        let provider = BuiltinProfileRegistry::compiled()
            .unwrap()
            .provider_for_key(&key)
            .unwrap();
        let base = provider
            .type_definition(key.provider_schema(), 12)
            .filter(|definition| definition.name == "BODY")
            .expect("compiled TopSolid base schema declares BODY");
        let effective = provider
            .type_definition(key.effective(), 12)
            .filter(|definition| definition.name == "BODY")
            .expect("compiled TopSolid effective schema declares BODY");
        let mut encoded = format!("{} ", effective.fields.len());
        let mut base_index = 0;
        let mut appending = false;

        for target in &effective.fields {
            if base.fields.get(base_index) == Some(target) {
                encoded.push('C');
                base_index += 1;
            } else if base
                .fields
                .get(base_index)
                .is_some_and(|field| field.name == target.name)
            {
                encoded.push('D');
                base_index += 1;
                if appending || base_index == base.fields.len() {
                    appending = true;
                    encoded.push('A');
                } else {
                    encoded.push('I');
                }
                encoded_delta_field(&mut encoded, target);
            } else if let Some(offset) = base.fields[base_index..]
                .iter()
                .position(|field| field == target)
            {
                for _ in 0..offset {
                    encoded.push('D');
                    base_index += 1;
                }
                encoded.push('C');
                base_index += 1;
            } else if base_index == base.fields.len() {
                appending = true;
                encoded.push('A');
                encoded_delta_field(&mut encoded, target);
            } else {
                encoded.push('I');
                encoded_delta_field(&mut encoded, target);
            }
        }
        while base_index < base.fields.len() {
            encoded.push('D');
            base_index += 1;
        }
        encoded.push('Z');
        encoded
    }

    fn encoded_delta_field(encoded: &mut String, field: &crate::FieldDefinition) {
        let _ = write!(
            encoded,
            "{} {}{} {}",
            field.name.len(),
            field.name,
            field.pointer_class,
            field.element_count
        );
        if field.pointer_class == 0 {
            let code = field.field_type.code();
            let _ = write!(encoded, " {} {}", code.len(), code);
        }
        if field.element_count == 1 {
            if field.pointer_class != 0 {
                encoded.push(' ');
            }
            encoded.push_str(if field.transmitted { "T" } else { "F" });
        } else if field.pointer_class != 0 {
            encoded.push(' ');
        }
    }

    fn body(id: BrepId, source_node_index: u32) -> Body {
        Body {
            id,
            kind: BodyKind::Solid,
            size_resolution: 0.0,
            linear_resolution: 0.0,
            regions: vec![],
            edges: vec![],
            vertices: vec![],
            attributes: vec![],
            source: source(source_node_index),
        }
    }

    fn source(node_index: u32) -> SourceNodeRef {
        SourceNodeRef {
            node_index,
            node_type: 12,
            type_name: "BODY".into(),
            node_id: None,
            byte_range: std::ops::Range {
                start: node_index as usize,
                end: node_index as usize + 1,
            },
        }
    }

    fn hierarchy(node_index: u32) -> BodyHierarchyData {
        BodyHierarchyData {
            child_head: None,
            owner: Some(900),
            next: None,
            previous: None,
            lowest_id: Some(1),
            highest_id: Some(8),
            source: source(node_index),
        }
    }

    fn empty_model(bodies: Vec<Body>) -> BrepModel {
        BrepModel {
            source_format: BrepSourceFormat::Text,
            schema_key: "SCH_3700192_36001_13006".into(),
            complete: false,
            bodies,
            regions: vec![],
            shells: vec![],
            faces: vec![],
            loops: vec![],
            half_edges: vec![],
            edges: vec![],
            vertices: vec![],
            points: vec![],
            curves: vec![],
            surfaces: vec![],
            topology: TopologyValidation {
                valid: false,
                closed_loop_count: 0,
                closed_edge_ring_count: 0,
                euler_characteristic: 0,
            },
            metrics: BrepMetrics {
                bounding_box: None,
                surface_area: None,
                volume: None,
            },
            assemblies: vec![],
            instances: vec![],
            attributes: vec![],
            diagnostics: vec![],
        }
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "This success fixture asserts that the sidecar joins by the supplied source indices."
    )]
    fn joins_sidecar_by_source_index_without_reordering_or_reinterpreting_links() {
        let bodies = vec![body(0, 50), body(1, 10)];
        let records = join_body_hierarchies(&bodies, vec![hierarchy(10), hierarchy(50)])
            .expect("source-index join");
        assert_eq!(records[0].source_node_index, 10);
        assert_eq!(records[0].brep_body_id, 1);
        assert_eq!(records[1].source_node_index, 50);
        assert_eq!(records[1].brep_body_id, 0);
        assert_eq!(records[0].data.owner, Some(900));
        assert_eq!(records[0].data.child_head, None);
    }

    #[test]
    fn rejects_missing_duplicate_and_unmatched_sidecar_source_keys() {
        let bodies = vec![body(0, 10), body(1, 20)];
        assert!(join_body_hierarchies(&bodies, vec![hierarchy(10)]).is_err());
        assert!(join_body_hierarchies(&bodies, vec![hierarchy(10), hierarchy(10)]).is_err());
        assert!(join_body_hierarchies(&bodies, vec![hierarchy(10), hierarchy(30)]).is_err());
        assert!(join_body_hierarchies(&[body(0, 10), body(1, 10)], vec![]).is_err());
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "This valid compound fixture must resolve all supplied child indices before checking order."
    )]
    fn resolves_compound_child_sources_to_ordered_body_ids() {
        let bodies = vec![body(8, 100), body(3, 20), body(5, 30)];
        let mut parent = hierarchy(100);
        parent.child_head = Some(20);
        let hierarchies =
            join_body_hierarchies(&bodies, vec![parent, hierarchy(20), hierarchy(30)]).unwrap();

        let resolved_parents = BTreeSet::from([100]);
        let children = join_body_children(
            &bodies,
            &hierarchies,
            vec![(100, vec![30, 20])],
            &resolved_parents,
        )
        .expect("all child transmit indexes resolve");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].parent_brep_body_id, 8);
        assert_eq!(children[0].parent_source_node_index, 100);
        assert_eq!(children[0].child_brep_body_ids, vec![5, 3]);
        assert_eq!(hierarchies[0].data.child_head, Some(20));
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "The fixture hierarchy is valid; unwrap_err calls assert the intentionally rejected body-kind cases."
    )]
    fn enforces_compound_body_kind_from_resolved_child_kinds() {
        let mut parent = hierarchy(100);
        parent.child_head = Some(20);
        let hierarchies = join_body_hierarchies(
            &[body(0, 100), body(1, 20), body(2, 30)],
            vec![parent, hierarchy(20), hierarchy(30)],
        )
        .unwrap();
        let resolved_parents = BTreeSet::from([100]);

        let mut empty_chain = vec![body(0, 100), body(1, 20), body(2, 30)];
        empty_chain[0].kind = BodyKind::Wire;
        assert!(
            join_body_children(
                &empty_chain,
                &hierarchies,
                vec![(100, vec![])],
                &resolved_parents,
            )
            .is_ok()
        );

        let mut homogeneous = vec![body(0, 100), body(1, 20), body(2, 30)];
        homogeneous[0].kind = BodyKind::Wire;
        assert_eq!(
            join_body_children(
                &homogeneous,
                &hierarchies,
                vec![(100, vec![20, 30])],
                &resolved_parents,
            )
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidBrepReference
        );
        homogeneous[1].kind = BodyKind::Solid;
        homogeneous[2].kind = BodyKind::Solid;
        homogeneous[0].kind = BodyKind::Solid;
        assert!(
            join_body_children(
                &homogeneous,
                &hierarchies,
                vec![(100, vec![20, 30])],
                &resolved_parents,
            )
            .is_ok()
        );

        let mut mixed = vec![body(0, 100), body(1, 20), body(2, 30)];
        mixed[0].kind = BodyKind::Solid;
        mixed[2].kind = BodyKind::Sheet;
        assert_eq!(
            join_body_children(
                &mixed,
                &hierarchies,
                vec![(100, vec![20, 30])],
                &resolved_parents,
            )
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidBrepReference
        );
        mixed[0].kind = BodyKind::General;
        assert!(
            join_body_children(
                &mixed,
                &hierarchies,
                vec![(100, vec![20, 30])],
                &resolved_parents,
            )
            .is_ok()
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "The valid sidecar fixture is prepared before assertions verify malformed child-chain errors."
    )]
    fn rejects_missing_compound_chain_and_unmapped_child_body() {
        let bodies = vec![body(8, 100), body(3, 20)];
        let mut parent = hierarchy(100);
        parent.child_head = Some(20);
        let mut child = hierarchy(20);
        child.owner = Some(100);
        let hierarchies = join_body_hierarchies(&bodies, vec![parent, child]).unwrap();

        let resolved_parents = BTreeSet::from([100]);
        assert_eq!(
            join_body_children(&bodies, &hierarchies, vec![], &resolved_parents)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidBrepReference
        );
        assert_eq!(
            join_body_children(
                &bodies,
                &hierarchies,
                vec![(100, vec![99])],
                &resolved_parents,
            )
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidBrepReference
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "The test prepares valid cyclic and acyclic hierarchies, then asserts their respective outcomes."
    )]
    fn rejects_multi_body_ancestor_cycles_and_accepts_acyclic_nesting() {
        let cyclic_bodies = vec![body(4, 100), body(7, 20)];
        let mut first = hierarchy(100);
        first.child_head = Some(20);
        let mut second = hierarchy(20);
        second.child_head = Some(100);
        let cyclic_hierarchies =
            join_body_hierarchies(&cyclic_bodies, vec![first, second]).unwrap();
        let resolved_parents = BTreeSet::from([100, 20]);
        assert_eq!(
            join_body_children(
                &cyclic_bodies,
                &cyclic_hierarchies,
                vec![(100, vec![20]), (20, vec![100])],
                &resolved_parents,
            )
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidBrepReference
        );

        let nested_bodies = vec![body(4, 100), body(7, 20), body(9, 30)];
        let mut first = hierarchy(100);
        first.child_head = Some(20);
        let mut second = hierarchy(20);
        second.child_head = Some(30);
        let nested_hierarchies =
            join_body_hierarchies(&nested_bodies, vec![first, second, hierarchy(30)]).unwrap();
        let resolved_parents = BTreeSet::from([100, 20]);
        let resolved = join_body_children(
            &nested_bodies,
            &nested_hierarchies,
            vec![(100, vec![20]), (20, vec![30])],
            &resolved_parents,
        )
        .expect("acyclic nesting is supported");
        assert_eq!(resolved[0].child_brep_body_ids, vec![7]);
        assert_eq!(resolved[1].child_brep_body_ids, vec![9]);
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "This test asserts supported and unsupported exact schema keys through their expected outcomes."
    )]
    fn validates_exact_hierarchy_provider_before_mapping_any_bodies() {
        for schema_key in [
            "SCH_2701192_26105_13006",
            "SCH_3300259_32001_13006",
            "SCH_3300261_32001_13006",
            "SCH_3700192_36001_13006",
        ] {
            validate_topsolid_hierarchy_profile(schema_key).unwrap();
        }
        assert_eq!(
            validate_topsolid_hierarchy_profile("SCH_3600188_36001_13006")
                .unwrap_err()
                .kind(),
            ErrorKind::BuiltinProfileUncoveredType
        );
        assert_eq!(
            validate_topsolid_hierarchy_profile("SCH_1_36001_13006")
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedBuiltinSchemaKey
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "The wrapper fixture requires a valid sidecar join before asserting model separation."
    )]
    fn wrapper_keeps_the_original_model_as_a_separate_value() {
        let model = empty_model(vec![body(7, 91)]);
        let wrapper = BrepModelWithBodyHierarchy {
            model,
            body_hierarchies: join_body_hierarchies(&[body(7, 91)], vec![hierarchy(91)]).unwrap(),
        };
        assert_eq!(wrapper.model.bodies[0].id, 7);
        assert_eq!(wrapper.body_hierarchies[0].brep_body_id, 7);
        assert_eq!(wrapper.body_hierarchies[0].source_node_index, 91);
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "This encoded fixture uses a reviewed schema and valid BODY records before checking mapped hierarchy data."
    )]
    fn maps_parsed_topsolid_compound_body_and_null_terminated_children() {
        let modeller = ": TRANSMIT FILE created by modeller version 3300259";
        let mut encoded = format!(
            "T{} {}{} {}300 0 ",
            modeller.len(),
            modeller,
            SYNTHETIC_SCHEMA_KEY.len(),
            SYNTHETIC_SCHEMA_KEY
        );
        // General bodies have no manifold topology minimum in the mapper.
        encoded.push_str(&encoded_body_node(
            SYNTHETIC_SCHEMA_KEY,
            10,
            true,
            &[
                ("body_type", "6"),
                ("res_size", "0"),
                ("res_linear", "0"),
                ("child", "20"),
                ("owner", "0"),
            ],
        ));
        encoded.push_str(&encoded_body_node(
            SYNTHETIC_SCHEMA_KEY,
            20,
            false,
            &[
                ("body_type", "6"),
                ("res_size", "0"),
                ("res_linear", "0"),
                ("child", "0"),
                ("owner", "10"),
                ("next", "30"),
                ("previous", "0"),
            ],
        ));
        encoded.push_str(&encoded_body_node(
            SYNTHETIC_SCHEMA_KEY,
            30,
            false,
            &[
                ("body_type", "6"),
                ("res_size", "0"),
                ("res_linear", "0"),
                ("child", "0"),
                ("owner", "10"),
                ("next", "0"),
                ("previous", "20"),
            ],
        ));
        encoded.push_str("1 0 ");

        let key = SchemaKey::parse(SYNTHETIC_SCHEMA_KEY).unwrap();
        let provider = BuiltinProfileRegistry::compiled()
            .unwrap()
            .provider_for_key(&key)
            .unwrap();
        let document = parse_xt(encoded.as_bytes(), &provider, DocumentLimits::default()).unwrap();
        let mapped = map_xt_brep_with_resolved_body_hierarchy(
            &document,
            BodyChildChainTermination::NullTerminated,
        )
        .unwrap();

        assert_eq!(
            mapped
                .model
                .bodies
                .iter()
                .map(|body| (body.id, body.source.node_index))
                .collect::<Vec<_>>(),
            vec![(0, 10), (1, 20), (2, 30)]
        );
        assert!(
            mapped
                .model
                .bodies
                .iter()
                .all(|body| body.kind == BodyKind::General)
        );
        assert_eq!(mapped.body_hierarchies[0].data.child_head, Some(20));
        assert_eq!(mapped.body_hierarchies[0].data.owner, None);
        assert_eq!(mapped.body_hierarchies[0].data.next, None);
        assert_eq!(mapped.body_hierarchies[0].data.previous, None);
        assert_eq!(mapped.body_hierarchies[0].data.lowest_id, Some(0));
        assert_eq!(mapped.body_hierarchies[0].data.highest_id, Some(0));
        assert_eq!(mapped.body_hierarchies[1].data.owner, Some(10));
        assert_eq!(mapped.body_hierarchies[1].data.child_head, None);
        assert_eq!(mapped.body_hierarchies[1].data.next, Some(30));
        assert_eq!(mapped.body_hierarchies[1].data.previous, None);
        assert_eq!(mapped.body_hierarchies[2].data.owner, Some(10));
        assert_eq!(mapped.body_hierarchies[2].data.child_head, None);
        assert_eq!(mapped.body_hierarchies[2].data.previous, Some(20));
        assert_eq!(mapped.body_hierarchies[2].data.next, None);
        assert_eq!(mapped.body_children.len(), 1);
        assert_eq!(mapped.body_children[0].parent_brep_body_id, 0);
        assert_eq!(mapped.body_children[0].parent_source_node_index, 10);
        assert_eq!(mapped.body_children[0].child_brep_body_ids, vec![1, 2]);

        let mut unresolved_child = document;
        let parent = unresolved_child
            .nodes
            .iter_mut()
            .find(|node| node.index == 10)
            .unwrap();
        let child_field = parent
            .fields
            .iter_mut()
            .find(|field| field.definition.name == "child")
            .unwrap();
        child_field.values[0] = FieldValue::PointerIndex(99);
        let mapped = map_xt_brep_with_resolved_body_hierarchy(
            &unresolved_child,
            BodyChildChainTermination::NullTerminated,
        )
        .unwrap();
        assert_eq!(mapped.body_hierarchies[0].data.child_head, Some(99));
        let hierarchy_fields = super::super::BodyHierarchyFieldNames {
            child_head: "child",
            owner: "owner",
            next: "next",
            previous: "previous",
            lowest_id: "lowest_node_id",
            highest_id: "highest_node_id",
        };
        assert_eq!(
            mapped.body_hierarchies[0]
                .data
                .unresolved_references(&unresolved_child.nodes, hierarchy_fields)
                .unwrap(),
            vec![("child", 99)]
        );
        assert!(mapped.body_children.is_empty());
    }
}
