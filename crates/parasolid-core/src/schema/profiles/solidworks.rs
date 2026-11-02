//! Exact `SolidWorks` partition subsets; deltas remain unsupported.
//!
//! Every observed `SolidWorks` key carries the `13006` base suffix, so all
//! profiles share one reviewed declaration set: the 13006 subset with the
//! `SP_CURVE` and B-spline surface layouts, `WORLD`, the reviewed `TORUS`,
//! blend, offset, swept and spun surfaces, and the assembly structure
//! (`ASSEMBLY`, `INSTANCE`, `TRANSFORM`) seen in a `SolidWorks` 2020 assembly
//! export. Version differences arrive as each stream's embedded edits; a
//! nearby key is never selected.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str =
    "3662056f554683d81869f7144eaa797a8b7ea0796b66a561cf295f372111e929";

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
        sha256: "bbba6676a2bee41fb369fe4dca51f1b700a657996aa6f0cd6adfee1e19df5563",
    },
    ProfileSpec {
        key: "SCH_2800191_28002_13006",
        id: "solidworks-2800191-28002-13006-r1",
        revision: 1,
        sha256: "417f6f1f870b3fe7386535c4409a99061428eb3da11bbbdccc5b75ec69475abb",
    },
    ProfileSpec {
        key: "SCH_2800209_28002_13006",
        id: "solidworks-2800209-28002-13006-r1",
        revision: 1,
        sha256: "bc2163693dffb4270bd06c9edd9d834410c27d1211339c19292b97f9f445048c",
    },
    ProfileSpec {
        key: "SCH_2900166_28101_13006",
        id: "solidworks-2900166-28101-13006-r1",
        revision: 1,
        sha256: "7de8374557028af2fc7f017362e61879b35b31dcb4acfc165d42aeea4abe535f",
    },
    ProfileSpec {
        key: "SCH_2900190_28101_13006",
        id: "solidworks-2900190-28101-13006-r1",
        revision: 1,
        sha256: "77cb32a1ae557a3a2566ceac1be1ce22294dd428c2d811ad402695d437bb48e8",
    },
    ProfileSpec {
        key: "SCH_2900195_28101_13006",
        id: "solidworks-2900195-28101-13006-r1",
        revision: 1,
        sha256: "6359eb168a9562e96ffa45cf749a19cd98a7749e5f4f99dd5a5c549a9521afec",
    },
    ProfileSpec {
        key: "SCH_3000226_30000_13006",
        id: "solidworks-3000226-30000-13006-r1",
        revision: 1,
        sha256: "405a4a8bcf11eb533f9cd34617c9d2246298793701fe9dbb5df2bc348c63d0ac",
    },
    ProfileSpec {
        key: "SCH_3000298_30000_13006",
        id: "solidworks-3000298-30000-13006-r1",
        revision: 1,
        sha256: "cde79502920b95e8af30b25eeeef83c7fb747eeb2801dba53ccb3d4d99c0c22f",
    },
    ProfileSpec {
        key: "SCH_3100203_31001_13006",
        id: "solidworks-3100203-31001-13006-r1",
        revision: 1,
        sha256: "cbc193e45f9e8bf6d0c2bcac9e1ca355641dc03072db7de4c6d069be356b648f",
    },
    ProfileSpec {
        key: "SCH_3101290_31100_13006",
        id: "solidworks-3101290-31100-13006-r1",
        revision: 1,
        sha256: "4ae0a021932e228b45fd12fdb798d19c30559e5a8ca8d483812cf234db81547a",
    },
    ProfileSpec {
        key: "SCH_3401254_34101_13006",
        id: "solidworks-3401254-34101-13006-r1",
        revision: 1,
        sha256: "94f2e2471cbff5cf7ae07798b86e4c4d999b9cfe4cd38460e53a41b3d2732a42",
    },
    ProfileSpec {
        key: "SCH_3501210_35102_13006",
        id: "solidworks-3501210-35102-13006-r1",
        revision: 1,
        sha256: "cffec6bfa46c75b6ccacf2551b73376fcba98e593fb50e611e81b2dd06ebc75f",
    },
    ProfileSpec {
        key: "SCH_3501256_35102_13006",
        id: "solidworks-3501256-35102-13006-r1",
        revision: 1,
        sha256: "f6e593910d16624cec2562bdf6913c85bd5533204380f8694fcd5adb3cd96308",
    },
    ProfileSpec {
        key: "SCH_3601228_36001_13006",
        id: "solidworks-3601228-36001-13006-r1",
        revision: 1,
        sha256: "2de76830bea2f0e7fe32030bd8c0105164283b5420c64bc471ce7b225ab78679",
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
    definitions.extend(super::assembly_definitions());
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
