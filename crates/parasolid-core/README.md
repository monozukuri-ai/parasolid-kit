# parasolid-core

Python-independent Rust parser for Parasolid `X_B` and `X_T` transmit data, with
exact schema selection, bounded raw-node decoding, source byte ranges, and a
Parasolid-native B-Rep model. This crate is part of
[parasolid-kit](https://github.com/monozukuri-ai/parasolid-kit).

```toml
[dependencies]
parasolid-core = "=0.1.0-dev5"
```

Inspect a header without claiming that the geometry is supported:

```rust
use parasolid_core::{InspectionLimits, inspect_xb};

# fn inspect(bytes: &[u8]) -> Result<(), parasolid_core::ParseError> {
let header = inspect_xb(bytes, InspectionLimits::default())?;
println!("{}: nodes start at {}", header.schema_key, header.header_range.end);
# Ok(())
# }
```

For a supported exact key, select its compiled provider before parsing:

```rust
use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, InspectionLimits, SchemaKey,
    brep::map_xb_brep, inspect_xb, parse_xb,
};

# fn parse(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
let header = inspect_xb(bytes, InspectionLimits::default())?;
let key = SchemaKey::parse(&header.schema_key)?;
let registry = BuiltinProfileRegistry::compiled()?;
let provider = registry.provider_for_key(&key).ok_or("unsupported exact schema key")?;
let document = parse_xb(bytes, &provider, DocumentLimits::default())?;
let brep = map_xb_brep(&document)?;
println!("{} bodies; complete={}", brep.bodies.len(), brep.complete);
# Ok(())
# }
```

The compiled profiles are verified subsets for `SCH_3000000_30000`,
`SCH_1300000_13006`, `SCH_3000310_30000_13006`, and `SolidWorks` partition key
`SCH_3701229_37102_13006`. A supported header, schema key,
or raw record does not imply complete geometric support. An explicit
`SchemaProvider` can supply an exact external catalog; no nearby-version
fallback is performed. `SolidWorks` containers and configuration selection belong
to the caller. `SolidWorks` deltas are unsupported even when their header uses
the same supported partition key; no final saved configuration is reconstructed.

Lengths and identifiers retain their Parasolid source meaning. Byte ranges are
relative to the complete byte slice supplied to the parser. Embedding applications
must track container/decompression offsets and any unit or public-ID conversion.
`write_xb` returns the retained original bytes, not an encoder for edited values.

The parser requires Rust 1.88 or newer and has no runtime dependencies, Python,
CAD installation, or network requirement. OCCT, STEP export, and preview adapters
are separate optional Python functionality in parasolid-kit. See the repository's
[format support](https://github.com/monozukuri-ai/parasolid-kit/blob/main/docs/format-support.md)
and [profile provenance](https://github.com/monozukuri-ai/parasolid-kit/blob/main/docs/builtin-profiles.md)
for the verified boundaries.

Licensed under MIT. Real CAD fixtures and external schema catalogs are not
included in this crate.

## Releasing

The Rust crate is published independently of Python artifacts. After updating
the workspace version and verifying the package, maintainers can run
`cargo publish -p parasolid-core --locked --dry-run`, followed by the same command
without `--dry-run`. The manual **Rust core release** workflow runs these gates;
its default is a dry run and upload requires `CARGO_REGISTRY_TOKEN` in repository
secrets. Python release events do not republish an existing Rust version.
