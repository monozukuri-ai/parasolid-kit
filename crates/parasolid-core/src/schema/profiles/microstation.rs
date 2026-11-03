//! Exact key of `MicroStation` modeler transmit files under schema 9008.
//!
//! Parasolid 9.0.129 inside `MicroStation` wrote the four public Bentley files
//! of the NIST `engineering-design-models` repository (`nut.x_t`, `ring.x_t`,
//! `washer inch.x_t`, `buttom part inch.x_t`) under `SCH_901297_9008`. No
//! public schema file describes schema 9008. The layouts are the reviewed
//! 13006 declarations with the three edits those streams show: `BODY` carries
//! no `geometry_state` (the field the public XT Format Reference, printed page
//! 85, marks "set to 1, for future use"), `FIN` carries no attribute pointer,
//! and `ATTRIB_DEF` carries no field names and thirteen legal owners, as the
//! reference's own schema-12006 transmit example (printed page 10) writes it.
//! Every other observed type reads with its 13006 layout; the four streams
//! read to their terminators and map complete B-Reps. The files are not
//! redistributed.

use super::icad_standard::Edit::{self, Array, Drop, Keep};
use super::icad_standard::ProfileSpec;
use crate::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, FieldType, ParseError,
};

// The 13006 layout without `geometry_state` (nom_geom_state): 22 fields.
const BODY_9008: &[Edit] = &[Keep(15), Drop, Keep(7)];

// The 13006 layout without `annotations` (attributes_groups): 9 fields.
const FIN_9008: &[Edit] = &[Drop, Keep(9)];

// ATTRIB_DEF without `field_labels` (field_names) and with thirteen legal
// owners instead of fourteen, as the public reference's own transmit example
// of schema 12006 (printed page 10) also writes it.
const ATTRIB_DEF_9008: &[Edit] = &[
    Keep(4),
    Drop,
    Drop,
    Array("allowed_owners", FieldType::Logical, 13),
    Keep(1),
];

pub(crate) const PROFILES: &[ProfileSpec] = &[ProfileSpec {
    key: "SCH_901297_9008",
    id: "microstation-901297-9008-r1",
    revision: 1,
    sha256: "cc992ddc4cdc9723df08eb07fa0ab52ec7dcc11eb5b7c831a4447f9977c8252a",
    schema: "9008",
    // The node types the four files carry.
    types: &[
        12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 38, 40, 41, 50, 51, 53, 54, 56, 59, 70, 74, 79,
        80, 81, 82, 141,
    ],
    edits: &[(12, BODY_9008), (17, FIN_9008), (80, ATTRIB_DEF_9008)],
}];

/// Construct the `MicroStation` modeler profile for its exact key.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn microstation_9008() -> Result<Vec<BuiltinSchemaProfile>, ParseError> {
    PROFILES
        .iter()
        .map(|spec| {
            BuiltinSchemaProfile::new(
                BuiltinProfileMetadata {
                    profile_id: spec.id.into(),
                    revision: spec.revision,
                    provider_schema: spec.schema.into(),
                    producer_scope: "MicroStation modeler transmit files, Parasolid 9.0.129".into(),
                    coverage: BuiltinProfileCoverage::VerifiedSubset,
                    evidence_manifest_sha256: None,
                    profile_sha256: spec.sha256.into(),
                },
                vec![spec.key.into()],
                spec.definitions()?,
            )
        })
        .collect()
}
