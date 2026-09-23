//! Exact iCAD V34 embedded key, using the independently reviewed 13006 subset.
//! No catalog field layouts are copied; unsupported base types remain unknown.
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str =
    "eaebc3477246b7a6b56d1b5dc500029beba46f54e9226973883bc444684f26eb";

/// Construct the reviewed base subset for iCAD SX V8L3 exported resources.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn icad_sch34101_13006() -> Result<BuiltinSchemaProfile, ParseError> {
    let mut definitions = super::sch13006_definitions();
    // Reuse only the public-reference TORUS (54) from the reviewed 13006 base.
    // Exact iCAD producer inputs qualify this unchanged declaration separately.
    definitions.extend(
        super::onshape_current_definitions()
            .into_iter()
            .filter(|d| d.node_type == 54),
    );
    BuiltinSchemaProfile::new_embedded(
        BuiltinProfileMetadata {
            profile_id: "icad-sch34101-13006-r2".into(),
            revision: 2,
            provider_schema: "13006".into(),
            producer_scope: "iCAD V34 extracted Parasolid streams".into(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: PROFILE_SHA256.into(),
        },
        vec!["SCH_3401212_34101_13006".into()],
        definitions,
        vec![],
        // Reuse only the existing 13006 membership audit: 204 is absent.
        vec![204],
    )
}
