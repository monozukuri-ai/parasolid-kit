//! Hash-pinned base and effective `psolid` catalogs for observed `TopSolid` v6 `X_T` keys.

use std::{collections::BTreeMap, io};

use crate::schema::profiles::{
    topsolid_13006_definitions, topsolid_26105_definitions, topsolid_32001_definitions,
    topsolid_36001_definitions,
};
use crate::schema::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, FieldType,
    SchemaCatalogLimits, SchemaKey, SchemaProvider, SchemaProviderProvenance, SchemaTypeLookup,
    TypeDefinition, parse_schema_catalog,
};
pub const CATALOG_13006_ID: &str = "13006";
const CATALOG_13006_SHA256: &str =
    "0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3";
const ABSENT_13006_MESH_TYPES: [u16; 6] = [189, 190, 191, 192, 193, 201];

pub const CATALOG_36001_ID: &str = "36001";
const CATALOG_36001_SHA256: &str =
    "42ec5f0bda547618007f3ebeac1479b73d8444084d24ac228e099eb9c5b67430";

pub const CATALOG_32001_ID: &str = "32001";
const CATALOG_32001_SHA256: &str =
    "5ffe6a8942c686e400b3cad1edc19fbe39457589079769d9f7dbcafec535a159";

pub const CATALOG_26105_ID: &str = "26105";
const CATALOG_26105_SHA256: &str =
    "c21517ad14061ff14e2636bb90842ba74b3ad4980b7d041abd78732d80249e95";

/// Complete transmit schema keys supported by the bundled base catalog.
pub const SUPPORTED_SCHEMA_KEYS: [&str; 5] = [
    "SCH_2701192_26105_13006",
    "SCH_3300259_32001_13006",
    "SCH_3300261_32001_13006",
    "SCH_3700192_36001_13006",
    "SCH_3600188_36001_13006",
];

/// Construct the compiled 13006 base subset for one exact observed `TopSolid` v6 key.
///
/// This profile exposes only the base 13006 definitions. Use
/// [`TopSolidV6PsolidCatalogProvider`] when effective 36001, 32001, or 26105
/// schema layouts are needed. This profile omits node types 0 and 1 and marks
/// types with transmitted opaque-pointer fields as present-but-unsupported.
/// Use the direct catalog provider when those types are needed.
///
/// # Errors
/// Returns invalid data if the compiled base profile is inconsistent.
pub fn topsolid_v6_psolid_base_profile(
    key: &SchemaKey,
) -> io::Result<Option<BuiltinSchemaProfile>> {
    if !SUPPORTED_SCHEMA_KEYS.contains(&key.raw()) {
        return Ok(None);
    }
    let definitions: BTreeMap<_, _> = topsolid_13006_definitions()
        .into_iter()
        .map(|definition| (definition.node_type, definition))
        .collect();
    let mut absent_base_types = ABSENT_13006_MESH_TYPES.to_vec();
    if matches!(
        key.raw(),
        "SCH_3300261_32001_13006"
            | "SCH_3300259_32001_13006"
            | "SCH_3700192_36001_13006"
            | "SCH_3600188_36001_13006"
    ) {
        absent_base_types.push(204);
    }
    let unsupported_base_types = definitions
        .values()
        .filter(|definition| {
            definition.node_type > 1
                && definition
                    .fields
                    .iter()
                    .any(|field| field.transmitted && field.field_type == FieldType::OpaquePointer)
        })
        .map(|definition| definition.node_type)
        .collect();
    let suffix = key.raw().to_ascii_lowercase().replace('_', "-");
    BuiltinSchemaProfile::new_embedded(
        BuiltinProfileMetadata {
            profile_id: format!("topsolid-v6-{suffix}-r1"),
            revision: 1,
            provider_schema: CATALOG_13006_ID.into(),
            producer_scope: "TopSolid v6 observed X_T streams".into(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: CATALOG_13006_SHA256.into(),
        },
        vec![key.raw().into()],
        definitions
            .into_values()
            .filter(|definition| definition.node_type > 1)
            .filter(|definition| {
                definition
                    .fields
                    .iter()
                    .all(|field| !field.transmitted || field.field_type != FieldType::OpaquePointer)
            })
            .collect(),
        unsupported_base_types,
        absent_base_types,
    )
    .map(Some)
    .map_err(|error| invalid_catalog_data(CATALOG_13006_ID, &error.to_string()))
}

/// Compiled exact-catalog provider for `X_T` streams.
#[derive(Debug, Clone)]
pub struct TopSolidV6PsolidCatalogProvider {
    definitions: BTreeMap<u16, TypeDefinition>,
    effective_definitions: BTreeMap<String, BTreeMap<u16, TypeDefinition>>,
    is_caller_supplied: bool,
}

impl TopSolidV6PsolidCatalogProvider {
    /// Build the provider from compiled exact base and effective definitions.
    ///
    /// # Errors
    /// Returns invalid data if any compiled catalog has duplicate definitions.
    pub fn new() -> io::Result<Self> {
        let definitions =
            index_catalog_definitions(topsolid_13006_definitions(), CATALOG_13006_ID)?;

        let mut effective_definitions = BTreeMap::new();
        effective_definitions.insert(
            CATALOG_36001_ID.to_string(),
            index_catalog_definitions(topsolid_36001_definitions(), CATALOG_36001_ID)?,
        );
        effective_definitions.insert(
            CATALOG_32001_ID.to_string(),
            index_catalog_definitions(topsolid_32001_definitions(), CATALOG_32001_ID)?,
        );
        effective_definitions.insert(
            CATALOG_26105_ID.to_string(),
            index_catalog_definitions(topsolid_26105_definitions(), CATALOG_26105_ID)?,
        );

        Ok(Self {
            definitions,
            effective_definitions,
            is_caller_supplied: false,
        })
    }

    /// Validate a caller-supplied effective catalog alongside bundled base 13006.
    ///
    /// # Errors
    /// Returns invalid data if parsing fails or definitions are duplicated in
    /// the supplied or compiled catalogs.
    pub fn new_with_effective_schema_bytes(bytes: &[u8]) -> io::Result<Self> {
        let mut provider = Self::new()?;
        let catalog = parse_schema_catalog(
            bytes,
            SchemaCatalogLimits {
                max_file_size: 64 * 1024,
                max_schema_types: 1_024,
                max_fields_per_type: 4_096,
                max_string_bytes: 4_096,
            },
        )
        .map_err(|error| invalid_effective_catalog_data(&error.to_string()))?;
        let mut definitions = BTreeMap::new();
        for definition in catalog.definitions {
            if definitions
                .insert(definition.node_type, definition)
                .is_some()
            {
                return Err(invalid_effective_catalog_data("duplicate type definition"));
            }
        }
        provider
            .effective_definitions
            .insert(catalog.schema_id, definitions);
        provider.is_caller_supplied = true;
        Ok(provider)
    }

    /// Select the built-in provider for one exact observed complete key.
    #[must_use]
    pub fn provider_for_key(&self, key: &SchemaKey) -> Option<TopSolidV6PsolidSchemaProvider<'_>> {
        let schema_key = SUPPORTED_SCHEMA_KEYS
            .iter()
            .copied()
            .find(|supported| key.raw() == *supported)?;
        let effective_schema_id = match schema_key {
            "SCH_3700192_36001_13006" => Some(CATALOG_36001_ID),
            "SCH_3300261_32001_13006" | "SCH_3300259_32001_13006" => Some(CATALOG_32001_ID),
            "SCH_2701192_26105_13006" => Some(CATALOG_26105_ID),
            _ => None,
        };
        let effective_definitions =
            effective_schema_id.and_then(|id| self.effective_definitions.get(id));
        Some(TopSolidV6PsolidSchemaProvider {
            definitions: &self.definitions,
            schema_key,
            effective_schema_id,
            effective_definitions,
            is_caller_supplied: self.is_caller_supplied,
        })
    }
}

fn index_catalog_definitions(
    input: Vec<TypeDefinition>,
    expected_id: &str,
) -> io::Result<BTreeMap<u16, TypeDefinition>> {
    let mut definitions = BTreeMap::new();
    for definition in input {
        if definitions
            .insert(definition.node_type, definition)
            .is_some()
        {
            return Err(invalid_catalog_data(
                expected_id,
                "duplicate type definition",
            ));
        }
    }
    Ok(definitions)
}

/// Key-selected provider view over the compiled exact catalog definitions.
#[derive(Debug, Clone, Copy)]
pub struct TopSolidV6PsolidSchemaProvider<'a> {
    definitions: &'a BTreeMap<u16, TypeDefinition>,
    schema_key: &'static str,
    effective_schema_id: Option<&'static str>,
    effective_definitions: Option<&'a BTreeMap<u16, TypeDefinition>>,
    is_caller_supplied: bool,
}

impl SchemaProvider for TopSolidV6PsolidSchemaProvider<'_> {
    fn contains_schema(&self, schema: &str) -> bool {
        schema == CATALOG_13006_ID || self.effective_schema_id == Some(schema)
    }

    fn type_definition(&self, schema: &str, node_type: u16) -> Option<&TypeDefinition> {
        if schema == CATALOG_13006_ID {
            self.definitions.get(&node_type)
        } else if self.effective_schema_id == Some(schema) {
            self.effective_definitions
                .and_then(|definitions| definitions.get(&node_type))
        } else {
            None
        }
    }

    fn lookup_type(&self, schema: &str, node_type: u16) -> SchemaTypeLookup<'_> {
        if let Some(definition) = self.type_definition(schema, node_type) {
            SchemaTypeLookup::Defined(definition)
        } else if schema == CATALOG_13006_ID && ABSENT_13006_MESH_TYPES.contains(&node_type) {
            SchemaTypeLookup::Absent
        } else if schema == CATALOG_13006_ID
            && node_type == 204
            && matches!(
                self.schema_key,
                "SCH_3300261_32001_13006"
                    | "SCH_3300259_32001_13006"
                    | "SCH_3700192_36001_13006"
                    | "SCH_3600188_36001_13006"
            )
        {
            // These exact pinned streams require an embedded full declaration for type 204.
            SchemaTypeLookup::Absent
        } else {
            SchemaTypeLookup::Unknown
        }
    }

    fn supports_schema_key(&self, key: &SchemaKey) -> bool {
        key.raw() == self.schema_key
    }

    fn provenance(&self) -> SchemaProviderProvenance<'_> {
        if self.is_caller_supplied {
            return SchemaProviderProvenance::CallerSupplied;
        }
        match self.effective_schema_id {
            Some(CATALOG_36001_ID) => SchemaProviderProvenance::Builtin {
                profile_id: "topsolid-v6-psolid-36001-r1",
                profile_revision: 1,
                schema_key: self.schema_key,
                coverage: BuiltinProfileCoverage::VerifiedSubset,
                profile_sha256: CATALOG_36001_SHA256,
            },
            Some(CATALOG_32001_ID) => SchemaProviderProvenance::Builtin {
                profile_id: "topsolid-v6-psolid-32001-r1",
                profile_revision: 1,
                schema_key: self.schema_key,
                coverage: BuiltinProfileCoverage::VerifiedSubset,
                profile_sha256: CATALOG_32001_SHA256,
            },
            Some(CATALOG_26105_ID) => SchemaProviderProvenance::Builtin {
                profile_id: "topsolid-v6-psolid-26105-r1",
                profile_revision: 1,
                schema_key: self.schema_key,
                coverage: BuiltinProfileCoverage::VerifiedSubset,
                profile_sha256: CATALOG_26105_SHA256,
            },
            _ => SchemaProviderProvenance::Builtin {
                profile_id: "topsolid-v6-psolid-13006-r1",
                profile_revision: 1,
                schema_key: self.schema_key,
                coverage: BuiltinProfileCoverage::VerifiedSubset,
                profile_sha256: CATALOG_13006_SHA256,
            },
        }
    }
}

fn invalid_catalog_data(catalog_id: &str, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid built-in psolid catalog {catalog_id}: {reason}"),
    )
}

fn invalid_effective_catalog_data(reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid caller-supplied effective schema catalog: {reason}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{BuiltinProfileRegistry, SchemaTypeLookup};

    #[test]
    fn exact_key_provider_retains_effective_catalogs_and_base_profile()
    -> Result<(), Box<dyn std::error::Error>> {
        let catalogs = TopSolidV6PsolidCatalogProvider::new()?;
        for raw_key in SUPPORTED_SCHEMA_KEYS {
            let key = SchemaKey::parse(raw_key)?;
            let catalog_provider = catalogs
                .provider_for_key(&key)
                .ok_or("supported key missing")?;
            assert!(catalog_provider.supports_schema_key(&key));
            assert!(catalog_provider.contains_schema(CATALOG_13006_ID));

            let profile = topsolid_v6_psolid_base_profile(&key)?.ok_or("base profile missing")?;
            let registry = BuiltinProfileRegistry::new(vec![profile])?;
            let profile_provider = registry
                .provider_for_key(&key)
                .ok_or("profile provider missing")?;
            for definition in profile_provider.profile().definitions() {
                assert_eq!(
                    catalog_provider.type_definition(CATALOG_13006_ID, definition.node_type),
                    Some(definition)
                );
            }

            let opaque_type = catalog_provider
                .definitions
                .values()
                .find(|definition| {
                    definition.node_type > 1
                        && definition.fields.iter().any(|field| {
                            field.transmitted && field.field_type == FieldType::OpaquePointer
                        })
                })
                .ok_or("no transmitted opaque-pointer type in base catalog")?;
            assert_eq!(
                catalog_provider.lookup_type(CATALOG_13006_ID, opaque_type.node_type),
                SchemaTypeLookup::Defined(opaque_type)
            );
            assert_eq!(
                profile_provider.lookup_type(CATALOG_13006_ID, opaque_type.node_type),
                SchemaTypeLookup::PresentUnsupported
            );

            for node_type in [189, 190, 191, 192, 193, 201] {
                assert_eq!(
                    catalog_provider.lookup_type(CATALOG_13006_ID, node_type),
                    SchemaTypeLookup::Absent,
                    "base catalog type {node_type} for {raw_key}"
                );
                assert_eq!(
                    profile_provider.lookup_type(CATALOG_13006_ID, node_type),
                    SchemaTypeLookup::Absent,
                    "base profile type {node_type} for {raw_key}"
                );
            }

            if raw_key == "SCH_3600188_36001_13006" {
                assert_eq!(catalog_provider.effective_schema_id, None);
                assert!(!catalog_provider.contains_schema(CATALOG_36001_ID));
                assert!(!catalog_provider.definitions.contains_key(&204));
                assert_eq!(
                    catalog_provider.lookup_type(CATALOG_13006_ID, 204),
                    SchemaTypeLookup::Absent
                );
                for (node_type, roles) in [
                    (14, &["node_id", "surface", "sense"][..]),
                    (17, &["loop", "forward", "backward", "edge", "sense"][..]),
                    (
                        52,
                        &["pvec", "axis", "radius", "sin_half_angle", "cos_half_angle"][..],
                    ),
                ] {
                    let definition = catalog_provider
                        .type_definition(CATALOG_13006_ID, node_type)
                        .ok_or("cone/topology type missing from base catalog")?;
                    for role in roles {
                        assert!(definition.fields.iter().any(|field| field.name == *role));
                    }
                }
            }

            if let Some(effective_schema_id) = catalog_provider.effective_schema_id {
                let effective = catalog_provider
                    .effective_definitions
                    .and_then(|definitions| definitions.values().next())
                    .ok_or("effective catalog empty")?;
                assert_eq!(
                    catalog_provider.lookup_type(effective_schema_id, effective.node_type),
                    SchemaTypeLookup::Defined(effective)
                );
                assert_eq!(
                    profile_provider.lookup_type(effective_schema_id, effective.node_type),
                    SchemaTypeLookup::Unknown
                );
            }
        }
        let unsupported = SchemaKey::parse("SCH_3700193_36001_13006")?;
        assert!(catalogs.provider_for_key(&unsupported).is_none());
        assert!(topsolid_v6_psolid_base_profile(&unsupported)?.is_none());
        Ok(())
    }

    #[test]
    fn compiled_registry_selects_only_topsolid_exact_keys() -> Result<(), Box<dyn std::error::Error>>
    {
        let registry = BuiltinProfileRegistry::compiled()?;
        let catalogs = TopSolidV6PsolidCatalogProvider::new()?;
        for raw_key in SUPPORTED_SCHEMA_KEYS {
            let key = SchemaKey::parse(raw_key)?;
            let provider = registry
                .provider_for_key(&key)
                .ok_or("compiled key missing")?;
            assert_eq!(provider.selected_schema_key(), raw_key);
            assert_eq!(
                provider.profile().metadata().provider_schema,
                CATALOG_13006_ID
            );

            let source = catalogs
                .provider_for_key(&key)
                .ok_or("catalog key missing")?;
            let opaque_type = source
                .definitions
                .values()
                .find(|definition| {
                    definition.node_type > 1
                        && definition.fields.iter().any(|field| {
                            field.transmitted && field.field_type == FieldType::OpaquePointer
                        })
                })
                .ok_or("opaque-pointer base type missing")?;
            assert_eq!(
                provider.lookup_type(CATALOG_13006_ID, opaque_type.node_type),
                SchemaTypeLookup::Defined(opaque_type)
            );
            assert_eq!(
                provider.type_definition(CATALOG_13006_ID, opaque_type.node_type),
                Some(opaque_type)
            );
            assert_eq!(
                provider.lookup_type(CATALOG_13006_ID, 204),
                if matches!(
                    raw_key,
                    "SCH_3300261_32001_13006"
                        | "SCH_3300259_32001_13006"
                        | "SCH_3700192_36001_13006"
                        | "SCH_3600188_36001_13006"
                ) {
                    SchemaTypeLookup::Absent
                } else {
                    SchemaTypeLookup::Unknown
                }
            );
            assert_eq!(provider.provenance(), source.provenance());
            if let Some(effective_schema_id) = source.effective_schema_id {
                let effective = source
                    .effective_definitions
                    .and_then(|definitions| definitions.values().next())
                    .ok_or("effective catalog empty")?;
                assert_eq!(
                    provider.lookup_type(effective_schema_id, effective.node_type),
                    SchemaTypeLookup::Defined(effective)
                );
                assert_eq!(
                    provider.type_definition(effective_schema_id, effective.node_type),
                    Some(effective)
                );
            } else {
                assert_eq!(source.effective_schema_id, None);
                assert_eq!(
                    provider.lookup_type(CATALOG_36001_ID, 52),
                    SchemaTypeLookup::Unknown
                );
            }
        }

        let onshape_key = SchemaKey::parse("SCH_3701212_37102_13006")?;
        let onshape = registry
            .provider_for_key(&onshape_key)
            .ok_or("compiled non-TopSolid key missing")?;
        assert_eq!(onshape.selected_schema_key(), onshape_key.raw());
        assert_eq!(
            onshape.lookup_type(CATALOG_36001_ID, 2),
            SchemaTypeLookup::Unknown
        );
        assert!(matches!(
            onshape.provenance(),
            SchemaProviderProvenance::Builtin { profile_id, .. }
                if profile_id == "onshape-sch37102-13006-r3"
        ));

        let nearby_topsolid_build = SchemaKey::parse("SCH_3700193_36001_13006")?;
        let family_provider = registry
            .provider_for_key(&nearby_topsolid_build)
            .ok_or("TopSolid-family neighboring build is not inferred")?;
        assert_eq!(
            family_provider.profile().metadata().coverage,
            BuiltinProfileCoverage::FamilyInferred
        );
        assert_eq!(
            family_provider.selected_schema_key(),
            nearby_topsolid_build.raw()
        );
        for raw_key in ["SCH_3700192_36001_13007", "SCH_3300259_32001_13007"] {
            let key = SchemaKey::parse(raw_key)?;
            assert!(registry.provider_for_key(&key).is_none());
        }
        Ok(())
    }
}
