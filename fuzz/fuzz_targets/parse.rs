#![no_main]

use libfuzzer_sys::fuzz_target;
use parasolid_core::{
    DocumentLimits, InMemorySchemaProvider, InspectionLimits, SchemaKey, SchemaSource,
    TypeDefinition, inspect_xb, inspect_xt, parse_xb, parse_xt,
};

const INSPECTION_LIMITS: InspectionLimits = InspectionLimits {
    max_file_size: 64 * 1024,
    max_string_bytes: 8 * 1024,
};

const DOCUMENT_LIMITS: DocumentLimits = DocumentLimits {
    max_file_size: 64 * 1024,
    max_nodes: 1_024,
    max_schema_types: 1_024,
    max_fields_per_type: 128,
    max_string_bytes: 8 * 1024,
    max_variable_elements: 4 * 1024,
};

fn provider(schema_key: &str) -> Option<InMemorySchemaProvider> {
    let schema = SchemaKey::parse(schema_key)
        .ok()?
        .provider_schema()
        .to_owned();
    let mut provider = InMemorySchemaProvider::new();
    provider.add_schema(schema.clone());
    for node_type in 2..=128 {
        provider.insert(
            schema.clone(),
            TypeDefinition::from_fields(
                node_type,
                format!("FUZZ_{node_type}"),
                "empty fuzz definition",
                Vec::new(),
                SchemaSource::Base,
            ),
        );
    }
    Some(provider)
}

fuzz_target!(|data: &[u8]| {
    if let Ok(header) = inspect_xb(data, INSPECTION_LIMITS)
        && let Some(provider) = provider(&header.schema_key)
    {
        let _ = parse_xb(data, &provider, DOCUMENT_LIMITS);
    }
    if let Ok(header) = inspect_xt(data, INSPECTION_LIMITS)
        && let Some(provider) = provider(&header.schema_key)
    {
        let _ = parse_xt(data, &provider, DOCUMENT_LIMITS);
    }
});
