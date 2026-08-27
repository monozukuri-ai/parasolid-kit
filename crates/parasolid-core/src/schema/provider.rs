//! Base-schema provider boundary.

use std::collections::BTreeMap;

use super::TypeDefinition;

/// Supplies complete standard/base schema definitions without prescribing storage.
pub trait SchemaProvider {
    /// Return whether the named schema catalog is loaded and complete.
    fn contains_schema(&self, schema: &str) -> bool;

    /// Return one type from a loaded schema, or `None` when that type is absent.
    fn type_definition(&self, schema: &str, node_type: u16) -> Option<&TypeDefinition>;
}

/// Deterministic provider used by callers, tests, and future file loaders.
#[derive(Debug, Clone, Default)]
pub struct InMemorySchemaProvider {
    schemas: BTreeMap<String, BTreeMap<u16, TypeDefinition>>,
}

impl InMemorySchemaProvider {
    /// Create an empty provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            schemas: BTreeMap::new(),
        }
    }

    /// Mark a schema catalog as loaded, even when it contains no definitions.
    pub fn add_schema(&mut self, schema: impl Into<String>) {
        self.schemas.entry(schema.into()).or_default();
    }

    /// Insert or replace one definition in a loaded schema catalog.
    pub fn insert(
        &mut self,
        schema: impl Into<String>,
        definition: TypeDefinition,
    ) -> Option<TypeDefinition> {
        self.schemas
            .entry(schema.into())
            .or_default()
            .insert(definition.node_type, definition)
    }
}

impl SchemaProvider for InMemorySchemaProvider {
    fn contains_schema(&self, schema: &str) -> bool {
        self.schemas.contains_key(schema)
    }

    fn type_definition(&self, schema: &str, node_type: u16) -> Option<&TypeDefinition> {
        self.schemas
            .get(schema)
            .and_then(|definitions| definitions.get(&node_type))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaSource;

    #[test]
    fn distinguishes_unloaded_schema_from_absent_type() {
        let mut provider = InMemorySchemaProvider::new();
        assert!(!provider.contains_schema("13006"));

        provider.add_schema("13006");
        assert!(provider.contains_schema("13006"));
        assert!(provider.type_definition("13006", 12).is_none());

        provider.insert(
            "13006",
            TypeDefinition::from_fields(12, "BODY", "Body", Vec::new(), SchemaSource::Base),
        );
        assert_eq!(
            provider
                .type_definition("13006", 12)
                .map(|definition| definition.name.as_str()),
            Some("BODY")
        );
    }
}
