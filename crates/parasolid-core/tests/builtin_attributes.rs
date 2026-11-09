#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::brep::{
    AttributeField, AttributeFieldData, AttributeOwner, BrepModel, map_xb_brep,
    map_xb_brep_attributes, map_xb_brep_with_metadata_recovery, map_xt_brep,
    map_xt_brep_attributes, map_xt_brep_with_metadata_recovery,
};
use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, ErrorKind, FieldType, FieldValue, RawField, RawNode,
    SchemaKey, parse_xb, parse_xt,
    schema::profiles::{legacy_standard_keys, onshape_sch13006, onshape_sch30000},
};

const KEY: &str = "SCH_3000000_30000";
const LEGACY_KEY: &str = "SCH_901297_9008";
const VALID_BODY_WITH_ATTRIBUTE: &str = concat!(
    "12 1 12 0 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 3 4 5 0 6 7 0 ",
    "70 2 0 1 0 0 4 1 20 8 8 8 1 T",
    "13 3 3 0 1 0 9 0 0 6 9 ",
    "50 4 11 0 9 0 0 0 +0 0 0 0 0 1 1 0 0 ",
    "31 5 10 0 7 0 0 0 +0 0 0 0 0 1 1 0 0 1 ",
    "19 6 5 0 1 0 0 3 V",
    "16 7 6 0 ?10 0 0 5 0 0 1 ",
    "17 10 11 10 10 0 12 7 0 0 +",
    "15 11 7 0 10 9 0 ",
    "17 12 0 0 0 0 10 7 0 0 -",
    "14 9 2 13 ?0 0 11 3 4 +0 0 0 0 3 ",
    "81 1 13 12 14 9 0 0 0 0 15 ",
    "80 1 14 0 16 8001 0 0 0 0 3 5 0 0 FFFFTFTFFFFFF1 ",
    "82 3 15 1 2 3 ",
    "79 15 16 SDL/TYSA_COLOUR",
    "74 20 8 1 0 13 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 ",
    "1 0 ",
);

#[allow(
    clippy::expect_used,
    reason = "Fixture invariants must hold for the test to be meaningful."
)]
fn node(
    profile: &parasolid_core::BuiltinSchemaProfile,
    kind: u16,
    index: u32,
    values: &[(usize, Vec<FieldValue>)],
) -> RawNode {
    let definition = profile
        .definitions()
        .find(|definition| definition.node_type == kind)
        .expect("known profile type")
        .clone();
    let fields = definition
        .fields
        .iter()
        .enumerate()
        .map(|(ordinal, definition)| {
            let values = values
                .iter()
                .find(|(field, _)| *field == ordinal)
                .map_or_else(
                    || {
                        let count = definition.element_count.max(1) as usize;
                        vec![default_value(definition.field_type); count]
                    },
                    |(_, values)| values.clone(),
                );
            RawField {
                definition: definition.clone().into(),
                values,
                byte_range: 0..0,
            }
        })
        .collect();
    let variable_length = definition.variable.then(|| {
        values
            .first()
            .map_or(0, |(_, values)| u32::try_from(values.len()).unwrap_or(0))
    });
    RawNode {
        node_type: kind,
        index,
        variable_length,
        definition: definition.into(),
        first_schema: None,
        fields,
        byte_range: 0..0,
        user_fields: Vec::new(),
    }
}

fn default_value(kind: FieldType) -> FieldValue {
    match kind {
        FieldType::UnsignedByte => FieldValue::UnsignedByte(0),
        FieldType::Character | FieldType::OpaquePointer => FieldValue::Character(0),
        FieldType::Logical => FieldValue::Logical(false),
        FieldType::ShortInteger => FieldValue::ShortInteger(None),
        FieldType::UnicodeCharacter => FieldValue::UnicodeCharacter(0),
        FieldType::Integer => FieldValue::Integer(None),
        FieldType::PointerIndex => FieldValue::PointerIndex(0),
        FieldType::Tag => FieldValue::Tag(None),
        FieldType::Double => FieldValue::Double(None),
        FieldType::Interval => FieldValue::Interval([None; 2]),
        FieldType::Vector => FieldValue::Vector([None; 3]),
        FieldType::Box3 => FieldValue::Box3([None; 6]),
        FieldType::IntersectionPoint => FieldValue::IntersectionPoint([None; 3]),
    }
}

fn encoded_nodes(profile: &parasolid_core::BuiltinSchemaProfile) -> Vec<RawNode> {
    use FieldValue::{Character as C, Integer as I, PointerIndex as P, UnsignedByte as U};
    vec![
        node(
            profile,
            79,
            1,
            &[(0, b"TEST/ID".iter().copied().map(C).collect())],
        ),
        node(
            profile,
            80,
            2,
            &[(1, vec![P(1)]), (2, vec![I(Some(8001))]), (6, vec![U(1)])],
        ),
        node(
            profile,
            81,
            3,
            &[
                (0, vec![I(Some(4))]),
                (1, vec![P(2)]),
                (2, vec![P(1)]),
                (7, vec![P(4)]),
            ],
        ),
        node(profile, 82, 4, &[(0, vec![I(Some(42))])]),
    ]
}

#[allow(
    clippy::expect_used,
    reason = "Fixture invariants must hold for the test to be meaningful."
)]
fn padded_attribute_nodes(
    profile: &parasolid_core::BuiltinSchemaProfile,
    user_fields: bool,
    padding_kind: u8,
) -> Vec<RawNode> {
    use FieldValue::UnsignedByte;
    let mut nodes = encoded_nodes(profile);
    let definition = nodes
        .iter_mut()
        .find(|node| node.node_type == 80)
        .expect("attribute definition fixture");
    definition.fields[6].values.push(UnsignedByte(padding_kind));
    definition.variable_length = Some(2);
    for node in &mut nodes {
        node.user_fields = if user_fields {
            vec![Some(7)]
        } else {
            Vec::new()
        };
    }
    nodes
}

#[allow(
    clippy::expect_used,
    reason = "Fixture invariants must hold for the test to be meaningful."
)]
fn semantic_owner_source_index(model: &BrepModel, owner: &AttributeOwner) -> u32 {
    let source_index = match owner {
        AttributeOwner::Assembly(id) => model
            .assemblies
            .iter()
            .find(|assembly| assembly.id == *id)
            .map(|assembly| assembly.source.node_index),
        AttributeOwner::Instance(id) => model
            .instances
            .iter()
            .find(|instance| instance.id == *id)
            .map(|instance| instance.source.node_index),
        AttributeOwner::Body(id) => model
            .bodies
            .iter()
            .find(|body| body.id == *id)
            .map(|body| body.source.node_index),
        AttributeOwner::Region(id) => model
            .regions
            .iter()
            .find(|region| region.id == *id)
            .map(|region| region.source.node_index),
        AttributeOwner::Shell(id) => model
            .shells
            .iter()
            .find(|shell| shell.id == *id)
            .map(|shell| shell.source.node_index),
        AttributeOwner::Face(id) => model
            .faces
            .iter()
            .find(|face| face.id == *id)
            .map(|face| face.source.node_index),
        AttributeOwner::Loop(id) => model
            .loops
            .iter()
            .find(|loop_| loop_.id == *id)
            .map(|loop_| loop_.source.node_index),
        AttributeOwner::Edge(id) => model
            .edges
            .iter()
            .find(|edge| edge.id == *id)
            .map(|edge| edge.source.node_index),
        AttributeOwner::Vertex(id) => model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *id)
            .map(|vertex| vertex.source.node_index),
        AttributeOwner::Point(id) => model
            .points
            .iter()
            .find(|point| point.id == *id)
            .map(|point| point.source.node_index),
        AttributeOwner::Curve(id) => model
            .curves
            .iter()
            .find(|curve| curve.id == *id)
            .map(|curve| curve.source.node_index),
        AttributeOwner::Surface(id) => model
            .surfaces
            .iter()
            .find(|surface| surface.id == *id)
            .map(|surface| surface.source.node_index),
        AttributeOwner::Other(source) => Some(source.node_index),
    };
    source_index.expect("mapped attribute owner resolves to a source node")
}

#[allow(
    clippy::panic,
    reason = "Unexpected fixture variants must fail the test with context."
)]
fn assert_semantic_attributes_match_raw(
    model: &BrepModel,
    raw: &parasolid_core::brep::BrepAttributes,
) {
    assert_eq!(model.attributes.len(), raw.attributes.len());
    for (semantic, raw) in model.attributes.iter().zip(&raw.attributes) {
        assert_eq!(semantic.source.node_index, raw.source.node_index);
        assert_eq!(
            semantic_owner_source_index(model, &semantic.owner),
            raw.owner.node_index
        );
        assert_eq!(semantic.identifier.as_bytes(), raw.identifier.as_slice());
        let FieldValue::Integer(raw_kind_id) = &raw.kind_id else {
            panic!("expected integer attribute kind, got {:?}", raw.kind_id);
        };
        assert_eq!(semantic.kind_id, *raw_kind_id);
        assert_eq!(semantic.fields.len(), raw.fields.len());
        for (semantic, raw) in semantic.fields.iter().zip(&raw.fields) {
            match (semantic, &raw.value) {
                (AttributeField::Integers(semantic), AttributeFieldData::Integers(raw)) => {
                    assert_eq!(semantic, raw);
                }
                (semantic, raw) => {
                    panic!("unexpected attribute field mapping: {semantic:?} / {raw:?}")
                }
            }
        }
    }
}

#[allow(
    clippy::expect_used,
    reason = "Fixture invariants must hold for the test to be meaningful."
)]
fn encode_xt(nodes: &[RawNode]) -> Vec<u8> {
    let user_field_size = nodes.first().map_or(0, |node| node.user_fields.len());
    let mut output = support::xt_header(
        KEY,
        i32::try_from(user_field_size).expect("fixture user field count fits i32"),
    );
    for node in nodes {
        output.extend_from_slice(format!("{} ", node.node_type).as_bytes());
        if let Some(length) = node.variable_length {
            output.extend_from_slice(format!("{length} ").as_bytes());
        }
        output.extend_from_slice(format!("{} ", node.index).as_bytes());
        for field in &node.fields {
            if matches!(
                field.definition.field_type,
                FieldType::Character | FieldType::Logical
            ) {
                for value in &field.values {
                    match value {
                        FieldValue::Character(byte) => output.push(*byte),
                        FieldValue::Logical(value) => output.push(if *value { b'T' } else { b'F' }),
                        _ => unreachable!("fixture codec matches schema"),
                    }
                }
                continue;
            }
            for value in &field.values {
                let token = match value {
                    FieldValue::UnsignedByte(value) => value.to_string(),
                    FieldValue::ShortInteger(value) => {
                        value.map_or("-32764".to_owned(), |v| v.to_string())
                    }
                    FieldValue::UnicodeCharacter(value) => value.to_string(),
                    FieldValue::Integer(value) | FieldValue::Tag(value) => {
                        value.map_or("-32764".to_owned(), |v| v.to_string())
                    }
                    FieldValue::PointerIndex(value) => value.to_string(),
                    FieldValue::Double(value) => {
                        value.map_or("-3.14158e13".to_owned(), |v| v.to_string())
                    }
                    _ => unreachable!("fixture codec matches schema"),
                };
                output.extend_from_slice(token.as_bytes());
                output.push(b' ');
            }
        }
        for value in &node.user_fields {
            output.extend_from_slice(value.map_or(-32_764, |value| value).to_string().as_bytes());
            output.push(b' ');
        }
    }
    output.extend_from_slice(b"1 0");
    output
}

#[test]
fn decodes_builtin_binary_attribute_values_and_owner() -> Result<(), Box<dyn std::error::Error>> {
    let profile = onshape_sch30000()?;
    let registry = BuiltinProfileRegistry::new(vec![profile.clone()])?;
    let provider = registry
        .provider_for_key(&SchemaKey::parse(KEY)?)
        .ok_or("profile missing")?;
    let nodes = encoded_nodes(&profile);
    let xb_bytes = support::encode_document_nodes(KEY, &nodes)?;
    let xb = parse_xb(&xb_bytes, &provider, DocumentLimits::default())?;
    let attributes = map_xb_brep_attributes(&xb)?;
    assert_eq!(attributes.attributes.len(), 1);
    let attribute = &attributes.attributes[0];
    assert_eq!(attribute.identifier, b"TEST/ID");
    assert_eq!(attribute.owner.node_index, 1);
    assert_eq!(attribute.fields[0].declared_kind, Some(1));
    assert_eq!(
        attribute.fields[0].value,
        parasolid_core::brep::AttributeFieldData::Integers(vec![Some(42)])
    );
    let xt = parse_xt(&encode_xt(&nodes), &provider, DocumentLimits::default())?;
    let xt_attribute = map_xt_brep_attributes(&xt)?;
    assert_eq!(xt_attribute.attributes[0].identifier, attribute.identifier);
    assert_eq!(
        xt_attribute.attributes[0].owner.node_index,
        attribute.owner.node_index
    );
    assert_eq!(xt_attribute.attributes[0].fields, attribute.fields);
    Ok(())
}

#[test]
fn ignores_zero_definition_padding_when_user_fields_are_present_in_xt_and_xb()
-> Result<(), Box<dyn std::error::Error>> {
    let profile = onshape_sch30000()?;
    let registry = BuiltinProfileRegistry::new(vec![profile.clone()])?;
    let provider = registry
        .provider_for_key(&SchemaKey::parse(KEY)?)
        .ok_or("profile missing")?;
    let nodes = padded_attribute_nodes(&profile, true, 0);
    let xb = parse_xb(
        &support::encode_document_nodes(KEY, &nodes)?,
        &provider,
        DocumentLimits::default(),
    )?;
    assert_eq!(map_xb_brep_attributes(&xb)?.attributes[0].fields.len(), 1);
    let xt = parse_xt(&encode_xt(&nodes), &provider, DocumentLimits::default())?;
    assert_eq!(map_xt_brep_attributes(&xt)?.attributes[0].fields.len(), 1);
    Ok(())
}

#[test]
fn rejects_malformed_definition_padding_and_count_mismatches()
-> Result<(), Box<dyn std::error::Error>> {
    let profile = onshape_sch30000()?;
    let registry = BuiltinProfileRegistry::new(vec![profile.clone()])?;
    let provider = registry
        .provider_for_key(&SchemaKey::parse(KEY)?)
        .ok_or("profile missing")?;
    let mut short_definition = padded_attribute_nodes(&profile, true, 0);
    let definition = short_definition
        .iter_mut()
        .find(|node| node.node_type == 80)
        .ok_or("attribute definition missing")?;
    definition.fields[6].values.clear();
    definition.variable_length = Some(0);
    for nodes in [
        padded_attribute_nodes(&profile, true, 1),
        padded_attribute_nodes(&profile, false, 0),
        short_definition,
    ] {
        let bytes = support::encode_document_nodes(KEY, &nodes)?;
        let document = parse_xb(&bytes, &provider, DocumentLimits::default())?;
        assert_eq!(
            map_xb_brep_attributes(&document)
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::InvalidBrepField)
        );
    }
    Ok(())
}

#[test]
fn rejects_broken_attribute_references() -> Result<(), Box<dyn std::error::Error>> {
    let profile = onshape_sch30000()?;
    let registry = BuiltinProfileRegistry::new(vec![profile.clone()])?;
    let provider = registry
        .provider_for_key(&SchemaKey::parse(KEY)?)
        .ok_or("profile missing")?;
    let bytes = support::encode_document_nodes(KEY, &encoded_nodes(&profile))?;
    let mut document = parse_xb(&bytes, &provider, DocumentLimits::default())?;
    document
        .nodes
        .iter_mut()
        .find(|node| node.node_type == 81)
        .ok_or("attribute missing")?
        .fields[2]
        .values[0] = FieldValue::PointerIndex(999);
    assert_eq!(
        map_xb_brep_attributes(&document)
            .err()
            .map(|error| error.kind()),
        Some(ErrorKind::InvalidBrepField)
    );
    Ok(())
}

#[test]
fn decodes_attributes_with_the_reviewed_13006_profile() -> Result<(), Box<dyn std::error::Error>> {
    const KEY_13006: &str = "SCH_1300000_13006";
    let profile = onshape_sch13006()?;
    let registry = BuiltinProfileRegistry::new(vec![profile.clone()])?;
    let provider = registry
        .provider_for_key(&SchemaKey::parse(KEY_13006)?)
        .ok_or("profile missing")?;
    let bytes = support::encode_document_nodes(KEY_13006, &encoded_nodes(&profile))?;
    let document = parse_xb(&bytes, &provider, DocumentLimits::default())?;
    let attributes = map_xb_brep_attributes(&document)?;
    assert_eq!(attributes.attributes.len(), 1);
    assert_eq!(attributes.attributes[0].identifier, b"TEST/ID");
    assert_eq!(
        attributes.attributes[0].fields[0].value,
        parasolid_core::brep::AttributeFieldData::Integers(vec![Some(42)])
    );
    Ok(())
}

#[test]
#[allow(
    clippy::similar_names,
    reason = "The test compares corresponding XT and XB values."
)]
fn brep_model_retains_xt_and_xb_attributes_from_a_valid_body()
-> Result<(), Box<dyn std::error::Error>> {
    let profile = legacy_standard_keys()?
        .into_iter()
        .find(|profile| profile.metadata().profile_id == "microstation-901297-9008-r1")
        .ok_or("legacy profile missing")?;
    let registry = BuiltinProfileRegistry::new(vec![profile])?;
    let key = SchemaKey::parse(LEGACY_KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("profile missing")?;
    let mut xt_bytes = support::xt_header(LEGACY_KEY, 0);
    xt_bytes.extend_from_slice(VALID_BODY_WITH_ATTRIBUTE.as_bytes());
    let xt = parse_xt(&xt_bytes, &provider, DocumentLimits::default())?;
    let xt_model = map_xt_brep(&xt)?;
    let xt_attributes = map_xt_brep_attributes(&xt)?;
    assert_semantic_attributes_match_raw(&xt_model, &xt_attributes);
    assert_eq!(xt_model.attributes.len(), 1);

    let xb_bytes = support::encode_document_nodes(LEGACY_KEY, &xt.nodes)?;
    let xb = parse_xb(&xb_bytes, &provider, DocumentLimits::default())?;
    let xb_model = map_xb_brep(&xb)?;
    let xb_attributes = map_xb_brep_attributes(&xb)?;
    assert_semantic_attributes_match_raw(&xb_model, &xb_attributes);
    assert_eq!(xb_model.attributes.len(), xt_model.attributes.len());
    for (xb_attribute, xt_attribute) in xb_model.attributes.iter().zip(&xt_model.attributes) {
        assert_eq!(xb_attribute.identifier, xt_attribute.identifier);
        assert_eq!(xb_attribute.kind_id, xt_attribute.kind_id);
        assert_eq!(xb_attribute.owner, xt_attribute.owner);
        assert_eq!(xb_attribute.fields, xt_attribute.fields);
        assert_eq!(
            xb_attribute.source.node_index,
            xt_attribute.source.node_index
        );
        assert_eq!(xb_attribute.source.node_type, xt_attribute.source.node_type);
    }
    let xb_attribute = &xb_attributes.attributes[0];
    let xt_attribute = &xt_attributes.attributes[0];
    assert_eq!(xb_attribute.identifier, xt_attribute.identifier);
    assert_eq!(xb_attribute.kind_id, xt_attribute.kind_id);
    assert_eq!(xb_attribute.owner.node_index, xt_attribute.owner.node_index);
    assert_eq!(xb_attribute.fields, xt_attribute.fields);
    Ok(())
}

#[test]
#[allow(
    clippy::similar_names,
    reason = "The test compares corresponding XT and XB values."
)]
fn malformed_attribute_references_recover_without_losing_xt_or_xb_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    let profile = legacy_standard_keys()?
        .into_iter()
        .find(|profile| profile.metadata().profile_id == "microstation-901297-9008-r1")
        .ok_or("legacy profile missing")?;
    let registry = BuiltinProfileRegistry::new(vec![profile])?;
    let provider = registry
        .provider_for_key(&SchemaKey::parse(LEGACY_KEY)?)
        .ok_or("profile missing")?;
    let mut xt_bytes = support::xt_header(LEGACY_KEY, 0);
    xt_bytes.extend_from_slice(VALID_BODY_WITH_ATTRIBUTE.as_bytes());
    let xt = parse_xt(&xt_bytes, &provider, DocumentLimits::default())?;
    let valid_xt_model = map_xt_brep(&xt)?;
    let xb_bytes = support::encode_document_nodes(LEGACY_KEY, &xt.nodes)?;
    let xb = parse_xb(&xb_bytes, &provider, DocumentLimits::default())?;
    let valid_xb_model = map_xb_brep(&xb)?;

    let mut malformed_xt = xt;
    malformed_xt
        .nodes
        .iter_mut()
        .find(|node| node.node_type == 81)
        .ok_or("XT attribute node missing")?
        .fields[7]
        .values[0] = FieldValue::PointerIndex(u32::MAX);
    assert!(map_xt_brep(&malformed_xt).is_err());
    let recovered_xt = map_xt_brep_with_metadata_recovery(&malformed_xt)?;
    assert_attribute_recovery_preserves_geometry(&valid_xt_model, &recovered_xt);

    let mut malformed_xb = xb;
    malformed_xb
        .nodes
        .iter_mut()
        .find(|node| node.node_type == 81)
        .ok_or("XB attribute node missing")?
        .fields[7]
        .values[0] = FieldValue::PointerIndex(u32::MAX);
    assert!(map_xb_brep(&malformed_xb).is_err());
    let recovered_xb = map_xb_brep_with_metadata_recovery(&malformed_xb)?;
    assert_attribute_recovery_preserves_geometry(&valid_xb_model, &recovered_xb);
    Ok(())
}

fn assert_attribute_recovery_preserves_geometry(valid: &BrepModel, recovered: &BrepModel) {
    assert_eq!(recovered.bodies, valid.bodies);
    assert_eq!(recovered.regions, valid.regions);
    assert_eq!(recovered.shells, valid.shells);
    let mut valid_faces = valid.faces.clone();
    let mut recovered_faces = recovered.faces.clone();
    assert!(valid_faces.iter().any(|face| !face.attributes.is_empty()));
    assert!(
        recovered_faces
            .iter()
            .all(|face| face.attributes.is_empty())
    );
    for face in &mut valid_faces {
        face.attributes.clear();
    }
    for face in &mut recovered_faces {
        face.attributes.clear();
    }
    assert_eq!(recovered_faces, valid_faces);
    assert_eq!(recovered.loops, valid.loops);
    assert_eq!(recovered.half_edges, valid.half_edges);
    assert_eq!(recovered.edges, valid.edges);
    assert_eq!(recovered.vertices, valid.vertices);
    assert_eq!(recovered.points, valid.points);
    assert_eq!(recovered.curves, valid.curves);
    assert_eq!(recovered.surfaces, valid.surfaces);
    assert!(recovered.attributes.is_empty());
    assert_eq!(recovered.complete, valid.complete);
    assert!(
        recovered
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata.attribute_decode_failed")
    );
}
