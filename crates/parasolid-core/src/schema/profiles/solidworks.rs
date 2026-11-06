//! Exact `SolidWorks` partition subsets; deltas remain unsupported.
//!
//! Every observed `SolidWorks` key carries the `13006` base suffix, so all
//! profiles share one reviewed declaration set: the 13006 subset with the
//! `SP_CURVE` and B-spline surface layouts, the attribute value nodes of the
//! public reference, `WORLD`, the reviewed `TORUS`, blend, offset, swept and
//! spun surfaces, and the assembly structure (`ASSEMBLY`, `INSTANCE`,
//! `TRANSFORM`) seen in a `SolidWorks` 2020 assembly export, and the
//! reference's helical curve descriptor (`HELIX_CU_FORM`, 163). Version
//! differences arrive as each stream's embedded edits; a nearby key is never
//! selected.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str =
    "7300096c034365663a13f40c96a919d19b73d25b4c957f9dbb0e472c0a381730";

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
            profile_id: "solidworks-sch37102-13006-r3".into(),
            revision: 3,
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
        id: "solidworks-2700199-26105-13006-r2",
        revision: 2,
        sha256: "edab51092b3b8d1e0f137e6e3d7f666348a433b2bf17a932e8b708886e6b4331",
    },
    ProfileSpec {
        key: "SCH_2800191_28002_13006",
        id: "solidworks-2800191-28002-13006-r2",
        revision: 2,
        sha256: "388cf98e9768b041ff7dd572df1e63f83c41afc36eaa635ccf61fdc272214a3e",
    },
    ProfileSpec {
        key: "SCH_2800209_28002_13006",
        id: "solidworks-2800209-28002-13006-r2",
        revision: 2,
        sha256: "162b6cdd51c8b6ac6bcbddf363c2b9cba98262d4e08365d31576b30a3653fe3e",
    },
    ProfileSpec {
        key: "SCH_2900166_28101_13006",
        id: "solidworks-2900166-28101-13006-r2",
        revision: 2,
        sha256: "f34b810890bf48d501fd941a5e2793741df03915b93292ce3abb36d6b16a6bff",
    },
    ProfileSpec {
        key: "SCH_2900190_28101_13006",
        id: "solidworks-2900190-28101-13006-r2",
        revision: 2,
        sha256: "6625faaa9d5afae86fa6f9058a6ae36a2bc97852f6d66dd8d68b3c899a868f7f",
    },
    ProfileSpec {
        key: "SCH_2900195_28101_13006",
        id: "solidworks-2900195-28101-13006-r2",
        revision: 2,
        sha256: "3ed9e73e24121d843d2393b42462c5ae633c8054ec8f66b54ebddc2ea4c8a28b",
    },
    ProfileSpec {
        key: "SCH_3000226_30000_13006",
        id: "solidworks-3000226-30000-13006-r2",
        revision: 2,
        sha256: "734a40413d7a1a97a2344e5d6379255b299365ea50361ff0286c7f90fcd956cc",
    },
    ProfileSpec {
        key: "SCH_3000298_30000_13006",
        id: "solidworks-3000298-30000-13006-r2",
        revision: 2,
        sha256: "c9d5c679dba4855d478de10e6c0cd88167113bbdc0a9553b259bab261e490cfd",
    },
    ProfileSpec {
        key: "SCH_3100203_31001_13006",
        id: "solidworks-3100203-31001-13006-r2",
        revision: 2,
        sha256: "b209ba14673b1fb93d43bb4d74aab652e63fba9d9bbeb88e4c1ee5608c1b9bb1",
    },
    ProfileSpec {
        key: "SCH_3101290_31100_13006",
        id: "solidworks-3101290-31100-13006-r2",
        revision: 2,
        sha256: "bee8630028173b14a84497976ff74082bbb9c8bc8af1ae71059c389968420c5b",
    },
    ProfileSpec {
        key: "SCH_3401254_34101_13006",
        id: "solidworks-3401254-34101-13006-r2",
        revision: 2,
        sha256: "c702f7a475e4e727791463e24fa7673d289f77aab0768e01bd61b25ce960b059",
    },
    ProfileSpec {
        key: "SCH_3501210_35102_13006",
        id: "solidworks-3501210-35102-13006-r2",
        revision: 2,
        sha256: "9673a3534ce3df39916de4d9016403861526d0cc66a3887ae4dcf41b2a12d689",
    },
    ProfileSpec {
        key: "SCH_3501256_35102_13006",
        id: "solidworks-3501256-35102-13006-r2",
        revision: 2,
        sha256: "2b66c69af0f79c8a2f7558696b125a994a86026e452d8fc2a7d6577b4fb5d9d9",
    },
    ProfileSpec {
        key: "SCH_3601228_36001_13006",
        id: "solidworks-3601228-36001-13006-r2",
        revision: 2,
        sha256: "71d7a26b5ee2ce9d857449556f91757cc943e8ea8aedceb82c150f706585dc7c",
    },
];

/// The 13006 subset with `SP_CURVE` and B-spline surface layouts, the point,
/// vector, axis, tag and direction value nodes, `WORLD`, and the reviewed
/// `TORUS` (54), `BLENDED_EDGE` (56), `BLEND_BOUND` (59), `OFFSET_SURF` (60),
/// `SWEPT_SURF` (67) and `SPUN_SURF` (68) surfaces.
pub(crate) fn solidworks_definitions() -> Vec<TypeDefinition> {
    let mut definitions = super::sch13006_definitions();
    definitions.extend(super::sch13006_sp_curve_definitions());
    definitions.extend(super::sch13006_bspline_surface_definitions());
    definitions.extend(super::sch13006_value_node_definitions());
    definitions.push(world());
    definitions.extend(super::icad_legacy::reviewed_extra_definitions());
    definitions.push(swept_surface());
    definitions.extend(super::assembly_definitions());
    definitions.push(helix_curve_form());
    definitions
}

/// Public XT Format Reference, April 2008, printed pp. 43-44 and 118.
/// `HELIX_CU_FORM` (163) is the optional analytic descriptor referenced by
/// `CURVE_DATA` (135), not a curve with the common geometry header. In
/// particular, `turns` is one interval field, not two scalar field groups;
/// embedded edits address these seven base fields.
fn helix_curve_form() -> TypeDefinition {
    use FieldType::{Character as C, Double as F, Interval as I, Vector as V};
    let fields = [
        ("axis_point", V),
        ("axis_direction", V),
        ("point", V),
        ("hand", C),
        ("turns", I),
        ("pitch", F),
        ("tolerance", F),
    ]
    .into_iter()
    .map(|(name, field_type)| FieldDefinition {
        name: name.into(),
        field_type,
        pointer_class: 0,
        element_count: 0,
        transmitted: true,
    })
    .collect();
    TypeDefinition::from_fields(
        163,
        "sch13006_type_163",
        "Reviewed helical curve descriptor",
        fields,
        SchemaSource::Base,
    )
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
