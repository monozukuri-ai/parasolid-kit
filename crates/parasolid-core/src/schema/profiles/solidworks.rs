//! Exact `SolidWorks` partition subsets; deltas remain unsupported.
//!
//! Every observed `SolidWorks` key carries the `13006` base suffix, so all
//! profiles share one reviewed declaration set: the 13006 subset with the
//! `SP_CURVE` and B-spline surface layouts, `WORLD`, and the reviewed `TORUS`,
//! blend, offset, swept and spun surfaces. Version differences arrive as each
//! stream's embedded edits; a nearby key is never selected.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str =
    "e019ad9eb369789e1c0a64704b0a3beaae55e9e7e254e7bf08538a737d4bc83c";

/// Construct the exact `SolidWorks` 2026 partition subset.
///
/// Deltas use the same header key but contain unsupported update records.
/// This profile does not establish a model's final saved configuration state.
///
/// # Errors
/// Returns a profile error for inconsistent compiled definitions.
pub fn solidworks_sch37102_13006() -> Result<BuiltinSchemaProfile, ParseError> {
    BuiltinSchemaProfile::new_embedded(
        BuiltinProfileMetadata {
            profile_id: "solidworks-sch37102-13006-r2".into(),
            revision: 2,
            provider_schema: "13006".into(),
            producer_scope: "SolidWorks 2026 partition".into(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: PROFILE_SHA256.into(),
        },
        vec!["SCH_3701229_37102_13006".into()],
        solidworks_definitions(),
        vec![],
        vec![204],
    )
}

pub(crate) struct ProfileSpec {
    pub key: &'static str,
    pub id: &'static str,
    pub revision: u32,
    pub sha256: &'static str,
}

/// Earlier `SolidWorks` keys observed in saved Parts, oldest first.
pub(crate) const PROFILES: &[ProfileSpec] = &[
    ProfileSpec {
        key: "SCH_2700199_26105_13006",
        id: "solidworks-2700199-26105-13006-r1",
        revision: 1,
        sha256: "65531d74c0e96e760414414a1df7065affa26c316f918617307158c491aa27d8",
    },
    ProfileSpec {
        key: "SCH_2800191_28002_13006",
        id: "solidworks-2800191-28002-13006-r1",
        revision: 1,
        sha256: "24524d4cca8bfe316cdf936de6f440dc83199384981132308603e792e662fe97",
    },
    ProfileSpec {
        key: "SCH_2800209_28002_13006",
        id: "solidworks-2800209-28002-13006-r1",
        revision: 1,
        sha256: "e65c8ddf1bf2876923b6e12f7b284750f2518a0f799d7696d10c74323a8cfc76",
    },
    ProfileSpec {
        key: "SCH_2900166_28101_13006",
        id: "solidworks-2900166-28101-13006-r1",
        revision: 1,
        sha256: "728d84812ee3cc63072085effa633b320ec4e41d3bdb1dafda3ff0d0be91655c",
    },
    ProfileSpec {
        key: "SCH_2900190_28101_13006",
        id: "solidworks-2900190-28101-13006-r1",
        revision: 1,
        sha256: "789a8804c22fbf038d9bcbedd8ce02d16731c9840e049b3b13bebf6951a9b250",
    },
    ProfileSpec {
        key: "SCH_2900195_28101_13006",
        id: "solidworks-2900195-28101-13006-r1",
        revision: 1,
        sha256: "f8cc88a32e3af19961aec053b1d341a7bc9934a780d63945e91908dca2ba95fb",
    },
    ProfileSpec {
        key: "SCH_3000226_30000_13006",
        id: "solidworks-3000226-30000-13006-r1",
        revision: 1,
        sha256: "14354ebf3830d1141ed4c61e3a9ce99f78f5216c381aa994f99de067903068a8",
    },
    ProfileSpec {
        key: "SCH_3000298_30000_13006",
        id: "solidworks-3000298-30000-13006-r1",
        revision: 1,
        sha256: "0d3a628fb09fb47a23701c2c99159bbd8d7c1bb0f96a2b33fa48cd4f4f21f8fa",
    },
    ProfileSpec {
        key: "SCH_3100203_31001_13006",
        id: "solidworks-3100203-31001-13006-r1",
        revision: 1,
        sha256: "314b947f7a8cebfb6ed382206065594ce6d7acc79a1d78765dac8dccf56a26f7",
    },
    ProfileSpec {
        key: "SCH_3101290_31100_13006",
        id: "solidworks-3101290-31100-13006-r1",
        revision: 1,
        sha256: "4a6bbe2db4b7b605447cbe5b13b79ff2fd215b182ed68b7e79a58b8a525e553d",
    },
    ProfileSpec {
        key: "SCH_3401254_34101_13006",
        id: "solidworks-3401254-34101-13006-r1",
        revision: 1,
        sha256: "aa746d6cbea446ca37593e5cdbc9b7ff9fbf5b0bd4096152823740a5f65191da",
    },
    ProfileSpec {
        key: "SCH_3501210_35102_13006",
        id: "solidworks-3501210-35102-13006-r1",
        revision: 1,
        sha256: "a99c8793ea67bebaf50d2dd7b5e61f842cb5fce6c563840df94a2182bdf18dc0",
    },
    ProfileSpec {
        key: "SCH_3501256_35102_13006",
        id: "solidworks-3501256-35102-13006-r1",
        revision: 1,
        sha256: "b4c382da97bead88b8065fa913a72c65d0e1e0f6999e01894fa9ecee2bf21ef6",
    },
    ProfileSpec {
        key: "SCH_3601228_36001_13006",
        id: "solidworks-3601228-36001-13006-r1",
        revision: 1,
        sha256: "1117b129bcb099219a8c61dc813b0a06cdf76daa85d3f567cf37dc0a156593f2",
    },
];

/// The 13006 subset with `SP_CURVE` and B-spline surface layouts, `WORLD`, and
/// the reviewed `TORUS` (54), `BLENDED_EDGE` (56), `BLEND_BOUND` (59),
/// `OFFSET_SURF` (60), `SWEPT_SURF` (67) and `SPUN_SURF` (68) surfaces.
pub(crate) fn solidworks_definitions() -> Vec<TypeDefinition> {
    let mut definitions = super::sch13006_definitions();
    definitions.extend(super::sch13006_sp_curve_definitions());
    definitions.extend(super::sch13006_bspline_surface_definitions());
    definitions.push(world());
    definitions.extend(super::icad_legacy::reviewed_extra_definitions());
    definitions.push(swept_surface());
    definitions
}

/// Construct the earlier `SolidWorks` partition profiles, one per exact key.
///
/// Deltas share each key but contain unsupported update records. These
/// profiles do not establish a model's final saved configuration state.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn solidworks_partitions_13006() -> Result<Vec<BuiltinSchemaProfile>, ParseError> {
    PROFILES
        .iter()
        .map(|spec| {
            BuiltinSchemaProfile::new_embedded(
                BuiltinProfileMetadata {
                    profile_id: spec.id.into(),
                    revision: spec.revision,
                    provider_schema: "13006".into(),
                    producer_scope: "SolidWorks partition".into(),
                    coverage: BuiltinProfileCoverage::VerifiedSubset,
                    evidence_manifest_sha256: None,
                    profile_sha256: spec.sha256.into(),
                },
                vec![spec.key.into()],
                solidworks_definitions(),
                vec![],
                vec![204],
            )
        })
        .collect()
}

/// Public XT Format Reference, April 2008, printed p. 73: `SWEPT_SURF` (67).
/// The common surface header is followed by the section curve, the sweep
/// direction and the scale; the layout is read to the stream terminator in
/// every sampled partition that uses it.
fn swept_surface() -> TypeDefinition {
    use FieldType::{Character as C, Double as F, Integer as D, PointerIndex as P, Vector as V};
    let fields = [
        ("local_id", D, 0),
        ("annotations", P, 1019),
        ("owner_ref", P, 1007),
        ("next_surface", P, 1006),
        ("previous_surface", P, 1006),
        ("indirect_owner", P, 141),
        ("orientation", C, 0),
        ("section_curve", P, 1008),
        ("sweep_direction", V, 0),
        ("scale", F, 0),
    ]
    .into_iter()
    .map(|(name, field_type, pointer_class)| FieldDefinition {
        name: name.into(),
        field_type,
        pointer_class,
        element_count: 0,
        transmitted: true,
    })
    .collect();
    TypeDefinition::from_fields(
        67,
        "sch13006_type_67",
        "Reviewed swept surface",
        fields,
        SchemaSource::Base,
    )
}

/// Public XT Format Reference, April 2008, pp. 82-83 and 118: WORLD (101).
/// Eleven base fields; indexed-transmit fields in the modern input are
/// embedded additions, not guessed members of the 13006 base.
fn world() -> TypeDefinition {
    use FieldType::{Integer as D, Logical as L, PointerIndex as P};
    let fields = [
        ("assembly_head", P, 10),
        ("attribute_head", P, 81),
        ("body_head", P, 12),
        ("transform_head", P, 100),
        ("surface_head", P, 1006),
        ("curve_head", P, 1008),
        ("point_head", P, 29),
        ("alive", L, 0),
        ("attribute_definition_head", P, 80),
        ("highest_id", D, 0),
        ("current_id", D, 0),
    ]
    .into_iter()
    .map(|(name, field_type, pointer_class)| FieldDefinition {
        name: name.into(),
        field_type,
        pointer_class,
        element_count: 0,
        transmitted: true,
    })
    .collect();
    TypeDefinition::from_fields(
        101,
        "sch13006_type_101",
        "Reviewed WORLD base layout",
        fields,
        SchemaSource::Base,
    )
}
