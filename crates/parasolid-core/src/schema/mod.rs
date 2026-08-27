//! Base, delta, and complete embedded schema resolution.

mod catalog;
mod coverage;
mod decoder;
mod model;
mod provider;
mod registry;

pub use catalog::{ParsedSchemaCatalog, SchemaCatalogLimits, parse_schema_catalog};
pub use coverage::SchemaCoverageReport;
pub use decoder::{SchemaLimits, decode_embedded_schema};
pub use model::{
    FieldDefinition, FieldType, SchemaEdit, SchemaKey, SchemaResolution, SchemaSource,
    TypeDefinition,
};
pub use provider::{InMemorySchemaProvider, SchemaProvider};
pub use registry::EffectiveSchemaRegistry;
