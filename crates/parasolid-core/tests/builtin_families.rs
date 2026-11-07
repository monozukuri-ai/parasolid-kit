//! Derived schema families of the compiled registry.
//!
//! A family merges the reviewed declarations of every exact profile of its
//! provider schema: the profiles whose keys share `<effective>[-<base>]` are its
//! members, the other profiles of that provider schema its base members. These
//! checks pin the family list, the member lists, the recomputed digests and the
//! absence of layout disagreements.

#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, DocumentLimits, FamilyMember, SchemaKey,
    SchemaProvider, SchemaProviderResolution, SchemaTypeLookup, parse_xt,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn identities(members: &[FamilyMember]) -> Vec<Value> {
    members
        .iter()
        .map(|member| json!([member.profile_id, member.revision, member.profile_sha256]))
        .collect()
}

fn family_digest(family_id: &str, members: &[FamilyMember], base: &[FamilyMember]) -> String {
    let canonical = json!({
        "base_members": identities(base),
        "family": family_id,
        "members": identities(members),
    });
    serde_json::to_vec(&canonical)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .unwrap_or_default()
}

#[test]
fn every_exact_key_belongs_to_one_derived_family() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    // Seventeen standard families (9003 … 33103) and nineteen embedded
    // families (15003-13006 … 37102-13006) are derived from 57 profiles;
    // five TopSolid exact profiles are excluded from family membership.
    assert_eq!(registry.families().len(), 36);
    let mut covered = 0;
    for profile in registry
        .profiles()
        .filter(|profile| !profile.metadata().producer_scope.starts_with("TopSolid"))
    {
        for key in profile.accepted_schema_keys() {
            let family = registry
                .family_for_key(key)
                .ok_or("every exact key must have a family")?;
            assert_eq!(family.family_id(), Some(key.family_id().as_str()));
            assert!(
                family
                    .members()
                    .iter()
                    .any(|member| member.profile_id == profile.metadata().profile_id)
            );
            // Every member definition survives the merge unless its layout is disputed.
            for definition in profile.definitions() {
                let merged = family.definition(definition.node_type);
                assert!(
                    merged.is_some_and(|merged| merged.fields == definition.fields),
                    "{} type {} is not in family {}",
                    profile.metadata().profile_id,
                    definition.node_type,
                    key.family_id()
                );
            }
            covered += 1;
        }
    }
    assert_eq!(covered, 57);
    assert_eq!(registry.len(), 62);
    Ok(())
}

#[test]
fn families_have_recomputable_digests_and_no_disputed_layouts() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    for family in registry.families() {
        let metadata = family.metadata();
        let family_id = family.family_id().ok_or("family id")?;
        assert_eq!(metadata.profile_id, format!("family-{family_id}"));
        assert_eq!(metadata.revision, 1);
        assert_eq!(metadata.coverage, BuiltinProfileCoverage::FamilyInferred);
        assert_eq!(
            metadata.profile_sha256,
            family_digest(family_id, family.members(), family.base_members())
        );
        for list in [family.members(), family.base_members()] {
            assert!(
                list.windows(2)
                    .all(|pair| pair[0].profile_id < pair[1].profile_id)
            );
        }
        // Every exact profile of the provider schema contributes exactly once.
        let contributors = family.members().len() + family.base_members().len();
        assert_eq!(
            contributors,
            registry
                .profiles()
                .filter(|profile| {
                    profile.metadata().provider_schema == metadata.provider_schema
                        && !profile.metadata().producer_scope.starts_with("TopSolid")
                })
                .count()
        );
        assert_eq!(
            family.conflicting_types().collect::<Vec<_>>(),
            Vec::<u16>::new(),
            "family {family_id} has disputed layouts"
        );
        assert_eq!(family.accepted_schema_keys().len(), 0);
        assert!(registry.profile(&metadata.profile_id).is_some());
        for member in family.members().iter().chain(family.base_members()) {
            let exact = registry.profile(&member.profile_id).ok_or("member")?;
            assert_eq!(exact.metadata().revision, member.revision);
            assert_eq!(exact.metadata().profile_sha256, member.profile_sha256);
        }
    }
    Ok(())
}

#[test]
fn producer_families_merge_the_expected_members() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let members = |family_id: &str| -> support::Result<Vec<String>> {
        Ok(registry
            .profile(&format!("family-{family_id}"))
            .ok_or("family")?
            .members()
            .iter()
            .map(|member| member.profile_id.clone())
            .collect())
    };
    assert_eq!(
        members("35102-13006")?,
        [
            "solidworks-3501210-35102-13006-r2",
            "solidworks-3501256-35102-13006-r2",
        ]
    );
    assert_eq!(
        members("37102-13006")?,
        ["onshape-sch37102-13006-r3", "solidworks-sch37102-13006-r3"]
    );
    assert_eq!(
        members("28002-13006")?,
        [
            "icad-2800188-28002-13006-r1",
            "solidworks-2800191-28002-13006-r2",
            "solidworks-2800209-28002-13006-r2",
        ]
    );
    assert_eq!(members("30000")?, ["onshape-sch30000-r3"]);
    assert_eq!(
        members("13006")?,
        [
            "icad-1300218-13006-r1",
            "icad-1302234-13006-r1",
            "onshape-sch13006-r6",
        ]
    );

    assert_eq!(
        members("25001-13006")?,
        [
            "nx-2600169-25001-13006-r2",
            "parasolid-2600217-25001-13006-r2"
        ]
    );
    // One public NX key each anchors these families; other builds of the two
    // schemas resolve through them.
    assert_eq!(members("30100-13006")?, ["nx-3001232-30100-13006-r2"]);
    assert_eq!(members("34001-13006")?, ["nx-3400176-34001-13006-r2"]);

    // Every embedded family over 13006 takes the reviewed declarations of all
    // 13006 profiles: the SolidWorks swept surface and blends reach the iCAD-only
    // families, and the Onshape 37.1 membership audit (176 absent) applies to all.
    for family_id in ["20000-13006", "19008-13006", "37102-13006", "30100-13006"] {
        let family = registry
            .profile(&format!("family-{family_id}"))
            .ok_or("family")?;
        assert_eq!(family.absent_base_types().collect::<Vec<_>>(), [176, 204]);
        for kind in [
            10, 11, 54, 56, 59, 60, 67, 68, 85, 86, 87, 88, 89, 90, 91, 100, 101,
        ] {
            assert!(
                family.definition(kind).is_some(),
                "{family_id} lacks {kind}"
            );
        }
        // The shared 56 declarations (with ASSEMBLY, INSTANCE, TRANSFORM and
        // the point, vector, axis, tag and direction value nodes) plus GROUP
        // and MEMBER_OF_GROUP from the Parasolid 24 acceptance-test profile.
        assert_eq!(family.definitions().len(), 58);
        assert!(!family.base_members().is_empty());
    }
    // The standard 13006 family likewise gains the embedded profiles' declarations.
    let standard = registry.profile("family-13006").ok_or("family")?;
    assert!(standard.definition(54).is_some() && standard.definition(101).is_some());
    // A standard family of another revision keeps only its own revision's layouts.
    let revision_28101 = registry.profile("family-28101").ok_or("family")?;
    assert!(revision_28101.base_members().is_empty());
    assert!(revision_28101.definition(54).is_none());
    Ok(())
}

#[test]
fn an_exact_profile_is_completed_by_its_family() -> support::Result<()> {
    // SolidWorks 2009 writes SCH_1901315_19008_13006, which the iCAD 19008
    // profile accepts exactly; its blend boundaries (59) are declared only by
    // other 13006 profiles, which the family supplies.
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse("SCH_1901315_19008_13006")?;
    let provider = registry.provider_for_key(&key).ok_or("provider")?;
    assert_eq!(
        provider.profile().metadata().profile_id,
        "icad-1901315-19008-13006-r2"
    );
    assert_eq!(
        provider
            .family()
            .map(|family| family.metadata().profile_id.as_str()),
        Some("family-19008-13006")
    );
    assert!(provider.profile().definition(59).is_none());
    assert!(matches!(
        provider.lookup_type("13006", 59),
        SchemaTypeLookup::Defined(_)
    ));
    assert!(provider.type_definition("13006", 59).is_some());
    assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
    assert!(matches!(
        provider.lookup_type("13006", 163),
        SchemaTypeLookup::Defined(_)
    ));
    // A family provider has no further family to fall back to.
    let other_build = registry
        .provider_for_key(&SchemaKey::parse("SCH_1901316_19008_13006")?)
        .ok_or("family provider")?;
    assert!(other_build.family().is_none());
    assert_eq!(
        other_build.profile().metadata().coverage,
        BuiltinProfileCoverage::FamilyInferred
    );
    Ok(())
}

#[test]
fn every_compiled_exact_profile_has_its_canonical_hash() -> support::Result<()> {
    // The per-profile tests pin most digests; this covers the transmit-file
    // profiles too and prints the recomputed digest of any that drifted.
    let mut mismatches = Vec::new();
    for profile in BuiltinProfileRegistry::compiled()?
        .profiles()
        .filter(|profile| !profile.metadata().producer_scope.starts_with("TopSolid"))
    {
        let digest = support::profile_hash(profile)?;
        if profile.metadata().profile_sha256 != digest {
            mismatches.push(format!("HASH {} {digest}", profile.metadata().profile_id));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));

    // TopSolid profile metadata pins source catalog bytes; these checks pin the
    // canonical hashes of the exact compiled profile declarations instead.
    for (raw_key, expected_hash) in [
        (
            "SCH_2701192_26105_13006",
            "770b1861143157ba89cfc0a102965a45dbd46a1df2bc0b996819de85e19c0c49",
        ),
        (
            "SCH_3300259_32001_13006",
            "d90a16c62e160ea72c0a527c331b8b9c25c1f82419d905edd595d9f4aa9256bc",
        ),
        (
            "SCH_3300261_32001_13006",
            "7cfe6a78742e4b8d91124f00d06ad30566bf6f6ffd2b55ce96112a29a55e9a3e",
        ),
        (
            "SCH_3700192_36001_13006",
            "99296c55918d8662c5b6d8bc7ab9201fd8c2e4d06616d123e58d26e0cdbcf95c",
        ),
        (
            "SCH_3600188_36001_13006",
            "4e6aaaa3b57c1c0964f6fa029562f72547a438eee2a02a9d81fd5f8036266f8b",
        ),
    ] {
        let key = SchemaKey::parse(raw_key)?;
        let profile = BuiltinProfileRegistry::compiled()?
            .profile_for_key(&key)
            .ok_or("TopSolid exact profile")?;
        assert_eq!(support::profile_hash(profile)?, expected_hash, "{raw_key}");
    }
    Ok(())
}

#[test]
fn an_unregistered_solidworks_build_parses_through_its_family() -> support::Result<()> {
    // SolidWorks 2024 exports carry Parasolid build 35.1.251; only the 2024 and
    // 2025 partition builds (210 and 256) have exact profiles.
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse("SCH_3501251_35102_13006")?;
    assert!(registry.profile_for_key(&key).is_none());
    let provider = registry.provider_for_key(&key).ok_or("family provider")?;
    assert_eq!(provider.selected_schema_key(), key.raw());
    assert!(provider.supports_schema_key(&key));
    assert!(!provider.supports_schema_key(&SchemaKey::parse("SCH_3501210_35102_13006")?));
    assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
    assert!(matches!(
        provider.lookup_type("13006", 163),
        SchemaTypeLookup::Defined(_)
    ));

    // A node-free stream still records the family provenance against the requested key.
    let mut bytes = support::xt_header(key.raw(), 0);
    bytes.extend_from_slice(b"1 0 ");
    let document = parse_xt(&bytes, &provider, DocumentLimits::default())?;
    assert!(matches!(
        &document.schema_provider,
        SchemaProviderResolution::Builtin {
            profile_id,
            profile_revision: 1,
            schema_key,
            coverage: BuiltinProfileCoverage::FamilyInferred,
            profile_sha256,
        } if profile_id == "family-35102-13006"
            && schema_key == key.raw()
            && *profile_sha256 == provider.profile().metadata().profile_sha256
    ));
    Ok(())
}
