#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, DocumentLimits, FieldValue, SchemaKey,
    SchemaProvider, SchemaTypeLookup, parse_xb, parse_xt,
    schema::profiles::{solidworks_partitions_13006, solidworks_sch37102_13006},
};

const KEY: &str = "SCH_3701229_37102_13006";
/// The shared `SolidWorks` declaration set: 56 types and 460 field groups.
const TYPES: usize = 56;
const FIELD_GROUPS: usize = 460;
/// Base types outside every `SolidWorks` profile, including the delta update
/// records (3 and 4) and the unreviewed surface descriptor (184).
const UNKNOWN_TYPES: [u16; 3] = [3, 4, 184];

/// Earlier keys, their profile ids and canonical hashes.
const EARLIER: &[(&str, &str, &str)] = &[
    (
        "SCH_2700199_26105_13006",
        "solidworks-2700199-26105-13006-r2",
        "edab51092b3b8d1e0f137e6e3d7f666348a433b2bf17a932e8b708886e6b4331",
    ),
    (
        "SCH_2800191_28002_13006",
        "solidworks-2800191-28002-13006-r2",
        "388cf98e9768b041ff7dd572df1e63f83c41afc36eaa635ccf61fdc272214a3e",
    ),
    (
        "SCH_2800209_28002_13006",
        "solidworks-2800209-28002-13006-r2",
        "162b6cdd51c8b6ac6bcbddf363c2b9cba98262d4e08365d31576b30a3653fe3e",
    ),
    (
        "SCH_2900166_28101_13006",
        "solidworks-2900166-28101-13006-r2",
        "f34b810890bf48d501fd941a5e2793741df03915b93292ce3abb36d6b16a6bff",
    ),
    (
        "SCH_2900190_28101_13006",
        "solidworks-2900190-28101-13006-r2",
        "6625faaa9d5afae86fa6f9058a6ae36a2bc97852f6d66dd8d68b3c899a868f7f",
    ),
    (
        "SCH_2900195_28101_13006",
        "solidworks-2900195-28101-13006-r2",
        "3ed9e73e24121d843d2393b42462c5ae633c8054ec8f66b54ebddc2ea4c8a28b",
    ),
    (
        "SCH_3000226_30000_13006",
        "solidworks-3000226-30000-13006-r2",
        "734a40413d7a1a97a2344e5d6379255b299365ea50361ff0286c7f90fcd956cc",
    ),
    (
        "SCH_3000298_30000_13006",
        "solidworks-3000298-30000-13006-r2",
        "c9d5c679dba4855d478de10e6c0cd88167113bbdc0a9553b259bab261e490cfd",
    ),
    (
        "SCH_3100203_31001_13006",
        "solidworks-3100203-31001-13006-r2",
        "b209ba14673b1fb93d43bb4d74aab652e63fba9d9bbeb88e4c1ee5608c1b9bb1",
    ),
    (
        "SCH_3101290_31100_13006",
        "solidworks-3101290-31100-13006-r2",
        "bee8630028173b14a84497976ff74082bbb9c8bc8af1ae71059c389968420c5b",
    ),
    (
        "SCH_3401254_34101_13006",
        "solidworks-3401254-34101-13006-r2",
        "c702f7a475e4e727791463e24fa7673d289f77aab0768e01bd61b25ce960b059",
    ),
    (
        "SCH_3501210_35102_13006",
        "solidworks-3501210-35102-13006-r2",
        "9673a3534ce3df39916de4d9016403861526d0cc66a3887ae4dcf41b2a12d689",
    ),
    (
        "SCH_3501256_35102_13006",
        "solidworks-3501256-35102-13006-r2",
        "2b66c69af0f79c8a2f7558696b125a994a86026e452d8fc2a7d6577b4fb5d9d9",
    ),
    (
        "SCH_3601228_36001_13006",
        "solidworks-3601228-36001-13006-r2",
        "71d7a26b5ee2ce9d857449556f91757cc943e8ea8aedceb82c150f706585dc7c",
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
        "solidworks-sch37102-13006-r3"
    );
    assert_eq!(profile.metadata().revision, 3);
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
        assert_eq!(profile.metadata().revision, 2);
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
