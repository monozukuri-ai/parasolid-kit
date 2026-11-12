//! Exact keys observed in standalone transmit files written by other producers.
//!
//! Each profile anchors the schema family of its key with the reviewed 13006
//! declaration set shared by the `SolidWorks` partition profiles; version
//! differences arrive as each stream's embedded edits. Other Parasolid builds
//! of the same schema resolve through the derived family. The evidence is local
//! parsing of the public files named in the documentation; the files are not
//! redistributed.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) struct ProfileSpec {
    pub key: &'static str,
    pub id: &'static str,
    pub revision: u32,
    pub sha256: &'static str,
    producer: &'static str,
    /// Whether `GROUP` (90) and `MEMBER_OF_GROUP` (91) were observed under the key.
    groups: bool,
}

/// Keys of public standalone transmit files, lowest build first.
pub(crate) const PROFILES: &[ProfileSpec] = &[
    ProfileSpec {
        key: "SCH_2400164_20000_13006",
        id: "parasolid-2400164-20000-13006-r3",
        revision: 3,
        sha256: "a234638a7b7cd27eeeafa7052db262d8832888d4196b22f2b6a169a38d175a92",
        producer: "Parasolid 24.0.164 acceptance-test transmit files",
        groups: true,
    },
    ProfileSpec {
        key: "SCH_2600169_25001_13006",
        id: "nx-2600169-25001-13006-r3",
        revision: 3,
        sha256: "e47af09ef9d77bfa9ff08b994fbd506997d1779655e2371e5c570716ab94ac4f",
        producer: "NX (unigraphics) transmit files, Parasolid 26.0.169",
        groups: false,
    },
    ProfileSpec {
        key: "SCH_2600217_25001_13006",
        id: "parasolid-2600217-25001-13006-r3",
        revision: 3,
        sha256: "942926d0b01fa981078a2d285f0684434950453122311e73b05fbee1aa3622a4",
        producer: "Parasolid 26.0.217 acceptance-test transmit files",
        groups: false,
    },
    // Three public NX parts carry this key; it anchors the first family of
    // schema 30100, which no SolidWorks or iCAD key uses.
    ProfileSpec {
        key: "SCH_3001232_30100_13006",
        id: "nx-3001232-30100-13006-r3",
        revision: 3,
        sha256: "d332ceb261f0f157cf15f1c4e34abc843fcec8d27c65a6cbee6b20a65431626a",
        producer: "NX (unigraphics) transmit files, Parasolid 30.1.232",
        groups: false,
    },
    // One public NX part; the first family of schema 34001.
    ProfileSpec {
        key: "SCH_3400176_34001_13006",
        id: "nx-3400176-34001-13006-r3",
        revision: 3,
        sha256: "a4517ad28a3d8139661b9fe41e317afa2d17ec6824088d22781e315f95189a5b",
        producer: "NX (unigraphics) transmit files, Parasolid 34.0.176",
        groups: false,
    },
];

/// Construct the standalone transmit-file profiles, one per exact key.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn transmit_13006() -> Result<Vec<BuiltinSchemaProfile>, ParseError> {
    PROFILES
        .iter()
        .map(|spec| {
            let mut definitions = super::solidworks::solidworks_definitions();
            if spec.groups {
                definitions.extend(group_definitions());
            }
            BuiltinSchemaProfile::new_embedded(
                BuiltinProfileMetadata {
                    profile_id: spec.id.into(),
                    revision: spec.revision,
                    provider_schema: "13006".into(),
                    producer_scope: spec.producer.into(),
                    coverage: BuiltinProfileCoverage::VerifiedSubset,
                    evidence_manifest_sha256: None,
                    profile_sha256: spec.sha256.into(),
                },
                vec![spec.key.into()],
                definitions,
                vec![],
                vec![204],
            )
        })
        .collect()
}

/// Public XT Format Reference, April 2008, printed pp. 112-114 and 118: `GROUP`
/// (90) and `MEMBER_OF_GROUP` (91) with the documented node classes (`PART`
/// 1005, `GROUP_MEMBER` 1017, `ATTRIB_GROUP` 1019). Both are base 13006 layouts;
/// the 24.0.164 stream reads them to the terminator with the unchanged marker.
/// No group semantics are assigned by the B-Rep mapper.
pub(crate) fn group_definitions() -> Vec<TypeDefinition> {
    use FieldType::{Integer as D, PointerIndex as P, UnsignedByte as U};
    let field = |name: &str, field_type, pointer_class| FieldDefinition {
        name: name.into(),
        field_type,
        pointer_class,
        element_count: 0,
        transmitted: true,
    };
    let group = vec![
        field("local_id", D, 0),
        field("annotations", P, 1019),
        field("owner_ref", P, 1005),
        field("next_annotation", P, 1019),
        field("previous_annotation", P, 1019),
        field("group_kind", U, 0),
        field("first_member", P, 91),
    ];
    let member = vec![
        field("entity_label", D, 0),
        field("owning_group", P, 90),
        field("member_ref", P, 1017),
        field("next_annotation", P, 1019),
        field("previous_annotation", P, 1019),
        field("next_member", P, 91),
        field("previous_member", P, 91),
    ];
    vec![
        TypeDefinition::from_fields(
            90,
            "sch13006_type_90",
            "Reviewed group",
            group,
            SchemaSource::Base,
        ),
        TypeDefinition::from_fields(
            91,
            "sch13006_type_91",
            "Reviewed member of group",
            member,
            SchemaSource::Base,
        ),
    ]
}
