#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, FieldValue, SchemaKey, SchemaProvider,
    SchemaTypeLookup, parse_xb, parse_xt,
    schema::profiles::{solidworks_partitions_13006, solidworks_sch37102_13006},
};

const KEY: &str = "SCH_3701229_37102_13006";
/// The shared `SolidWorks` declaration set: 47 types and 412 field groups.
const TYPES: usize = 47;
const FIELD_GROUPS: usize = 412;
/// Base types outside every `SolidWorks` profile, including the delta update
/// records (3 and 4) and layouts without local evidence (163, 184).
const UNKNOWN_TYPES: [u16; 4] = [3, 4, 163, 184];

/// Earlier keys, their profile ids and canonical hashes.
const EARLIER: &[(&str, &str, &str)] = &[
    (
        "SCH_2700199_26105_13006",
        "solidworks-2700199-26105-13006-r1",
        "65531d74c0e96e760414414a1df7065affa26c316f918617307158c491aa27d8",
    ),
    (
        "SCH_2800191_28002_13006",
        "solidworks-2800191-28002-13006-r1",
        "24524d4cca8bfe316cdf936de6f440dc83199384981132308603e792e662fe97",
    ),
    (
        "SCH_2800209_28002_13006",
        "solidworks-2800209-28002-13006-r1",
        "e65c8ddf1bf2876923b6e12f7b284750f2518a0f799d7696d10c74323a8cfc76",
    ),
    (
        "SCH_2900166_28101_13006",
        "solidworks-2900166-28101-13006-r1",
        "728d84812ee3cc63072085effa633b320ec4e41d3bdb1dafda3ff0d0be91655c",
    ),
    (
        "SCH_2900190_28101_13006",
        "solidworks-2900190-28101-13006-r1",
        "789a8804c22fbf038d9bcbedd8ce02d16731c9840e049b3b13bebf6951a9b250",
    ),
    (
        "SCH_2900195_28101_13006",
        "solidworks-2900195-28101-13006-r1",
        "f8cc88a32e3af19961aec053b1d341a7bc9934a780d63945e91908dca2ba95fb",
    ),
    (
        "SCH_3000226_30000_13006",
        "solidworks-3000226-30000-13006-r1",
        "14354ebf3830d1141ed4c61e3a9ce99f78f5216c381aa994f99de067903068a8",
    ),
    (
        "SCH_3000298_30000_13006",
        "solidworks-3000298-30000-13006-r1",
        "0d3a628fb09fb47a23701c2c99159bbd8d7c1bb0f96a2b33fa48cd4f4f21f8fa",
    ),
    (
        "SCH_3100203_31001_13006",
        "solidworks-3100203-31001-13006-r1",
        "314b947f7a8cebfb6ed382206065594ce6d7acc79a1d78765dac8dccf56a26f7",
    ),
    (
        "SCH_3101290_31100_13006",
        "solidworks-3101290-31100-13006-r1",
        "4a6bbe2db4b7b605447cbe5b13b79ff2fd215b182ed68b7e79a58b8a525e553d",
    ),
    (
        "SCH_3401254_34101_13006",
        "solidworks-3401254-34101-13006-r1",
        "aa746d6cbea446ca37593e5cdbc9b7ff9fbf5b0bd4096152823740a5f65191da",
    ),
    (
        "SCH_3501210_35102_13006",
        "solidworks-3501210-35102-13006-r1",
        "a99c8793ea67bebaf50d2dd7b5e61f842cb5fce6c563840df94a2182bdf18dc0",
    ),
    (
        "SCH_3501256_35102_13006",
        "solidworks-3501256-35102-13006-r1",
        "b4c382da97bead88b8065fa913a72c65d0e1e0f6999e01894fa9ecee2bf21ef6",
    ),
    (
        "SCH_3601228_36001_13006",
        "solidworks-3601228-36001-13006-r1",
        "1117b129bcb099219a8c61dc813b0a06cdf76daa85d3f567cf37dc0a156593f2",
    ),
];

fn assert_exact_key_only(
    registry: &BuiltinProfileRegistry,
    key: &SchemaKey,
) -> support::Result<()> {
    for component in 1..=3 {
        let mut parts = key.raw().split('_').map(str::to_owned).collect::<Vec<_>>();
        parts[component].push('9');
        let nearby = SchemaKey::parse(&parts.join("_"))?;
        assert!(
            registry.provider_for_key(&nearby).is_none(),
            "{nearby:?} must not resolve"
        );
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
        "solidworks-sch37102-13006-r2"
    );
    assert_eq!(profile.metadata().revision, 2);
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
    assert_exact_key_only(registry, &key)
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
        assert_eq!(profile.metadata().revision, 1);
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
        assert_exact_key_only(registry, key)?;
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
