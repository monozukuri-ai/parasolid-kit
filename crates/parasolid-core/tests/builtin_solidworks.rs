#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, DocumentLimits, FieldValue, SchemaKey,
    SchemaProvider, SchemaTypeLookup, parse_xb, parse_xt,
    schema::profiles::{solidworks_partitions_13006, solidworks_sch37102_13006},
};

const KEY: &str = "SCH_3701229_37102_13006";
/// The shared `SolidWorks` declaration set: 50 types and 448 field groups.
const TYPES: usize = 50;
const FIELD_GROUPS: usize = 448;
/// Base types outside every `SolidWorks` profile, including the delta update
/// records (3 and 4) and layouts without local evidence (163, 184).
const UNKNOWN_TYPES: [u16; 4] = [3, 4, 163, 184];

/// Earlier keys, their profile ids and canonical hashes.
const EARLIER: &[(&str, &str, &str)] = &[
    (
        "SCH_2700199_26105_13006",
        "solidworks-2700199-26105-13006-r1",
        "bbba6676a2bee41fb369fe4dca51f1b700a657996aa6f0cd6adfee1e19df5563",
    ),
    (
        "SCH_2800191_28002_13006",
        "solidworks-2800191-28002-13006-r1",
        "417f6f1f870b3fe7386535c4409a99061428eb3da11bbbdccc5b75ec69475abb",
    ),
    (
        "SCH_2800209_28002_13006",
        "solidworks-2800209-28002-13006-r1",
        "bc2163693dffb4270bd06c9edd9d834410c27d1211339c19292b97f9f445048c",
    ),
    (
        "SCH_2900166_28101_13006",
        "solidworks-2900166-28101-13006-r1",
        "7de8374557028af2fc7f017362e61879b35b31dcb4acfc165d42aeea4abe535f",
    ),
    (
        "SCH_2900190_28101_13006",
        "solidworks-2900190-28101-13006-r1",
        "77cb32a1ae557a3a2566ceac1be1ce22294dd428c2d811ad402695d437bb48e8",
    ),
    (
        "SCH_2900195_28101_13006",
        "solidworks-2900195-28101-13006-r1",
        "6359eb168a9562e96ffa45cf749a19cd98a7749e5f4f99dd5a5c549a9521afec",
    ),
    (
        "SCH_3000226_30000_13006",
        "solidworks-3000226-30000-13006-r1",
        "405a4a8bcf11eb533f9cd34617c9d2246298793701fe9dbb5df2bc348c63d0ac",
    ),
    (
        "SCH_3000298_30000_13006",
        "solidworks-3000298-30000-13006-r1",
        "cde79502920b95e8af30b25eeeef83c7fb747eeb2801dba53ccb3d4d99c0c22f",
    ),
    (
        "SCH_3100203_31001_13006",
        "solidworks-3100203-31001-13006-r1",
        "cbc193e45f9e8bf6d0c2bcac9e1ca355641dc03072db7de4c6d069be356b648f",
    ),
    (
        "SCH_3101290_31100_13006",
        "solidworks-3101290-31100-13006-r1",
        "4ae0a021932e228b45fd12fdb798d19c30559e5a8ca8d483812cf234db81547a",
    ),
    (
        "SCH_3401254_34101_13006",
        "solidworks-3401254-34101-13006-r1",
        "94f2e2471cbff5cf7ae07798b86e4c4d999b9cfe4cd38460e53a41b3d2732a42",
    ),
    (
        "SCH_3501210_35102_13006",
        "solidworks-3501210-35102-13006-r1",
        "cffec6bfa46c75b6ccacf2551b73376fcba98e593fb50e611e81b2dd06ebc75f",
    ),
    (
        "SCH_3501256_35102_13006",
        "solidworks-3501256-35102-13006-r1",
        "f6e593910d16624cec2562bdf6913c85bd5533204380f8694fcd5adb3cd96308",
    ),
    (
        "SCH_3601228_36001_13006",
        "solidworks-3601228-36001-13006-r1",
        "2de76830bea2f0e7fe32030bd8c0105164283b5420c64bc471ce7b225ab78679",
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
