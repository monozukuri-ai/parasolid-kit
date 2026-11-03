//! Exact `SolidWorks` partition subsets; deltas remain unsupported.
//!
//! Every observed `SolidWorks` key carries the `13006` base suffix, so all
//! profiles share one reviewed declaration set: the 13006 subset with the
//! `SP_CURVE` and B-spline surface layouts, the attribute value nodes of the
//! public reference, `WORLD`, the reviewed `TORUS`, blend, offset, swept and
//! spun surfaces, and the assembly structure (`ASSEMBLY`, `INSTANCE`,
//! `TRANSFORM`) seen in a `SolidWorks` 2020 assembly export. Version
//! differences arrive as each stream's embedded edits; a nearby key is never
//! selected.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str =
    "a3d1a2d825e536dacfdddc95b5ca39de60b2d1c9af8e3381081a7ae662696a84";

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
        sha256: "61a654e5bb4df611facb23111f450656972af25b1b6f55dff97d844b18adee31",
    },
    ProfileSpec {
        key: "SCH_2800191_28002_13006",
        id: "solidworks-2800191-28002-13006-r1",
        revision: 1,
        sha256: "f0490af8d0773a1ad5f8804c4b5638ecd00c08aad7b05cc846f4649aa96d601a",
    },
    ProfileSpec {
        key: "SCH_2800209_28002_13006",
        id: "solidworks-2800209-28002-13006-r1",
        revision: 1,
        sha256: "b4b34fb287a688e68791a1ace42d70d7cff61b23718b15760dffb0d26d27e517",
    },
    ProfileSpec {
        key: "SCH_2900166_28101_13006",
        id: "solidworks-2900166-28101-13006-r1",
        revision: 1,
        sha256: "e0c74972b365d686e37c297134906e40617102e6be3c8a9782c06f780a25f888",
    },
    ProfileSpec {
        key: "SCH_2900190_28101_13006",
        id: "solidworks-2900190-28101-13006-r1",
        revision: 1,
        sha256: "d8a47dec500e2ed31901257b79de15a72b3a67784373d409f808f269cef4de62",
    },
    ProfileSpec {
        key: "SCH_2900195_28101_13006",
        id: "solidworks-2900195-28101-13006-r1",
        revision: 1,
        sha256: "cbb74b85cc8bbbba07dc518c6320c406029a159156fbb0f10448532e52f2be44",
    },
    ProfileSpec {
        key: "SCH_3000226_30000_13006",
        id: "solidworks-3000226-30000-13006-r1",
        revision: 1,
        sha256: "d606e1f8d7b738717de4c9c2212db805cd7e9d0bb1710ffea8447ee166ec9255",
    },
    ProfileSpec {
        key: "SCH_3000298_30000_13006",
        id: "solidworks-3000298-30000-13006-r1",
        revision: 1,
        sha256: "6bd9e9778f9dfe75fb879db617daa4b7f888718ea14c3f01cce68c44c880a5e5",
    },
    ProfileSpec {
        key: "SCH_3100203_31001_13006",
        id: "solidworks-3100203-31001-13006-r1",
        revision: 1,
        sha256: "43458838d65f494f5d15fb854ba6fb072cd8071b0f7bb72a83e0636066dbd651",
    },
    ProfileSpec {
        key: "SCH_3101290_31100_13006",
        id: "solidworks-3101290-31100-13006-r1",
        revision: 1,
        sha256: "63124f4e458c2bfd26998dac4ac29a2fbee04b858675db13e3bd89b2bca58827",
    },
    ProfileSpec {
        key: "SCH_3401254_34101_13006",
        id: "solidworks-3401254-34101-13006-r1",
        revision: 1,
        sha256: "dc94f65e04520c3ad3707fc293c1f3456bda4b53de64cb332d03038724f6f69d",
    },
    ProfileSpec {
        key: "SCH_3501210_35102_13006",
        id: "solidworks-3501210-35102-13006-r1",
        revision: 1,
        sha256: "805b85624a1cc298e13493443b0edc73279b5f0ab7b79f317deaba94d6950f7b",
    },
    ProfileSpec {
        key: "SCH_3501256_35102_13006",
        id: "solidworks-3501256-35102-13006-r1",
        revision: 1,
        sha256: "1d30ef6d2637e866800116a1a6542664472b4da337874dce5db25a23f7ac9758",
    },
    ProfileSpec {
        key: "SCH_3601228_36001_13006",
        id: "solidworks-3601228-36001-13006-r1",
        revision: 1,
        sha256: "30a14081482f630314285cd3a62ac02ea217d66f3d209dd2efb6f5f0635d8525",
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
