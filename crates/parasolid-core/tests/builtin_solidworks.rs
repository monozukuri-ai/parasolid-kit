#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, DocumentLimits, FieldValue, SchemaKey,
    SchemaProvider, SchemaTypeLookup, parse_xb, parse_xt,
    schema::profiles::{solidworks_partitions_13006, solidworks_sch37102_13006},
};

const KEY: &str = "SCH_3701229_37102_13006";
/// The shared `SolidWorks` declaration set: 58 types and 468 field groups.
const TYPES: usize = 58;
const FIELD_GROUPS: usize = 468;
/// Base types outside every `SolidWorks` profile: the delta update records.
const UNKNOWN_TYPES: [u16; 2] = [3, 4];

/// Earlier keys, their profile ids and canonical hashes.
const EARLIER: &[(&str, &str, &str)] = &[
    (
        "SCH_2700199_26105_13006",
        "solidworks-2700199-26105-13006-r3",
        "71f1eb426abd13984739aa01009c237be5a176140c00776e4ae5345711a92928",
    ),
    (
        "SCH_2800191_28002_13006",
        "solidworks-2800191-28002-13006-r3",
        "8bedc5c55d98b876012afc83cecf32e2af883ab2f1b85488d3eb1b68be5c53ba",
    ),
    (
        "SCH_2800209_28002_13006",
        "solidworks-2800209-28002-13006-r3",
        "8cb6615798a8afe525bdd2dfb1f153f358cf702bbbf490f04c14a42a0e760090",
    ),
    (
        "SCH_2900166_28101_13006",
        "solidworks-2900166-28101-13006-r3",
        "7096617ce7a03febd577460c60c3da38ef8c0afe85a79da28fa34fb3f3d16253",
    ),
    (
        "SCH_2900190_28101_13006",
        "solidworks-2900190-28101-13006-r3",
        "5e3f76ffd1a090c6a0a23fd7d1b6de0541e8aad86cd5466721581380da62f5f3",
    ),
    (
        "SCH_2900195_28101_13006",
        "solidworks-2900195-28101-13006-r3",
        "df4501986282d4682094cf8dac682764f9ab75c059a060d7ef1368cca3334b0e",
    ),
    (
        "SCH_3000226_30000_13006",
        "solidworks-3000226-30000-13006-r3",
        "9ac6a98ac91cb2dbc66fe8d10a89c7a750bb379b24189b86f9403e7249016f12",
    ),
    (
        "SCH_3000298_30000_13006",
        "solidworks-3000298-30000-13006-r3",
        "5a1e21f7bca73720ffd3dcb77cdf8c9405d662f2807123caa8d58974c2ee1822",
    ),
    (
        "SCH_3100203_31001_13006",
        "solidworks-3100203-31001-13006-r3",
        "68977cb927733ad30279b6566874dc16cc4f7ef2858a9a5f395b233dcce65851",
    ),
    (
        "SCH_3101290_31100_13006",
        "solidworks-3101290-31100-13006-r3",
        "0477947dd1a5b6fc11baaa344637e44d44af0bec1ae82a1de8dd956272b1c5f4",
    ),
    (
        "SCH_3401254_34101_13006",
        "solidworks-3401254-34101-13006-r3",
        "e7206570f57a650be545095dca12c517b82106dc55540a613e3002022e457867",
    ),
    (
        "SCH_3501210_35102_13006",
        "solidworks-3501210-35102-13006-r3",
        "463fd89c390ef3be8a4d2ee8f790008f00c8146b0d6690d89fa3c0fa66b124bc",
    ),
    (
        "SCH_3501256_35102_13006",
        "solidworks-3501256-35102-13006-r3",
        "cd3dc807919d5f40ae8d8b39eb2889787e88689c727c14ee903b4a02508757ec",
    ),
    (
        "SCH_3601228_36001_13006",
        "solidworks-3601228-36001-13006-r3",
        "cf6e41ea2c801aa42338a6fb7710fc2510831ebadcc521087b2e9ce0bee38b5d",
    ),
];

/// Another build of the same schema resolves through the derived family; another
/// schema or base resolves to nothing.
fn assert_exact_key_or_family(
    registry: &BuiltinProfileRegistry,
    key: &SchemaKey,
) -> support::Result<()> {
    for component in 1..=3 {
        let mut parts = key.raw().split('_').map(str::to_owned).collect::<Vec<_>>();
        parts[component].push('9');
        let nearby = SchemaKey::parse(&parts.join("_"))?;
        assert!(registry.profile_for_key(&nearby).is_none());
        let provider = registry.provider_for_key(&nearby);
        if component == 1 {
            let provider = provider.ok_or("same-schema build must resolve through its family")?;
            assert_eq!(
                provider.profile().metadata().profile_id,
                format!("family-{}", nearby.family_id())
            );
            assert_eq!(
                provider.profile().metadata().coverage,
                BuiltinProfileCoverage::FamilyInferred
            );
            assert_eq!(provider.selected_schema_key(), nearby.raw());
        } else {
            assert!(provider.is_none(), "{nearby:?} must not resolve");
        }
    }
    Ok(())
}

#[test]
fn solidworks_profile_has_reviewed_world_and_exact_membership() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let profile = provider.profile();
    assert_eq!(
        profile.metadata().profile_id,
        "solidworks-sch37102-13006-r4"
    );
    assert_eq!(profile.metadata().revision, 4);
    assert_eq!(profile.definitions().len(), TYPES);
    assert_eq!(
        profile.definitions().map(|d| d.fields.len()).sum::<usize>(),
        FIELD_GROUPS
    );
    assert_eq!(
        profile.metadata().profile_sha256,
        support::profile_hash(profile)?
    );
    assert_eq!(
        profile.definition(101).ok_or("missing WORLD")?.fields.len(),
        11
    );
    assert_eq!(
        profile
            .definition(67)
            .ok_or("missing SWEPT_SURF")?
            .fields
            .len(),
        10
    );
    for kind in UNKNOWN_TYPES {
        assert_eq!(
            provider.lookup_type("13006", kind),
            SchemaTypeLookup::Unknown
        );
    }
    assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
    assert!(!provider.supports_schema_key(&SchemaKey::parse("SCH_3701230_37102_13006")?));
    assert!(!provider.supports_schema_key(&SchemaKey::parse("SCH_3000310_30000_13006")?));
    assert_exact_key_or_family(registry, &key)
}

#[test]
fn earlier_keys_share_declarations_and_pin_canonical_hashes() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let profiles = solidworks_partitions_13006()?;
    assert_eq!(profiles.len(), EARLIER.len());
    let reference = solidworks_sch37102_13006()?;
    let reference_types = reference
        .definitions()
        .map(|d| (d.node_type, d.fields.clone(), d.variable))
        .collect::<Vec<_>>();
    for (profile, (expected_key, expected_id, expected_hash)) in profiles.iter().zip(EARLIER) {
        let key = profile.accepted_schema_keys().next().ok_or("key")?;
        assert_eq!(key.raw(), *expected_key);
        assert_eq!(profile.accepted_schema_keys().len(), 1);
        assert_eq!(profile.metadata().profile_id, *expected_id);
        assert_eq!(profile.metadata().revision, 3);
        assert_eq!(profile.metadata().profile_sha256, *expected_hash);
        assert_eq!(
            profile.metadata().profile_sha256,
            support::profile_hash(profile)?
        );
        assert_eq!(profile.definitions().len(), TYPES);
        assert_eq!(
            profile.definitions().map(|d| d.fields.len()).sum::<usize>(),
            FIELD_GROUPS
        );
        let types = profile
            .definitions()
            .map(|d| (d.node_type, d.fields.clone(), d.variable))
            .collect::<Vec<_>>();
        assert_eq!(types, reference_types);
        let provider = registry.provider_for_key(key).ok_or("provider")?;
        assert_eq!(provider.profile().metadata().profile_id, *expected_id);
        for kind in UNKNOWN_TYPES {
            assert_eq!(
                provider.lookup_type("13006", kind),
                SchemaTypeLookup::Unknown
            );
        }
        assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
        assert_exact_key_or_family(registry, key)?;
    }
    Ok(())
}

fn world_pair() -> support::Result<(Vec<u8>, Vec<u8>)> {
    let mut binary = support::xb_header(KEY, 0)?;
    binary.extend_from_slice(&[0, 101, 0xff]);
    binary.extend_from_slice(&support::pointer(7)?);
    for index in [0, 0, 23, 0, 0, 0, 0] {
        binary.extend_from_slice(&support::pointer(index)?);
    }
    binary.push(1);
    binary.extend_from_slice(&support::pointer(41)?);
    binary.extend_from_slice(&123_456_i32.to_be_bytes());
    binary.extend_from_slice(&789_i32.to_be_bytes());
    support::terminate(&mut binary);
    let mut text = support::xt_header(KEY, 0);
    text.extend_from_slice(b"101 255 7 0 0 23 0 0 0 0 T41 123456 789 1 0");
    Ok((binary, text))
}

#[test]
fn world_fields_match_text_binary_and_independent_encoding() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let (binary, text) = world_pair()?;
    let parsed = parse_xb(&binary, &provider, DocumentLimits::default())?;
    let textual = parse_xt(&text, &provider, DocumentLimits::default())?;
    let node = &parsed.nodes[0];
    assert_eq!(node.node_type, 101);
    assert_eq!(node.index, 7);
    assert_eq!(node.fields[2].values, [FieldValue::PointerIndex(23)]);
    assert_eq!(node.fields[7].values, [FieldValue::Logical(true)]);
    assert_eq!(node.fields[8].values, [FieldValue::PointerIndex(41)]);
    assert_eq!(node.fields[9].values, [FieldValue::Integer(Some(123_456))]);
    assert_eq!(node.fields[10].values, [FieldValue::Integer(Some(789))]);
    for (left, right) in node.fields.iter().zip(&textual.nodes[0].fields) {
        assert_eq!(left.values, right.values);
        assert!(left.byte_range.start >= node.byte_range.start);
        assert!(left.byte_range.end <= node.byte_range.end);
    }
    assert_eq!(support::encode_document_nodes(KEY, &parsed.nodes)?, binary);
    for end in 0..binary.len() {
        assert!(parse_xb(&binary[..end], &provider, DocumentLimits::default()).is_err());
    }
    let mut invalid = binary;
    invalid[node.fields[7].byte_range.start] = 2;
    assert!(parse_xb(&invalid, &provider, DocumentLimits::default()).is_err());
    Ok(())
}

#[test]
fn helical_forms_and_field_names_follow_the_reference_layouts() -> support::Result<()> {
    let profile = solidworks_sch37102_13006()?;
    let codes = |kind: u16| -> support::Result<String> {
        Ok(profile
            .definition(kind)
            .ok_or("definition")?
            .fields
            .iter()
            .map(|field| field.field_type.code())
            .collect())
    };
    // 163: the surface form (axis, hand, turns, pitch, gap, tolerance);
    // 184: the curve form (axis, point, hand, turns, pitch, tolerance).
    assert_eq!(codes(163)?, "vvcifff");
    assert_eq!(codes(184)?, "vvvciff");
    let surface = profile.definition(163).ok_or("163")?;
    assert_eq!(surface.fields[5].name, "gap");
    let curve = profile.definition(184).ok_or("184")?;
    assert_eq!(curve.fields[2].name, "point");
    // FIELD_NAMES: one variable array of FIELD_NAME (1037) pointers.
    let names = profile.definition(99).ok_or("99")?;
    assert!(names.variable);
    assert_eq!(names.fields.len(), 1);
    assert_eq!(names.fields[0].pointer_class, 1037);
    assert_eq!(names.fields[0].element_count, 1);
    Ok(())
}
