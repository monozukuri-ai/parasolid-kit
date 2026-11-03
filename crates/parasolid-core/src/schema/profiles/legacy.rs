//! Exact keys of older Parasolid versions observed in public transmit files.
//!
//! No public schema file describes schemas 9003 to 16004. Each profile here
//! declares the node types the public files of its key carry with the
//! reviewed 13006 layouts and the edits those streams show, each consistent
//! with the public XT Format Reference: under Parasolid 9 `BODY` carries no
//! `geometry_state` (the field the reference, printed page 85, marks "set to
//! 1, for future use"), `FIN` carries no attribute pointer and `CURVE_DATA`
//! only its self-intersection state; under Parasolid 9 to 12 `ATTRIB_DEF`
//! carries no field names and thirteen legal owners, as the reference's own
//! schema-12006 transmit example (printed page 10) writes it. The streams
//! read to their terminators and map complete B-Reps; the files are not
//! redistributed.

use super::icad_standard::Edit::{self, Array, Drop, Keep};
use super::icad_standard::{LIST, ProfileSpec};
use crate::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, FieldDefinition,
    FieldType, ParseError, SchemaSource, TypeDefinition,
};

// The 13006 layout without `geometry_state` (nom_geom_state): 22 fields.
const BODY_9008: &[Edit] = &[Keep(15), Drop, Keep(7)];

// The 13006 layout without `annotations` (attributes_groups): 9 fields.
const FIN_9008: &[Edit] = &[Drop, Keep(9)];

// CURVE_DATA carries the self-intersection state alone; the analytic-form
// pointer of the 13006 layout is not written.
const CURVE_DATA_9008: &[Edit] = &[Keep(1), Drop];

// ATTRIB_DEF without `field_labels` (field_names) and with thirteen legal
// owners instead of fourteen, as the public reference's own transmit example
// of schema 12006 (printed page 10) also writes it.
const ATTRIB_DEF_12006: &[Edit] = &[
    Keep(4),
    Drop,
    Drop,
    Array("allowed_owners", FieldType::Logical, 13),
    Keep(1),
];

// Parasolid 10 keeps the 13006 BODY layout but still writes fins without
// their attribute pointer.
const PARASOLID_10: &[(u16, &[Edit])] = &[
    (17, FIN_9008),
    (80, ATTRIB_DEF_12006),
    (135, CURVE_DATA_9008),
];

// Parasolid 11 and 12 keep the 13006 BODY and FIN layouts.
const PARASOLID_11: &[(u16, &[Edit])] = &[(80, ATTRIB_DEF_12006), (135, CURVE_DATA_9008)];

const PARASOLID_9: &[(u16, &[Edit])] = &[
    (12, BODY_9008),
    (17, FIN_9008),
    (80, ATTRIB_DEF_12006),
    (135, CURVE_DATA_9008),
];

// Schema 9003 writes its lists without the trailing transmission flag that
// 9008 and the reference carry.
const LIST_9003: &[Edit] = &[Keep(11), Drop];

const PARASOLID_9003: &[(u16, &[Edit])] = &[
    (12, BODY_9008),
    (17, FIN_9008),
    (70, LIST_9003),
    (80, ATTRIB_DEF_12006),
    (135, CURVE_DATA_9008),
];

/// `KEY` (102): the part's key as a character string (public reference,
/// printed page 83). Assemblies and bodies may point at one; the mapper reads
/// neither.
pub(crate) fn key_definition() -> TypeDefinition {
    TypeDefinition::from_fields(
        102,
        "sch13006_type_102",
        "Reviewed part key",
        vec![FieldDefinition {
            name: "key_bytes".into(),
            field_type: FieldType::Character,
            pointer_class: 0,
            element_count: 1,
            transmitted: true,
        }],
        SchemaSource::Base,
    )
}

/// One exact key of an older version with the producer that wrote it.
pub(crate) struct LegacySpec {
    pub profile: ProfileSpec,
    producer: &'static str,
}

pub(crate) const PROFILES: &[LegacySpec] = &[
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_900141_9003",
            id: "nx-900141-9003-r1",
            revision: 1,
            sha256: "a76f2f8fb2ef43265887356651c011a09c2a39ef284b506a0427ed610c4e1dd4",
            schema: "9003",
            // The node types the two public NX assemblies carry.
            types: &[
                10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 38, 40, 41, 45, 50, 51, 52, 54,
                67, 70, 74, 79, 80, 81, 82, 83, 84, 90, 91, 100, 102, 124, 125, 126, 127, 128, 133,
                134, 135, 136, 141,
            ],
            edits: PARASOLID_9003,
        },
        producer: "NX (unigraphics) transmit files, Parasolid 9.0.141",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_901297_9008",
            id: "microstation-901297-9008-r1",
            revision: 1,
            sha256: "9069e19228b0ad87331d428b316379fd674afe56af939f3af956cefbfbbd697f",
            schema: "9008",
            // The node types the eight public files carry (four of the NIST
            // repository's Bentley folder, four of other builds of the schema in
            // its Unigraphics folder).
            types: &[
                12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52, 53, 54,
                56, 59, 60, 68, 70, 74, 79, 80, 81, 82, 83, 84, 90, 91, 124, 125, 126, 127, 128,
                134, 135, 136, 141,
            ],
            edits: PARASOLID_9,
        },
        producer: "MicroStation modeler transmit files, Parasolid 9.0.129",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_1000252_10004",
            id: "nx-1000252-10004-r1",
            revision: 1,
            sha256: "a744e21d11d31c699a658ccd117431250364255a88707f32a01694d84be45063",
            schema: "10004",
            // The node types the three public files carry (two NX parts, one
            // SolidWorks 98Plus part under another build of the schema).
            types: &[
                10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 53,
                54, 56, 59, 60, 70, 74, 79, 80, 81, 82, 83, 84, 90, 91, 100, 124, 125, 126, 127,
                128, 133, 134, 135, 136, 137, 141,
            ],
            edits: PARASOLID_10,
        },
        producer: "NX (unigraphics) transmit files, Parasolid 10.0.252",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_1100129_11004",
            id: "ironcad-1100129-11004-r1",
            revision: 1,
            sha256: "e3710ba1d738ab627448987fe60b81808d0f985c9368433fd0266064953f1adb",
            schema: "11004",
            // The node types the ten public IronCAD files carry.
            types: &[
                12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52, 53, 54,
                56, 59, 124, 125, 126, 127, 128, 133, 134, 135, 136, 137, 141,
            ],
            edits: PARASOLID_11,
        },
        producer: "IronCAD transmit files, Parasolid 11.0.129",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_1200000_12006",
            id: "solidworks-1200000-12006-r1",
            revision: 1,
            sha256: "fc4d672349724760f98e267dfa2d901b9dcf4d1335c4c03a6509e40f359c0ecc",
            schema: "12006",
            // The node types of the reference's transmit example (printed page
            // 10). The one public file of this key stops at its first CYLINDER
            // under this profile, and would stop at HELIX_CU_FORM (163), which the
            // reference does not describe, with its types declared.
            types: &[12, 13, 14, 15, 16, 17, 19, 31, 50, 70, 74, 79, 80, 81, 83],
            edits: PARASOLID_11,
        },
        producer: "SolidWorks 2022 transmit files under the reference key, Parasolid 12.0",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_1600000_16004",
            id: "nx-1600000-16004-r1",
            revision: 1,
            sha256: "ac2c24ed9b802285a17fd6cbb20805944442363d831ba6aaf40578de1bed2590",
            schema: "16004",
            // The node types the one public NX file carries.
            types: &[
                12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 45, 50, 70, 74, 79, 80, 81, 83, 84, 124,
                125, 126, 127, 128, 133, 134, 135, 136, 137, 141,
            ],
            edits: &[(70, LIST)],
        },
        producer: "NX (unigraphics) transmit files, Parasolid 16.0",
    },
];

/// Construct the profiles of the older standard keys.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn legacy_standard_keys() -> Result<Vec<BuiltinSchemaProfile>, ParseError> {
    PROFILES
        .iter()
        .map(|spec| {
            BuiltinSchemaProfile::new(
                BuiltinProfileMetadata {
                    profile_id: spec.profile.id.into(),
                    revision: spec.profile.revision,
                    provider_schema: spec.profile.schema.into(),
                    producer_scope: spec.producer.into(),
                    coverage: BuiltinProfileCoverage::VerifiedSubset,
                    evidence_manifest_sha256: None,
                    profile_sha256: spec.profile.sha256.into(),
                },
                vec![spec.profile.key.into()],
                spec.profile.definitions()?,
            )
        })
        .collect()
}
