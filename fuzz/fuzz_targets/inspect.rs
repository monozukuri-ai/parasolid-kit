#![no_main]

use libfuzzer_sys::fuzz_target;
use parasolid_core::{InspectionLimits, inspect_xb, inspect_xt};

const LIMITS: InspectionLimits = InspectionLimits {
    max_file_size: 64 * 1024,
    max_string_bytes: 8 * 1024,
};

fuzz_target!(|data: &[u8]| {
    let _ = inspect_xb(data, LIMITS);
    let _ = inspect_xt(data, LIMITS);
});
