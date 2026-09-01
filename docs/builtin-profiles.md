# Built-in profile provenance

Only the following profile is registered for default parsing. The complete
internal stream key must match; a caller-selected provider always takes priority.

```json
{
  "kind": "builtin",
  "profile_id": "onshape-sch30000-r1",
  "profile_revision": 1,
  "schema_key": "SCH_3000000_30000",
  "coverage": "verified_subset",
  "profile_sha256": "e28a5e11a7713573a7134025bd8c3d83f194fc663662f079e839a95ea5981f80"
}
```

The maintained definitions are in
[`sch30000.rs`](../crates/parasolid-core/src/schema/profiles/sch30000.rs).
The source model's field roles are in
[`profile_roles.rs`](../crates/parasolid-core/src/brep/profile_roles.rs).
The profile is project-owned code, not an embedded Siemens schema catalog.
Build and runtime use neither local evidence files nor a CAD installation.

## Sources and method

The implementation uses the published
[Parasolid XT Format Reference, April 2008](https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf),
particularly printed pages 5–19 (wire format), 31–34, 54–57, 77–78
(analytic geometry), and 87–119 (records and classes). Per-type source comments
remain alongside the code. The PDF is not redistributed in the package.

Version-specific definitions were checked against paired Onshape V30 X_T/X_B
exports and self-describing V30 model edits for types 12, 19, 70, and 74.
Project-owned names identify fields; the raw codec and semantic role tables
remain separate. Class 1040 is declared by observed inputs, without claiming
its complete membership. Type 74 retains generic pointers without inventing a
class constraint. No Siemens catalog was used to generate the table.

Optional catalog comparisons were performed after candidate parsing, as an
independent comparison of complete record boundaries and field values. Those
catalogs were not parser input on the built-in path. Producer body details,
mass properties, simultaneous STEP exports, and isolated STEP reimports supplied
geometry evidence independent of the raw decoder.

## Verified scope

The profile covers Onshape V30 X_T and neutral X_B with zero user fields and
single-solid boxes, polygonal prisms, cylinders, and through-holes. The 22 raw
node types are:

```text
12 13 14 15 16 17 18 19 29 30 31 50 51 70 74 79 80 81 82 83 84 98
```

These contain 179 field groups. The 13 types through 51 in this list provide
B-Rep topology and point/line/circle/plane/cylinder roles; the remaining types
preserve associated lists, attributes, and value arrays. Raw decoding alone
does not imply that all attributes have a public semantic mapping.

Local validation used 13 development pairs and 12 new pairs collected after
freezing the implementation and validation conditions. Across all 50 streams,
7,282 records and 51,142 fields matched the saved boundaries and decoded values.
The frozen holdouts included transforms, dimension changes, a distinct
asymmetric octagonal prism, Unicode body names, and shifted through-holes.
All 25 pairs passed B-Rep and X_T/X_B comparison. Planar exports were validated
through OCCT/STEP reimport. Curved core area/volume remain unavailable, and
vertex-trimmed arc conversion is not extended by this profile.

These are local real-data results. Public CI uses synthetic selection, codec,
limit, packaging, and bounded fuzz tests; it does not contain the real fixtures.
An isolated-install runner accepts separately supplied fixture pairs for local
wheel and sdist validation. The fixtures, catalogs, private evidence, and PDF
are excluded from both distributions.

Other keys/producers, embedded bases, user fields, assemblies, multiple bodies,
sheets, and NURBS are outside this verification claim. Runtime checks use exact
keys, reviewed types, and structural invariants; they do not classify a file's
shape to broaden the claim. See [format support](format-support.md).

## Hash and revision contract

`profile_sha256` hashes the canonical compiled wire definitions, not the Rust
source bytes. The canonical object contains `profile_id`, `revision`, sorted
`schema_keys`, and node types in ascending order. Each type contains
`node_type`, `name`, `variable`, and ordered fields represented as
`[name, codec, pointer_class, element_count, transmitted]`. JSON object keys
are sorted and serialization is compact UTF-8. Descriptions, source paths,
evidence paths, and the digest itself are excluded.

The Rust integration test recomputes this hash. The distribution gate checks
that the reviewed profile, registry, and role source files are present unchanged
in the sdist; isolated wheel and sdist installs verify the reported ID, revision,
exact key, coverage, and hash. Runtime provenance is distinct from per-type
`SchemaSource` and is exposed through `document.schema_resolution` and the
B-Rep summary. `verified_subset` must not be presented as full-schema coverage.
