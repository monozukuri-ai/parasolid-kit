#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, ErrorKind, SchemaKey, SchemaProvider, SchemaTypeLookup,
    parse_xb, parse_xt, schema::profiles::icad_legacy_13006,
};

#[test]
fn exact_profiles_and_canonical_hashes() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let profiles = icad_legacy_13006()?;
    assert_eq!(profiles.len(), 5);
    for (profile, (types, fields)) in
        profiles
            .iter()
            .zip([(31, 252), (34, 292), (31, 252), (39, 305), (31, 252)])
    {
        let key = profile.accepted_schema_keys().next().ok_or("key")?;
        let provider = registry.provider_for_key(key).ok_or("provider")?;
        assert_eq!(
            profile.metadata().profile_sha256,
            support::profile_hash(profile)?
        );
        assert_eq!(profile.definitions().len(), types);
        assert_eq!(
            profile.definitions().map(|d| d.fields.len()).sum::<usize>(),
            fields
        );
        assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
        for kind in [55, 57, 58, 60, 67, 69, 110, 138] {
            assert_eq!(
                provider.lookup_type("13006", kind),
                SchemaTypeLookup::Unknown
            );
        }
        for component in 1..=3 {
            let mut parts = key.raw().split('_').map(str::to_owned).collect::<Vec<_>>();
            parts[component].push('9');
            let nearby = SchemaKey::parse(&parts.join("_"))?;
            assert!(!provider.supports_schema_key(&nearby));
            assert!(registry.provider_for_key(&nearby).is_none());
        }
    }
    Ok(())
}

#[test]
fn extra_membership_is_scoped_to_observed_keys() -> support::Result<()> {
    for profile in icad_legacy_13006()? {
        let key = profile.accepted_schema_keys().next().ok_or("key")?;
        for kind in [45, 56, 59, 68, 127, 128, 134, 135, 136, 137] {
            let covered = match key.raw() {
                "SCH_1500245_15003_13006" => [56, 59, 68].contains(&kind),
                "SCH_1700256_16100_13006" => ![56, 59].contains(&kind),
                _ => false,
            };
            assert_eq!(profile.definition(kind).is_some(), covered);
        }
    }
    Ok(())
}

#[test]
fn independent_point_values_roles_truncation_and_unknown_membership() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    for profile in icad_legacy_13006()? {
        let key = profile.accepted_schema_keys().next().ok_or("key")?;
        let provider = registry.provider_for_key(key).ok_or("provider")?;
        let mut binary = support::xb_header(key.raw(), 0)?;
        binary.extend([0, 29, 255]);
        binary.extend(support::pointer(7)?);
        binary.extend(123_i32.to_be_bytes());
        binary.extend(support::pointer(0)?.repeat(4));
        for value in [13.25_f64, -17.5, 23.75] {
            binary.extend(value.to_be_bytes());
        }
        support::terminate(&mut binary);
        let mut text = support::xt_header(key.raw(), 0);
        text.extend(b"29 255 7 123 0 0 0 0 13.25 -17.5 23.75 1 0");
        let a = parse_xb(&binary, &provider, DocumentLimits::default())?;
        let b = parse_xt(&text, &provider, DocumentLimits::default())?;
        for (x, y) in a.nodes[0].fields.iter().zip(&b.nodes[0].fields) {
            assert_eq!(x.values, y.values);
        }
        assert_eq!(
            a.nodes[0].fields[5].values,
            [parasolid_core::FieldValue::Vector([
                Some(13.25),
                Some(-17.5),
                Some(23.75),
            ])]
        );
        assert_eq!(support::encode_document_nodes(key.raw(), &a.nodes)?, binary);
        // The role gate passes; this deliberately isolated point has no body.
        assert_eq!(
            parasolid_core::brep::map_xb_brep(&a)
                .err()
                .ok_or("body")?
                .kind(),
            ErrorKind::MissingBrepBody
        );
        for end in 0..binary.len() {
            assert!(parse_xb(&binary[..end], &provider, DocumentLimits::default()).is_err());
        }
        let mut unknown = support::xb_header(key.raw(), 0)?;
        unknown.extend([0, 60, 255]);
        let error = parse_xb(&unknown, &provider, DocumentLimits::default())
            .err()
            .ok_or("unknown type")?;
        assert_eq!(error.kind(), ErrorKind::UnknownBaseSchemaType);
    }
    Ok(())
}
