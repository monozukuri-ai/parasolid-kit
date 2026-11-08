use parasolid_core::{BuiltinProfileCoverage, BuiltinProfileRegistry, SchemaKey};

#[test]
fn topsolid_exact_profiles_keep_their_identity_and_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = BuiltinProfileRegistry::compiled()?;
    for (raw_key, profile_id) in [
        (
            "SCH_2701192_26105_13006",
            "topsolid-v6-sch-2701192-26105-13006-r1",
        ),
        (
            "SCH_3300259_32001_13006",
            "topsolid-v6-sch-3300259-32001-13006-r1",
        ),
        (
            "SCH_3300261_32001_13006",
            "topsolid-v6-sch-3300261-32001-13006-r1",
        ),
        (
            "SCH_3700192_36001_13006",
            "topsolid-v6-sch-3700192-36001-13006-r1",
        ),
        (
            "SCH_3600188_36001_13006",
            "topsolid-v6-sch-3600188-36001-13006-r1",
        ),
    ] {
        let key = SchemaKey::parse(raw_key)?;
        let provider = registry
            .provider_for_key(&key)
            .ok_or("missing exact TopSolid profile")?;
        assert_eq!(provider.selected_schema_key(), key.raw());
        assert_eq!(provider.profile().metadata().profile_id, profile_id);
        assert_eq!(
            provider.profile().metadata().coverage,
            BuiltinProfileCoverage::VerifiedSubset
        );
    }
    Ok(())
}
