//! Exact `SolidWorks` partition subsets; deltas remain unsupported.
//!
//! Every observed `SolidWorks` key carries the `13006` base suffix, so all
//! profiles share one reviewed declaration set: the 13006 subset with the
//! `SP_CURVE` and B-spline surface layouts, the attribute value nodes of the
//! public reference, `WORLD`, the reviewed `TORUS`, blend, offset, swept and
//! spun surfaces, and the assembly structure (`ASSEMBLY`, `INSTANCE`,
//! `TRANSFORM`) seen in a `SolidWorks` 2020 assembly export, the helical
//! surface and curve forms of B-surfaces and B-curves (163 and 184) and the
//! attribute field names (`FIELD_NAMES`, 99). Version differences arrive as
//! each stream's embedded edits; a nearby key is never selected.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str =
    "aa0061b69e33343ad26aeb35a4598c071643d26f541eb4cca5e810e0e28b7a33";

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
            profile_id: "solidworks-sch37102-13006-r4".into(),
            revision: 4,
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
        id: "solidworks-2700199-26105-13006-r3",
        revision: 3,
        sha256: "71f1eb426abd13984739aa01009c237be5a176140c00776e4ae5345711a92928",
    },
    ProfileSpec {
        key: "SCH_2800191_28002_13006",
        id: "solidworks-2800191-28002-13006-r3",
        revision: 3,
        sha256: "8bedc5c55d98b876012afc83cecf32e2af883ab2f1b85488d3eb1b68be5c53ba",
    },
    ProfileSpec {
        key: "SCH_2800209_28002_13006",
        id: "solidworks-2800209-28002-13006-r3",
        revision: 3,
        sha256: "8cb6615798a8afe525bdd2dfb1f153f358cf702bbbf490f04c14a42a0e760090",
    },
    ProfileSpec {
        key: "SCH_2900166_28101_13006",
        id: "solidworks-2900166-28101-13006-r3",
        revision: 3,
        sha256: "7096617ce7a03febd577460c60c3da38ef8c0afe85a79da28fa34fb3f3d16253",
    },
    ProfileSpec {
        key: "SCH_2900190_28101_13006",
        id: "solidworks-2900190-28101-13006-r3",
        revision: 3,
        sha256: "5e3f76ffd1a090c6a0a23fd7d1b6de0541e8aad86cd5466721581380da62f5f3",
    },
    ProfileSpec {
        key: "SCH_2900195_28101_13006",
        id: "solidworks-2900195-28101-13006-r3",
        revision: 3,
        sha256: "df4501986282d4682094cf8dac682764f9ab75c059a060d7ef1368cca3334b0e",
    },
    ProfileSpec {
        key: "SCH_3000226_30000_13006",
        id: "solidworks-3000226-30000-13006-r3",
        revision: 3,
        sha256: "9ac6a98ac91cb2dbc66fe8d10a89c7a750bb379b24189b86f9403e7249016f12",
    },
    ProfileSpec {
        key: "SCH_3000298_30000_13006",
        id: "solidworks-3000298-30000-13006-r3",
        revision: 3,
        sha256: "5a1e21f7bca73720ffd3dcb77cdf8c9405d662f2807123caa8d58974c2ee1822",
    },
    ProfileSpec {
        key: "SCH_3100203_31001_13006",
        id: "solidworks-3100203-31001-13006-r3",
        revision: 3,
        sha256: "68977cb927733ad30279b6566874dc16cc4f7ef2858a9a5f395b233dcce65851",
    },
    ProfileSpec {
        key: "SCH_3101290_31100_13006",
        id: "solidworks-3101290-31100-13006-r3",
        revision: 3,
        sha256: "0477947dd1a5b6fc11baaa344637e44d44af0bec1ae82a1de8dd956272b1c5f4",
    },
    ProfileSpec {
        key: "SCH_3401254_34101_13006",
        id: "solidworks-3401254-34101-13006-r3",
        revision: 3,
        sha256: "e7206570f57a650be545095dca12c517b82106dc55540a613e3002022e457867",
    },
    ProfileSpec {
        key: "SCH_3501210_35102_13006",
        id: "solidworks-3501210-35102-13006-r3",
        revision: 3,
        sha256: "463fd89c390ef3be8a4d2ee8f790008f00c8146b0d6690d89fa3c0fa66b124bc",
    },
    ProfileSpec {
        key: "SCH_3501256_35102_13006",
        id: "solidworks-3501256-35102-13006-r3",
        revision: 3,
        sha256: "cd3dc807919d5f40ae8d8b39eb2889787e88689c727c14ee903b4a02508757ec",
    },
    ProfileSpec {
        key: "SCH_3601228_36001_13006",
        id: "solidworks-3601228-36001-13006-r3",
        revision: 3,
        sha256: "cf6e41ea2c801aa42338a6fb7710fc2510831ebadcc521087b2e9ce0bee38b5d",
    },
];

/// The 13006 subset with `SP_CURVE` and B-spline surface layouts, the point,
/// vector, axis, tag and direction value nodes, `WORLD`, the reviewed
/// `TORUS` (54), `BLENDED_EDGE` (56), `BLEND_BOUND` (59), `OFFSET_SURF` (60),
/// `SWEPT_SURF` (67) and `SPUN_SURF` (68) surfaces, the assembly structure,
/// the helical surface and curve forms (163, 184) and `FIELD_NAMES` (99).
pub(crate) fn solidworks_definitions() -> Vec<TypeDefinition> {
    let mut definitions = super::sch13006_definitions();
    definitions.extend(super::sch13006_sp_curve_definitions());
    definitions.extend(super::sch13006_bspline_surface_definitions());
    definitions.extend(super::sch13006_value_node_definitions());
    definitions.push(world());
    definitions.extend(super::icad_legacy::reviewed_extra_definitions());
    definitions.push(swept_surface());
    definitions.extend(super::assembly_definitions());
    definitions.push(helix_surface_form());
    definitions.push(helix_curve_form());
    definitions.push(field_names());
    definitions
}

/// `HELIX_SU_FORM` layout of the public XT Format Reference, April 2008,
/// printed p. 72: the optional analytic form of a B-surface, referenced by
/// `SURFACE_DATA` (125) with analytic form type `'H'`. Streams write it as node
/// type 163 (the reference's table, printed p. 118, numbers it 184 and gives
/// 163 to `HELIX_CU_FORM`) with the reference's seven field groups: the axis,
/// the hand, the turns as one interval, the pitch, the gap and the tolerance.
/// Embedded edits address these seven base fields.
fn helix_surface_form() -> TypeDefinition {
    use FieldType::{Character as C, Double as F, Interval as I, Vector as V};
    let fields = [
        ("axis_point", V),
        ("axis_direction", V),
        ("hand", C),
        ("turns", I),
        ("pitch", F),
        ("gap", F),
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
        "Reviewed helical surface form",
        fields,
        SchemaSource::Base,
    )
}

/// `HELIX_CU_FORM` layout of the public XT Format Reference, April 2008,
/// printed pp. 43-44: the optional analytic form of a B-curve, referenced by
/// `CURVE_DATA` (135). Streams write it as node type 184 (the reference's
/// table, printed p. 118, gives that number to `HELIX_SU_FORM`) with the
/// reference's seven field groups, the turns as one interval.
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
        184,
        "sch13006_type_184",
        "Reviewed helical curve form",
        fields,
        SchemaSource::Base,
    )
}

/// Public XT Format Reference, April 2008, printed pp. 102-103 and 118-119:
/// `FIELD_NAMES` (99), the names of an attribute definition's fields, one
/// `CHAR_VALUES` or `UNICODE_VALUES` pointer (class `FIELD_NAME`, 1037) per
/// field.
fn field_names() -> TypeDefinition {
    TypeDefinition::from_fields(
        99,
        "sch13006_type_99",
        "Reviewed attribute field names",
        vec![FieldDefinition {
            name: "names".into(),
            field_type: FieldType::PointerIndex,
            pointer_class: 1037,
            element_count: 1,
            transmitted: true,
        }],
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
