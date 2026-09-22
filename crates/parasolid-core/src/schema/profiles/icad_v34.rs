//! Exact iCAD V34 embedded key, using the independently reviewed 13006 subset.
//! No catalog field layouts are copied; unsupported base types remain unknown.
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str =
    "a516a515d3d0c0866a001cf148e7e2e066e9741912c26d45c6ba4fc232179d6e";

/// Construct the reviewed base subset for iCAD SX V8L3 exported resources.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn icad_sch34101_13006() -> Result<BuiltinSchemaProfile, ParseError> {
    BuiltinSchemaProfile::new_embedded(
        BuiltinProfileMetadata {
            profile_id: "icad-sch34101-13006-r1".into(),
            revision: 1,
            provider_schema: "13006".into(),
            producer_scope: "iCAD V34 extracted Parasolid streams".into(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: PROFILE_SHA256.into(),
        },
        vec!["SCH_3401212_34101_13006".into()],
        super::sch13006_definitions(),
        vec![],
        // Reuse only the existing 13006 membership audit: 204 is absent.
        vec![204],
    )
}
