# parasolid-core

Python-independent Rust parser for Parasolid `X_B` and `X_T` transmit data, with
exact schema selection, bounded raw-node decoding, source byte ranges, and a
Parasolid-native B-Rep model. This crate is part of
[parasolid-kit](https://github.com/monozukuri-ai/parasolid-kit).

```toml
[dependencies]
parasolid-core = "=0.1.0"
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

For a supported key, select its compiled provider before parsing. An exact
profile is selected when one accepts the complete key; otherwise the provider
derived for the key's schema family (the key without its modeller build) is
selected and the document reports `family_inferred` coverage:

```rust
use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, InspectionLimits, SchemaKey,
    brep::map_xb_brep, inspect_xb, parse_xb,
};

# fn parse(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
let header = inspect_xb(bytes, InspectionLimits::default())?;
let key = SchemaKey::parse(&header.schema_key)?;
let registry = BuiltinProfileRegistry::compiled()?;
let provider = registry.provider_for_key(&key).ok_or("unsupported schema key")?;
let document = parse_xb(bytes, &provider, DocumentLimits::default())?;
let brep = map_xb_brep(&document)?;
println!("{} bodies; complete={}", brep.bodies.len(), brep.complete);
# Ok(())
# }
```

`map_xt_brep` and `map_xb_brep` remain strict for assembly and attribute metadata.
The opt-in `map_xt_brep_with_metadata_recovery` and
`map_xb_brep_with_metadata_recovery` preserve mapped geometry when a metadata
group is malformed, skip that group, and report the loss through bounded
`BrepModel.diagnostics`. Standalone assembly and attribute decoders remain strict.

The compiled profiles are listed in the
[shared support matrix](https://github.com/monozukuri-ai/parasolid-kit/blob/main/docs/format-support.md#supported-profiles),
with exact keys, revisions, hashes, and separate raw/B-Rep/geometry evidence.
Five observed TopSolid v6 X_T keys use bundled base and effective schema
catalogs; they cover a verified subset and do not provide general TopSolid
package or model decoding. A supported header, schema key, or raw record does
not imply complete geometric support. An explicit
`SchemaProvider` can supply an exact external catalog. Within the built-in
registry, only the modeller build may differ from a reviewed key: another
schema revision or base is never substituted. `SolidWorks` containers and configuration selection belong
to the caller. `SolidWorks` deltas are unsupported even when their header uses
the same supported partition key; no final saved configuration is reconstructed.
`BrepModel.complete` describes source mapping for that document. It does not
certify interpreted attributes, numerical geometry evaluation, available curved
metrics, or optional conversion. iCAD container extraction and saved-state
selection belong to the caller; this crate consumes extracted streams.

The exact iCAD embedded and standard profiles also use `BuiltinProfileRegistry::compiled()`.
Their extra base types are scoped to the exact keys in the support matrix; a
standard profile declares only the node types checked under its key.
`SPUN_SURF` (68) preserves raw parameters and yields an unsupported surface with
`complete == false`; `BLEND_BOUND` (59), its blend dependency, and `SP_CURVE` (137)
use the qualified source B-Rep mappings. This does not add OCCT conversion.

`SurfaceKind::SweptWithSource` retains a swept surface's source direction and
optional scale. Geometric evaluation still requires the direction to be a unit
vector and rejects non-unit values; the retained scale is metadata only and is
never applied to the direction or geometry.
Generic tessellation also fails closed for `SweptWithSource` faces whose trims
lack explicit UV curves: their projected 3D boundary residual has no justified
source-tolerance bound. Existing `Swept` faces retain their projected fallback.

The candidate intersection evaluator handles the supported rolling-ball blend
offset-contact cases and uses exact UV chart stations when they are present in
the mapped chart. These bounded cases do not imply general APPLIO coverage or
parity with the Parasolid kernel.

Lengths and identifiers retain their Parasolid source meaning. Byte ranges are
relative to the complete byte slice supplied to the parser. Embedding applications
must track container/decompression offsets and any unit or public-ID conversion.
`write_xb` returns the retained original bytes, not an encoder for edited values.

The parser requires Rust 1.88 or newer and has no runtime dependencies, Python,
CAD installation, or network requirement. Built-in runtime, build and installation
require no external catalog. Development-only catalog comparisons and type-204
membership evidence remain described in the profile provenance; no catalog
field layouts are generated into the built-in definitions by those comparisons.
OCCT, STEP export, and preview adapters
are separate optional Python functionality in parasolid-kit. See the repository's
[format support](https://github.com/monozukuri-ai/parasolid-kit/blob/main/docs/format-support.md)
and [profile provenance](https://github.com/monozukuri-ai/parasolid-kit/blob/main/docs/builtin-profiles.md)
for the verified boundaries.

Real CAD fixtures and external schema catalogs are not included in this crate.
See the license and partial-reader provenance below.

Maintainer sanitizer and measurement procedures, including the distinction
between parser and caller resource bounds, are documented in
[resource validation](https://github.com/monozukuri-ai/parasolid-kit/blob/main/docs/resource-validation.md).

## Rust API compatibility

Starting with the first stable `0.1.0` release, patch releases in the `0.1.x`
series will preserve source compatibility for documented public Rust APIs,
including the public document/B-Rep models and `partial` reader types. Breaking
changes require a new minor series. Prereleases such as `0.1.0-dev6` remain
development releases; this policy does not retroactively make them stable.

Compatibility includes source units, IDs, byte-range origins and the distinction
between strict document parsing and bounded partial recovery described above and
in [PARTIAL_READERS.md](PARTIAL_READERS.md). Fixing acceptance of invalid input or
incorrectly interpreted values is allowed; such changes must be documented and
covered by regression tests. New profiles or recovered carriers may change
results, diagnostics and coverage, so callers must inspect the reported support
and completeness rather than assume a fixed count or treat partial recovery as
full decoding. Human-readable diagnostic text is not a stable matching key.

## Releasing

The Rust crate is published independently of Python artifacts, using the same
verified candidate commit. Follow the
[release verification procedure](https://github.com/monozukuri-ai/parasolid-kit/blob/main/docs/releasing.md).
The manual **Rust core release** workflow requires a private verification receipt
and checks the candidate artifact hashes. Its default is a dry run; uploading
requires `CARGO_REGISTRY_TOKEN` in repository secrets. Python publication checks
the published Rust checksum and reuses the verified Python artifacts.

For existing embedded fragment integrations, `parasolid_core::partial` provides
bounded topology, analytic/NURBS, and procedural-carrier readers in source units.
It does not replace schema-aware parsing or establish complete delta semantics.
See [the API boundary, provenance and licenses](PARTIAL_READERS.md).

The original implementation is MIT licensed; adopted partial readers are
Apache-2.0 licensed. This crate declares `MIT AND Apache-2.0` and distributes both
license texts.
