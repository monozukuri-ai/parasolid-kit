#![no_main]

use libfuzzer_sys::fuzz_target;
use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, InMemorySchemaProvider, InspectionLimits, SchemaKey,
    SchemaSource, TypeDefinition,
    brep::{map_xb_brep_with_diagnostic_limit, map_xt_brep_with_diagnostic_limit},
    inspect_xb, inspect_xt, parse_xb, parse_xt,
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
    if data.len() <= 64 * 1024 {
        use parasolid_core::partial;
        let mut tables = partial::topology::scan(data);
        let _ = partial::native_hierarchy::scan(data, "SCH_3701229_37102_13006", &tables);
        partial::native_fin::normalize_or_withhold(&mut tables);
        let _ = partial::analytic::parse_carrier(data, 0);
        let _ = partial::analytic::parse_carrier(data, usize::MAX);
        let _ = partial::spline::scan_curve_carriers(data);
        let _ = partial::spline::scan_surface_carriers(data);
        let _ = partial::intersection::scan_intersection_carriers(data);
        let _ = partial::blend::scan(data);
        let _ = partial::offset::scan(data);
        let _ = partial::sweep::scan_sweep_carriers(data);
        let _ = partial::subset::scan(data);
    }

    if let Ok(header) = inspect_xb(data, INSPECTION_LIMITS)
        && let Ok(key) = SchemaKey::parse(&header.schema_key)
    {
        if let Ok(registry) = BuiltinProfileRegistry::compiled()
            && let Some(builtin) = registry.provider_for_key(&key)
        {
            if let Ok(document) = parse_xb(data, &builtin, DOCUMENT_LIMITS) {
                let _ = map_xb_brep_with_diagnostic_limit(&document, 256);
            }
        } else if let Some(provider) = provider(&header.schema_key) {
            let _ = parse_xb(data, &provider, DOCUMENT_LIMITS);
        }
    }
    if let Ok(header) = inspect_xt(data, INSPECTION_LIMITS)
        && let Ok(key) = SchemaKey::parse(&header.schema_key)
    {
        if let Ok(registry) = BuiltinProfileRegistry::compiled()
            && let Some(builtin) = registry.provider_for_key(&key)
        {
            if let Ok(document) = parse_xt(data, &builtin, DOCUMENT_LIMITS) {
                let _ = map_xt_brep_with_diagnostic_limit(&document, 256);
            }
        } else if let Some(provider) = provider(&header.schema_key) {
            let _ = parse_xt(data, &provider, DOCUMENT_LIMITS);
        }
    }
});
