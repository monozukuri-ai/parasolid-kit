//! Exact standard keys observed in public transmit files.
//!
//! No public schema file describes schemas 8008 to 16004 or 25001. Each
//! profile here declares the node types the files of its key carry with the
//! reviewed 13006 layouts and the edits those streams show, each consistent
//! with the public XT Format Reference: under Parasolid 8 and 9 `BODY` carries
//! no `geometry_state` (the field the reference, printed page 85, marks "set
//! to 1, for future use"), `FIN` carries no attribute pointer and `CURVE_DATA`
//! only its self-intersection state; under Parasolid 8 to 11 `ATTRIB_DEF`
//! carries no field names and thirteen legal owners, as the reference's own
//! schema-12006 transmit example (printed page 10) writes it, while the
//! schema-12006 files of current writers list fourteen. Schema 25001 takes the
//! edits that embedded 25001 streams carry against base 13006. Schema 31001
//! takes the edits of the standard key of the next revision (32001): `BODY`
//! with its mesh offset reference and `INTERSECTION` with its data pointer.
//! Schema 12103 keeps the reviewed 13006 layouts: a transmit file of that key
//! reads to its terminator with them and not with the 12006 `ATTRIB_DEF`.
//! Schema 37102 takes the edits that embedded 37102 streams carry against
//! base 13006. The multi-part transmit block (176) of schema 25001 is the
//! layout later embedded streams declare in full, without the mesh offset
//! reference that `BODY` also lacks before schema 32001.
//! The streams read to their terminators and map complete B-Reps; the files
//! are not redistributed.

use super::icad_standard::Edit::{self, Array, Drop, Keep};
use super::icad_standard::{
    ATTRIB_DEF_37102, BODY_32001, BODY_37102, INTERSECTION, LIMIT, LIST, POINTER_BLOCK,
    ProfileSpec, REGION, REGION_37102, integer, pointer,
};
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

// Parasolid 11 keeps the 13006 BODY and FIN layouts.
const PARASOLID_11: &[(u16, &[Edit])] = &[(80, ATTRIB_DEF_12006), (135, CURVE_DATA_9008)];

// Schema 12006 as SolidWorks and CAD Exchanger write it: ATTRIB_DEF without
// `field_labels` but with the fourteen legal owners of the reference's
// declaration (printed page 106); CURVE_DATA keeps its analytic-form pointer.
const ATTRIB_DEF_12006_FOURTEEN_OWNERS: &[Edit] = &[Keep(4), Drop, Keep(2)];

const PARASOLID_12: &[(u16, &[Edit])] = &[(80, ATTRIB_DEF_12006_FOURTEEN_OWNERS)];

const PARASOLID_9: &[(u16, &[Edit])] = &[
    (12, BODY_9008),
    (17, FIN_9008),
    (80, ATTRIB_DEF_12006),
    (135, CURVE_DATA_9008),
];

// Schemas 8008 and 9003 write their lists without the trailing transmission
// flag that 9008 and the reference carry.
const LIST_9003: &[Edit] = &[Keep(11), Drop];

// Schema 8008 shares every edit of 9003.
const PARASOLID_9003: &[(u16, &[Edit])] = &[
    (12, BODY_9008),
    (17, FIN_9008),
    (70, LIST_9003),
    (80, ATTRIB_DEF_12006),
    (135, CURVE_DATA_9008),
];

// Embedded 25001 streams declare BODY as the 28002 edit without the boundary
// mesh: the owner pointer of the 13006 layout is replaced and six fields are
// appended (29 fields).
const BODY_25001: &[Edit] = &[
    Keep(13),
    Drop,
    pointer("container_ref", 1040),
    Keep(9),
    integer("index_origin"),
    pointer("index_values", 82),
    pointer("node_id_values", 82),
    pointer("schema_values", 82),
    pointer("child_body", 12),
    integer("min_local_id"),
];

// The other edits of embedded 25001 streams are those of the iCAD standard
// keys; every other observed type keeps its 13006 layout.
const SCHEMA_25001: &[(u16, &[Edit])] = &[
    (12, BODY_25001),
    (19, REGION),
    (70, LIST),
    (74, POINTER_BLOCK),
];

// A transmit file written under the standard key of schema 31001 reads to
// its terminator with the edits of the 32001 standard key and with no other
// combination of the neighbouring revisions' BODY and INTERSECTION edits.
const SCHEMA_31001: &[(u16, &[Edit])] = &[
    (12, BODY_32001),
    (19, REGION),
    (38, INTERSECTION),
    (41, LIMIT),
    (70, LIST),
    (74, POINTER_BLOCK),
];

// A transmit file written under the standard key of schema 37102 reads to
// its terminator with the edits that embedded 37102 streams carry: `BODY`
// with its lattice references, `REGION` with its frame reference,
// `ATTRIB_DEF` with seventeen legal owners and the list types of the later
// standard keys.
const SCHEMA_37102: &[(u16, &[Edit])] = &[
    (12, BODY_37102),
    (19, REGION_37102),
    (70, LIST),
    (74, POINTER_BLOCK),
    (80, ATTRIB_DEF_37102),
];

/// The multi-part transmit block (176), the node that opens a transmit file
/// of several parts. Embedded streams of later revisions declare it in full:
/// an entry count, the index origin, the index and schema value references, a
/// mesh offset reference and the part entries. A transmit file under the
/// standard key of schema 25001 reads to its terminator without the mesh
/// offset reference, which `BODY` also lacks before schema 32001, and with no
/// other subset of those fields. The B-Rep mapper assigns the node no role.
pub(crate) fn transmit_block_definition() -> TypeDefinition {
    use FieldType::{Integer as D, PointerIndex as P};
    let field = |name: &str, field_type, pointer_class, element_count| FieldDefinition {
        name: name.into(),
        field_type,
        pointer_class,
        element_count,
        transmitted: true,
    };
    TypeDefinition::from_fields(
        176,
        "standard_type_176",
        "Reviewed multi-part transmit block",
        vec![
            field("entry_count", D, 0, 0),
            field("index_origin", D, 0, 0),
            field("index_values", P, 82, 0),
            field("schema_values", P, 82, 0),
            field("entries", P, 1005, 1),
        ],
        SchemaSource::Base,
    )
}

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
            key: "SCH_800000_8008",
            id: "solidworks-800000-8008-r1",
            revision: 1,
            sha256: "1aa242c9e5b46f7eadc7876f475c30183659ecfc036a8279e078880502178cc5",
            schema: "8008",
            // The node types of SolidWorks transmit files saved under this key
            // and of iCAD SX transmit files under another build of the schema.
            types: &[
                10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52,
                53, 54, 56, 59, 60, 67, 70, 74, 79, 80, 81, 82, 83, 84, 86, 100, 102, 124, 125,
                126, 127, 128, 134, 135, 136, 141,
            ],
            edits: PARASOLID_9003,
        },
        producer: "SolidWorks transmit files saved as Parasolid 8",
    },
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
            id: "microstation-901297-9008-r2",
            revision: 2,
            sha256: "aea055eee8fae1b4348815c5390838288551609674b842db4bc4daecaf851d91",
            schema: "9008",
            // The node types the eight public files carry (four of the NIST
            // repository's Bentley folder, four of other builds of the schema in
            // its Unigraphics folder).
            // Revision 2 adds the assembly structure (10, 11, 100), SWEPT_SURF
            // (67), AXIS_VALUES (87), TRIMMED_CURVE (133) and SP_CURVE (137),
            // observed in a Solid Edge assembly saved under another build of
            // the schema (`SCH_901000_9008`); all keep their reviewed 13006
            // layouts and the assembly reads to the terminator.
            types: &[
                10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52,
                53, 54, 56, 59, 60, 67, 68, 70, 74, 79, 80, 81, 82, 83, 84, 87, 90, 91, 100, 124,
                125, 126, 127, 128, 133, 134, 135, 136, 137, 141,
            ],
            edits: PARASOLID_9,
        },
        producer: "MicroStation modeler transmit files, Parasolid 9.0.129",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_1000252_10004",
            id: "nx-1000252-10004-r2",
            revision: 2,
            sha256: "f00b90e3c5beee0a627c2f026c3267a0b1f55a08f599d19ee6a859dbfa25967a",
            schema: "10004",
            // The node types the three public files carry (two NX parts, one
            // SolidWorks 98Plus part under another build of the schema).
            // Revision 2 adds CONE (52), SWEPT_SURF (67) and SPUN_SURF (68),
            // observed in NX and Kompas-3D files under `SCH_1000000_10004`.
            types: &[
                10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52,
                53, 54, 56, 59, 60, 67, 68, 70, 74, 79, 80, 81, 82, 83, 84, 90, 91, 100, 124, 125,
                126, 127, 128, 133, 134, 135, 136, 137, 141,
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
            id: "solidworks-1200000-12006-r2",
            revision: 2,
            sha256: "32c0ed77f0c30c31f226521bf7a7b103c14cdbc722f36abb2049e8ecc070246b",
            schema: "12006",
            // Revision 1 declared the node types of the reference's transmit
            // example (printed page 10). Revision 2 declares the types that
            // SolidWorks and CAD Exchanger transmit files of this key carry,
            // the helical surface form (163) included, and their fourteen
            // legal owners.
            types: &[
                10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 38, 40, 41, 45, 50, 51, 53, 54,
                70, 74, 79, 80, 81, 82, 83, 84, 100, 124, 125, 126, 127, 128, 133, 134, 135, 136,
                137, 141, 163,
            ],
            edits: PARASOLID_12,
        },
        producer: "SolidWorks 2022 transmit files under the reference key, Parasolid 12.0",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_1200000_12103",
            id: "cadexchanger-1200000-12103-r1",
            revision: 1,
            sha256: "c85519111531174eaed40542cefbc03f044c3d12784f8f6e06719fc571f36f35",
            schema: "12103",
            // The node types of a CAD Exchanger assembly written under this
            // key. Every type keeps its reviewed 13006 layout.
            types: &[
                10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 45, 50, 51, 52, 53, 54, 70, 74,
                79, 80, 81, 83, 84, 100, 124, 126, 127, 128, 133, 134, 135, 136, 137, 141,
            ],
            edits: &[],
        },
        producer: "CAD Exchanger transmit files under the standard key, Parasolid 12.1",
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
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_2600169_25001",
            id: "inventor-2600169-25001-r2",
            revision: 2,
            sha256: "6852a4b03ad92ed762ea6c5b773c46ea7daac3bc43a98ebba7a8f5a4ee6dac80",
            schema: "25001",
            // The node types of Autodesk Inventor 2015 transmit files under this
            // key and of iCAD SX transmit files under another build of the schema.
            // Revision 2 adds SPHERE (53), SPUN_SURF (68), UNICODE_VALUES (98),
            // TRIMMED_CURVE (133), SP_CURVE (137) and the multi-part transmit
            // block (176), observed in transmit files of several parts and of
            // Onshape under other builds of the schema (`SCH_2500000_25001`,
            // `SCH_2600000_25001`).
            types: &[
                10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52,
                53, 54, 68, 70, 74, 79, 80, 81, 82, 83, 84, 98, 124, 125, 126, 127, 128, 133, 134,
                135, 136, 137, 141, 176,
            ],
            edits: SCHEMA_25001,
        },
        producer: "Autodesk Inventor transmit files, Parasolid 26.0.169",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_3100000_31001",
            id: "parasolid-3100000-31001-r1",
            revision: 1,
            sha256: "2887c464c9c858fe3f369daff35a37a69be5ad550e4a48808bb86870e5f1f52d",
            schema: "31001",
            // The node types of a transmit file written under this key.
            types: &[
                12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52, 56, 59,
                70, 74, 79, 80, 81, 82, 83, 84, 98, 127, 128, 133, 134, 135, 136, 137, 141, 204,
            ],
            edits: SCHEMA_31001,
        },
        producer: "Transmit files under the standard key, Parasolid 31.0",
    },
    LegacySpec {
        profile: ProfileSpec {
            key: "SCH_3701153_37102",
            id: "feko-3701153-37102-r1",
            revision: 1,
            sha256: "8fb4f530b0f8fa7eb1784078d3cb2d5ba969bc7672382dd0897030f7148f2c97",
            schema: "37102",
            // The node types of Altair Feko transmit files written under
            // this key.
            types: &[
                12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 45, 50, 51, 52, 53, 70, 74, 79, 80, 81,
                84, 127, 128, 134, 135, 136,
            ],
            edits: SCHEMA_37102,
        },
        producer: "Altair Feko transmit files, Parasolid 37.1.153",
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
