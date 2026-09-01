//! Base, delta, and complete embedded schema resolution.

mod catalog;
mod coverage;
mod decoder;
mod model;
mod profile;
pub mod profiles;
mod provider;
mod registry;
mod wire;

pub use catalog::{ParsedSchemaCatalog, SchemaCatalogLimits, parse_schema_catalog};
pub use coverage::SchemaCoverageReport;
pub use decoder::{SchemaLimits, decode_embedded_schema};
pub use model::{
    FieldDefinition, FieldType, SchemaEdit, SchemaKey, SchemaResolution, SchemaSource,
    TypeDefinition,
};
pub use profile::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinProfileRegistry, BuiltinSchemaProfile,
    BuiltinSchemaProvider,
};
pub use provider::{
    InMemorySchemaProvider, SchemaProvider, SchemaProviderProvenance, SchemaProviderResolution,
};
pub(crate) use provider::{
    missing_type_definition_error, unavailable_schema_error, validate_builtin_input,
};
pub use registry::EffectiveSchemaRegistry;
pub(crate) use wire::WireDecodeClass;
