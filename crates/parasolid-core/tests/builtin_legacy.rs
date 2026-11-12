//! Authored checks for the older standard keys, the `MicroStation` modeler
//! key `SCH_901297_9008` in detail. Field codes and record values are stated
//! here independently of the profile table; the public files are compared
//! separately and are not redistributed.

#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::brep::{AttributeField, AttributeOwner, map_xt_brep};
use parasolid_core::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, BuiltinSchemaProfile, DocumentLimits,
    FieldValue, SchemaKey, SchemaProvider, SchemaProviderResolution, SchemaTypeLookup, parse_xt,
    schema::profiles::legacy_standard_keys,
};
use support::Result;

const KEY: &str = "SCH_901297_9008";
const PROFILE: &str = "microstation-901297-9008-r1";
const TYPES: [u16; 44] = [
    12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52, 53, 54, 56, 59, 60,
    68, 70, 74, 79, 80, 81, 82, 83, 84, 90, 91, 124, 125, 126, 127, 128, 134, 135, 136, 141,
];

/// The public XT Format Reference's transmit example (printed page 10, a disc
/// under schema 12006) written in the 9008 layouts: BODY without
/// `nom_geom_state`, both fins without their attribute pointer, and the
/// attribute definition without field names and with thirteen legal owners.
/// Its real values are replaced by integer values, the only value node the
/// Bentley files carry.
const DISC: &str = concat!(
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

fn codes(profile: &BuiltinSchemaProfile, node_type: u16) -> Option<String> {
    profile.definition(node_type).map(|definition| {
        definition
            .fields
            .iter()
            .map(|field| field.field_type.code())
            .collect()
    })
}

fn stream(key: &str, nodes: &str) -> Vec<u8> {
    let mut bytes = support::xt_header(key, 0);
    bytes.extend_from_slice(nodes.as_bytes());
    bytes
}

#[test]
fn exact_profile_layouts_and_canonical_hash() -> Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let profiles = legacy_standard_keys()?;
    let profile = profiles
        .iter()
        .find(|item| item.metadata().profile_id == PROFILE)
        .ok_or("profile")?;
    let key = profile.accepted_schema_keys().next().ok_or("key")?;
    assert_eq!(key.raw(), KEY);
    assert_eq!(profile.accepted_schema_keys().len(), 1);
    let metadata = profile.metadata();
    assert_eq!(metadata.profile_id, PROFILE);
    assert_eq!(metadata.revision, 1);
    assert_eq!(metadata.provider_schema, "9008");
    assert_eq!(metadata.coverage, BuiltinProfileCoverage::VerifiedSubset);
    assert_eq!(metadata.profile_sha256, support::profile_hash(profile)?);
    let mut declared: Vec<u16> = profile.definitions().map(|d| d.node_type).collect();
    declared.sort_unstable();
    assert_eq!(declared, TYPES);

    // BODY drops the "for future use" byte before the obsolete shell pointer,
    // FIN its attribute pointer; the attribute definition keeps the eight
    // event actions, loses its field names and lists thirteen legal owners.
    assert_eq!(
        codes(profile, 12).as_deref(),
        Some("dppppppffpppupuppppppp")
    );
    assert_eq!(codes(profile, 17).as_deref(), Some("ppppppppc"));
    assert_eq!(codes(profile, 80).as_deref(), Some("ppdulu"));
    let base = registry.profile("family-13006").ok_or("family 13006")?;
    for kind in TYPES {
        let own = profile.definition(kind).ok_or("definition")?;
        let reviewed = base.definition(kind).ok_or("base definition")?;
        match kind {
            12 => {
                assert_eq!(reviewed.fields.len(), 23);
                assert_eq!(reviewed.fields[15].name, "geometry_state");
                let mut expected = reviewed.fields.clone();
                expected.remove(15);
                assert_eq!(own.fields, expected);
            }
            17 => {
                assert_eq!(reviewed.fields[0].name, "annotations");
                assert_eq!(own.fields, reviewed.fields[1..]);
            }
            135 => {
                assert_eq!(reviewed.fields.len(), 2);
                assert_eq!(own.fields, reviewed.fields[..1]);
            }
            80 => {
                assert_eq!(own.fields[..4], reviewed.fields[..4]);
                assert_eq!(reviewed.fields[4].name, "field_labels");
                assert_eq!(own.fields[4].name, "allowed_owners");
                assert_eq!(own.fields[4].element_count, 13);
                assert_eq!(reviewed.fields[5].element_count, 14);
                assert_eq!(own.fields[5], reviewed.fields[6]);
                assert!(own.variable);
            }
            _ => assert_eq!(own.fields, reviewed.fields, "type {kind}"),
        }
    }

    // Only the observed types are declared; a neighbouring build of the same
    // schema resolves through the derived family of this one profile.
    let provider = registry.provider_for_key(key).ok_or("provider")?;
    for kind in [10, 11, 67, 98, 100, 102, 133, 137] {
        assert_eq!(
            provider.lookup_type("9008", kind),
            SchemaTypeLookup::Unknown
        );
    }
    assert!(matches!(
        provider.lookup_type("9008", 54),
        SchemaTypeLookup::Defined(_)
    ));
    let other_build = SchemaKey::parse("SCH_900000_9008")?;
    assert!(registry.profile_for_key(&other_build).is_none());
    let family = registry.family_for_key(&other_build).ok_or("family")?;
    assert_eq!(family.metadata().profile_id, "family-9008");
    assert_eq!(family.members().len(), 1);
    assert_eq!(family.members()[0].profile_id, PROFILE);
    assert_eq!(family.base_members(), []);
    assert_eq!(family.definitions().len(), TYPES.len());
    Ok(())
}

#[test]
fn the_reshaped_reference_example_decodes_to_its_terminator() -> Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("provider")?;
    let bytes = stream(KEY, DISC);
    let document = parse_xt(&bytes, &provider, DocumentLimits::default())?;
    assert!(matches!(
        &document.schema_provider,
        SchemaProviderResolution::Builtin {
            profile_id,
            profile_revision: 1,
            schema_key,
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            ..
        } if profile_id == PROFILE && schema_key == KEY
    ));
    assert_eq!(document.nodes.len(), 16);
    assert_eq!(document.terminator.byte_range.end, bytes.len());
    let body = &document.nodes[0];
    assert_eq!(body.fields.len(), 22);
    assert_eq!(body.fields[0].values, [FieldValue::Integer(Some(12))]);
    assert_eq!(body.fields[14].definition.name, "body_kind");
    assert_eq!(body.fields[14].values, [FieldValue::UnsignedByte(3)]);
    assert_eq!(body.fields[15].definition.name, "legacy_shell");
    assert_eq!(body.fields[19].definition.name, "region_head");
    assert_eq!(body.fields[19].values, [FieldValue::PointerIndex(6)]);
    let fin = &document.nodes[7];
    assert_eq!((fin.node_type, fin.index), (17, 10));
    assert_eq!(fin.fields.len(), 9);
    assert_eq!(fin.fields[0].definition.name, "loop_ref");
    assert_eq!(fin.fields[0].values, [FieldValue::PointerIndex(11)]);
    assert_eq!(fin.fields[8].values, [FieldValue::Character(b'+')]);
    let definition = &document.nodes[12];
    assert_eq!((definition.node_type, definition.index), (80, 14));
    assert_eq!(definition.fields[1].values, [FieldValue::PointerIndex(16)]);
    assert_eq!(definition.fields[3].values.len(), 8);
    let owners = &definition.fields[4].values;
    assert_eq!(owners.len(), 13);
    assert_eq!(
        owners
            .iter()
            .filter(|v| **v == FieldValue::Logical(true))
            .count(),
        2
    );
    assert_eq!(owners[4], FieldValue::Logical(true));
    assert_eq!(definition.fields[5].values, [FieldValue::UnsignedByte(1)]);
    let identifier = &document.nodes[14];
    assert_eq!((identifier.node_type, identifier.index), (79, 16));
    assert_eq!(identifier.fields[0].values.len(), 15);

    // The colour attribute of the example maps to the disc's face.
    let model = map_xt_brep(&document)?;
    assert_eq!(model.attributes.len(), 1);
    let attribute = &model.attributes[0];
    assert_eq!(attribute.identifier, "SDL/TYSA_COLOUR");
    assert_eq!(attribute.kind_id, Some(8001));
    assert_eq!(attribute.owner, AttributeOwner::Face(0));
    assert_eq!(
        attribute.fields,
        [AttributeField::Integers(vec![Some(1), Some(2), Some(3)])]
    );
    assert_eq!(model.faces[0].attributes, [0]);

    // The V13 attribute definition (field names, fourteen owners) and a value
    // node the MicroStation files do not carry both fail closed under this key.
    let v13 = DISC.replace("FFFFTFTFFFFFF1 ", "0 FFFFTFTFFFFFFF1 ");
    assert!(parse_xt(&stream(KEY, &v13), &provider, DocumentLimits::default()).is_err());
    let unicode = DISC.replace("82 3 15 1 2 3 ", "98 3 15 1 2 3 ");
    assert!(parse_xt(&stream(KEY, &unicode), &provider, DocumentLimits::default()).is_err());
    for end in 0..bytes.len() - 1 {
        assert!(parse_xt(&bytes[..end], &provider, DocumentLimits::default()).is_err());
    }
    Ok(())
}

#[test]
fn every_legacy_profile_has_its_canonical_hash() -> Result<()> {
    let mut mismatches = Vec::new();
    for profile in legacy_standard_keys()? {
        let digest = support::profile_hash(&profile)?;
        if profile.metadata().profile_sha256 != digest {
            mismatches.push(format!("HASH {} {digest}", profile.metadata().profile_id));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    Ok(())
}

#[test]
fn parasolid_8_12_and_25001_layouts_follow_their_streams() -> Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let profiles = legacy_standard_keys()?;
    let profile = |id: &str| {
        profiles
            .iter()
            .find(|item| item.metadata().profile_id == id)
            .ok_or("profile")
    };
    let base = registry.profile("family-13006").ok_or("family 13006")?;

    // Schema 8008 shares every edit of 9003: BODY without the "for future
    // use" byte, FIN without its attribute pointer, the list without its
    // transmission flag and thirteen owners without field names.
    let parasolid_8 = profile("solidworks-800000-8008-r1")?;
    assert_eq!(parasolid_8.metadata().provider_schema, "8008");
    assert_eq!(
        codes(parasolid_8, 12).as_deref(),
        Some("dppppppffpppupuppppppp")
    );
    assert_eq!(codes(parasolid_8, 17).as_deref(), Some("ppppppppc"));
    assert_eq!(codes(parasolid_8, 70).as_deref(), Some("dpppddddppd"));
    assert_eq!(codes(parasolid_8, 80).as_deref(), Some("ppdulu"));
    assert_eq!(codes(parasolid_8, 135).as_deref(), Some("u"));
    let owners = parasolid_8.definition(80).ok_or("ATTRIB_DEF")?;
    assert_eq!(owners.fields[4].element_count, 13);

    // Schema 12006 as SolidWorks and CAD Exchanger write it: fourteen owners
    // without field names; CURVE_DATA keeps its analytic-form pointer.
    let reference = profile("solidworks-1200000-12006-r2")?;
    assert_eq!(reference.metadata().revision, 2);
    assert_eq!(codes(reference, 80).as_deref(), Some("ppdulu"));
    let definition = reference.definition(80).ok_or("ATTRIB_DEF")?;
    assert_eq!(definition.fields[4].name, "allowed_owners");
    assert_eq!(definition.fields[4].element_count, 14);
    assert_eq!(codes(reference, 135).as_deref(), Some("up"));
    assert_eq!(codes(reference, 163).as_deref(), Some("vvcifff"));

    // Schema 25001: the BODY of embedded 25001 streams (29 fields), the
    // region owner, the list header and the pointer block's index origin.
    let inventor = profile("inventor-2600169-25001-r1")?;
    assert_eq!(inventor.metadata().provider_schema, "25001");
    assert_eq!(
        codes(inventor, 12).as_deref(),
        Some("dppppppffpppupuupppppppdppppd")
    );
    let body = inventor.definition(12).ok_or("BODY")?;
    assert_eq!(body.fields[13].pointer_class, 1040);
    assert_eq!(body.fields[20].name, "region_head");
    assert_eq!(codes(inventor, 19).as_deref(), Some("dpppppcp"));
    assert_eq!(codes(inventor, 70).as_deref(), Some("dulpppdddpp"));
    assert_eq!(codes(inventor, 74).as_deref(), Some("ddpp"));

    // Every other declared type keeps its reviewed 13006 layout.
    for (profile, edited) in [
        (parasolid_8, &[12, 17, 70, 80, 135][..]),
        (reference, &[80][..]),
        (inventor, &[12, 19, 70, 74][..]),
    ] {
        for definition in profile.definitions() {
            if edited.contains(&definition.node_type) {
                continue;
            }
            if let Some(reviewed) = base.definition(definition.node_type) {
                assert_eq!(
                    definition.fields,
                    reviewed.fields,
                    "{} type {}",
                    profile.metadata().profile_id,
                    definition.node_type
                );
            }
        }
    }

    // Other builds of the two new schemas resolve through their families.
    for (key, family) in [
        ("SCH_800194_8008", "family-8008"),
        ("SCH_2600000_25001", "family-25001"),
    ] {
        let provider = registry
            .provider_for_key(&SchemaKey::parse(key)?)
            .ok_or("provider")?;
        assert_eq!(provider.profile().metadata().profile_id, family);
    }
    Ok(())
}
