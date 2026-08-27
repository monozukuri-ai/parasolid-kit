//! Python-independent parsing core for Parasolid transmit data.

#![allow(clippy::module_name_repetitions)]

pub mod brep;
mod comparison;
mod document;
mod error;
mod header;
mod reader;
pub mod schema;
mod text_document;
mod text_reader;

pub use comparison::{
    ComparisonDifference, ComparisonOptions, DocumentComparison, compare_xb_documents,
    compare_xb_xt_documents, compare_xt_documents, compare_xt_xb_documents,
};
pub use document::{
    DocumentLimits, FieldValue, RawField, RawNode, XbDocument, XbTermination, parse_xb, write_xb,
};
pub use error::{ErrorDetails, ErrorKind, ParseError};
pub use header::{InspectionLimits, XbBinaryFormat, XbHeader, inspect_xb};
pub use reader::BinaryReader;
pub use schema::{
    EffectiveSchemaRegistry, FieldDefinition, FieldType, InMemorySchemaProvider,
    ParsedSchemaCatalog, SchemaCatalogLimits, SchemaCoverageReport, SchemaEdit, SchemaKey,
    SchemaLimits, SchemaProvider, SchemaResolution, SchemaSource, TypeDefinition,
    decode_embedded_schema, parse_schema_catalog,
};
pub use text_document::{XtDocument, XtHeader, XtTermination, inspect_xt, parse_xt};
