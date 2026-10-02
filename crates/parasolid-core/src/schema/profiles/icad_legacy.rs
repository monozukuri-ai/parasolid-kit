//! Exact legacy iCAD keys qualified with authored fixtures and retained streams.
//! Declarations come from the public XT reference and existing producer profiles;
//! external catalogs are used only for subsequent comparison and membership checks.

use crate::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, FieldDefinition,
    FieldType, ParseError, SchemaSource, TypeDefinition,
};

pub(crate) struct ProfileSpec {
    pub key: &'static str,
    pub id: &'static str,
    pub sha256: &'static str,
    extra_types: &'static [u16],
}

pub(crate) const PROFILES: &[ProfileSpec] = &[
    ProfileSpec {
        key: "SCH_1500137_15003_13006",
        id: "icad-1500137-15003-13006-r1",
        sha256: "dcb4bb70654e9d1bf091b1310b2068872b56304f0b820435a3910bcec366a9e8",
        extra_types: &[],
    },
    ProfileSpec {
        key: "SCH_1500245_15003_13006",
        id: "icad-1500245-15003-13006-r1",
        sha256: "4179d5785e61e89926dfb54e0bc5eed7f0fb7cdec7300de0936ef678ba4d0f4c",
        extra_types: &[56, 59, 68],
    },
    ProfileSpec {
        key: "SCH_1700223_16100_13006",
        id: "icad-1700223-16100-13006-r1",
        sha256: "a27d8216055fe16425438c854482ec8dfac5e6f2e0aea1e1b7ce3605bc6ac633",
        extra_types: &[],
    },
    ProfileSpec {
        key: "SCH_1700256_16100_13006",
        id: "icad-1700256-16100-13006-r1",
        sha256: "30fc3f141639819bb6c76a4585056c7b274a6103d6c3924f71c448603796e802",
        extra_types: &[45, 68, 127, 128, 134, 135, 136, 137],
    },
    ProfileSpec {
        key: "SCH_1901315_19008_13006",
        id: "icad-1901315-19008-13006-r1",
        sha256: "781de5125a27ee855bd377a456df9f21d2a71099ec231cbd1d67a86b0c637f96",
        extra_types: &[],
    },
];

impl ProfileSpec {
    pub(crate) fn definitions(&self) -> Vec<TypeDefinition> {
        let mut definitions = super::sch13006_definitions();
        definitions.extend(
            super::onshape_current_definitions()
                .into_iter()
                .filter(|d| d.node_type == 54),
        );
        definitions.extend(
            super::sch13006_sp_curve_definitions()
                .into_iter()
                .chain(surface_definitions())
                .filter(|d| self.extra_types.contains(&d.node_type)),
        );
        definitions
    }
}

/// Construct the five reviewed legacy iCAD embedded profiles.
///
/// Each profile claims one exact key and its separately qualified base subset.
/// `SPUN_SURF` (68) is retained as explicitly unsupported B-Rep geometry.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn icad_legacy_13006() -> Result<Vec<BuiltinSchemaProfile>, ParseError> {
    PROFILES
        .iter()
        .map(|spec| {
            BuiltinSchemaProfile::new_embedded(
                BuiltinProfileMetadata {
                    profile_id: spec.id.into(),
                    revision: 1,
                    provider_schema: "13006".into(),
                    producer_scope: "Legacy iCAD extracted Parasolid streams".into(),
                    coverage: BuiltinProfileCoverage::VerifiedSubset,
                    evidence_manifest_sha256: None,
                    profile_sha256: spec.sha256.into(),
                },
                vec![spec.key.into()],
                spec.definitions(),
                vec![],
                vec![204],
            )
        })
        .collect()
}

/// Public XT reference (April 2008), printed pp. 62-65 and 74-76.
/// Only the observed keys above admit these 13006 declarations.
fn surface_definitions() -> Vec<TypeDefinition> {
    use FieldType::{
        Character as C, Double as F, Integer as D, PointerIndex as P, ShortInteger as N,
        Vector as V,
    };
    let field = |name: &str, field_type, pointer_class, element_count| FieldDefinition {
        name: name.into(),
        field_type,
        pointer_class,
        element_count,
        transmitted: true,
    };
    let common = || {
        vec![
            field("local_id", D, 0, 0),
            field("annotations", P, 1019, 0),
            field("owner_ref", P, 1007, 0),
            field("next_surface", P, 1006, 0),
            field("previous_surface", P, 1006, 0),
            field("indirect_owner", P, 141, 0),
            field("orientation", C, 0, 0),
        ]
    };
    let mut blend = common();
    blend.extend([
        field("blend_kind", C, 0, 0),
        field("support_surfaces", P, 1006, 2),
        field("spine_curve", P, 1008, 0),
        field("offsets", F, 0, 2),
        field("weights", F, 0, 2),
        field("boundary_surfaces", P, 1006, 2),
        field("start_limit", P, 41, 0),
        field("end_limit", P, 41, 0),
    ]);
    let mut boundary = common();
    boundary.extend([
        field("boundary_index", N, 0, 0),
        field("blend_surface", P, 1006, 0),
    ]);
    let mut spun = common();
    spun.extend([
        field("profile_curve", P, 1008, 0),
        field("axis_origin", V, 0, 0),
        field("axis_direction", V, 0, 0),
        field("start_point", V, 0, 0),
        field("end_point", V, 0, 0),
        field("start_parameter", F, 0, 0),
        field("end_parameter", F, 0, 0),
        field("x_direction", V, 0, 0),
        field("scale", F, 0, 0),
    ]);
    [(56, blend), (59, boundary), (68, spun)]
        .into_iter()
        .map(|(kind, fields)| {
            TypeDefinition::from_fields(
                kind,
                format!("sch13006_type_{kind}"),
                "Reviewed legacy iCAD surface",
                fields,
                SchemaSource::Base,
            )
        })
        .collect()
}
