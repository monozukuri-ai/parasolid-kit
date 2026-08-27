#![no_main]

use libfuzzer_sys::fuzz_target;
use parasolid_core::{SchemaCatalogLimits, parse_schema_catalog};

const LIMITS: SchemaCatalogLimits = SchemaCatalogLimits {
    max_file_size: 64 * 1024,
    max_schema_types: 1_024,
    max_fields_per_type: 128,
    max_string_bytes: 8 * 1024,
};

fuzz_target!(|data: &[u8]| {
    let _ = parse_schema_catalog(data, LIMITS);
});
