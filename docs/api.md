# Python API

`parasolid_kit` is the supported Python namespace. `parasolid_kit._core` is a
private implementation module and must not be imported by applications. Public
functions validate their arguments and return immutable typed values.

The package is currently pre-alpha. The documented names and behaviors are the
intended public boundary, but backward compatibility is not yet guaranteed
between development releases.

## Inputs and schema selection

`inspect_xb`, `inspect_xt`, `parse_xb`, and `parse_xt` accept a path or a
`bytes`, `bytearray`, or `memoryview` value. They do not accept a file-like
object. Inspection validates only the header and must not be treated as proof
that the node stream or geometry is valid.

Complete parsing uses the schema key inside the X_B/X_T stream. For
`SCH_<modeller>_<effective>` the provider must supply `<effective>`; for
`SCH_<modeller>_<effective>_<base>` it must supply `<base>`. The parser does not
fall back to a nearby version, infer an unknown field layout, or select the
human-oriented common-header `SCH` value in X_T.

```python
from parasolid_kit import DirectorySchemaProvider, parse_xb

provider = DirectorySchemaProvider("/caller/owned/schema")
document = parse_xb("model.x_b", schema_provider=provider)
```

`DirectorySchemaProvider` constructs exactly
`sch_<requested-schema>.sch_txt`, rejects symbolic-link directories/catalogs,
validates the catalog's internal identifier, and caches a successful load. A
missing exact file returns `None` to the parser; it does not scan recursively or
select another version.

The repository and package contain no Siemens schema catalog. Use
`parasolid-kit inspect MODEL.x_b` to read the internal schema key without a
catalog. A key such as `SCH_3000000_30000` requires `sch_30000.sch_txt`; an
embedded-base key such as `SCH_3000310_30000_13006` requires
`sch_13006.sch_txt`. Obtain that exact catalog from an available Parasolid SDK
or Parasolid-based product and pass its containing directory to
`DirectorySchemaProvider` or `--schema-dir`. See
[Schema catalogs](../README.md#schema-catalogs) for acquisition and location
instructions.

## Entry points

| Function | Result | Purpose |
|---|---|---|
| `read_brep(source, *, schema_provider=None, schema_dir=None, source_format="auto", limits=...)` | `ParsedBrep` | Inspect, parse, map, and summarize one X_T/X_B source |
| `inspect_xb(source, *, limits=...)` | `XbHeader` | Validate and inspect an X_B header |
| `inspect_xt(source, *, limits=...)` | `XtHeader` | Validate and inspect an X_T header |
| `load_schema_catalog(path, *, expected_schema_id=None, limits=...)` | `SchemaCatalog` | Load and validate one exact ASCII `sch_*.sch_txt` catalog |
| `parse_xb(source, *, schema_provider=None, limits=...)` | `ParasolidDocument` | Parse a complete X_B node stream |
| `parse_xt(source, *, schema_provider=None, limits=...)` | `ParasolidDocument` | Parse a complete X_T node stream |
| `write_xb(document)` | `bytes` | Byte-exact reconstruction of an unmodified X_B document |
| `compare_documents(left, right, *, absolute_tolerance=..., relative_tolerance=..., max_differences=...)` | `DocumentComparison` | Structural comparison after pointer-index remapping |
| `map_brep(document, *, limits=...)` | `BrepModel` | Validated Parasolid-native topology and geometry model |

## High-level B-Rep result

For normal inspection, `read_brep()` is the shortest complete path:

```python
from parasolid_kit import read_brep

parsed = read_brep(
    "model.x_t",
    schema_dir="/caller/owned/schema",
)

print(parsed.complete)
print(parsed.summary.counts.faces)
print(parsed.summary.curve_kind_counts)
print(parsed.summary.to_dict())
```

`ParsedBrep` contains:

- `document`: the complete `ParasolidDocument` with raw nodes and source bytes;
- `brep`: the complete or explicitly incomplete `BrepModel` source model; and
- `summary`: a compact `BrepSummary` suitable for immediate inspection.

The summary reports the format, modeller and schema identifiers, file/node and
resolved-schema counts, B-Rep completeness, topology/geometry counts, body,
curve and surface kinds, kernel-free metrics, and document/B-Rep diagnostics as
separate lists. Bounding boxes, areas, and volumes remain in source transmit
units; `unit_basis="source_transmit_units"` does not claim a physical unit.

`source_format="auto"` accepts only known `.x_b`/`.xb` and `.x_t`/`.xt`
suffixes or known binary/text signatures. Use `source_format="x-b"` or
`source_format="x-t"` for an ambiguous bytes-like source. `schema_provider` and
`schema_dir` are mutually exclusive. Neither is required when an embedded
schema is independently sufficient for the complete stream.

`ParasolidDocument.format` is `"binary"` or `"text"`. Its `nodes` remain in
physical source order and retain node indices, effective definitions, decoded
field values, exact byte ranges, schema-resolution provenance, terminator, raw
bytes, diagnostics, and schema-coverage data. Public model objects provide
`to_dict()` where a JSON-compatible report is required.

`write_xb` rejects an X_T-derived document. It is a reconstruction API, not a
general editor or serializer for a mutated object graph.

`compare_documents` does not require equal bytes, node order, or physical node
indices. `equivalent` is true only when schema coverage, node-type counts,
topology, and field values agree under the supplied finite non-negative numeric
tolerances. Differences are structured and capped by `max_differences`.

`map_brep` preserves source topology and analytic/NURBS definitions. A valid raw
record whose geometry kind is not decoded remains explicit as
`UnsupportedGeometry`, and `BrepModel.complete` is false. Metrics are returned
only when their exact preconditions are met; curved shapes are not estimated
from endpoints. See [format support and limitations](format-support.md) for the
current geometry coverage.

## Optional interoperability boundary

`parasolid_kit.interop` is importable from the parser-only installation. Its
import does not load or require OCP, CadQuery, or VTK. Runtime selection is
explicit:

```python
from parasolid_kit import interop

OCP = interop.require_occt()
# or, in a separate environment:
cadquery = interop.require_cadquery()
```

Install exactly one profile:

| Profile | Python | Installed runtime | Intended use |
|---|---:|---|---|
| `parasolid-kit[occt]` | 3.10+ | `cadquery-ocp-novtk` | Headless OCCT conversion and STEP export |
| `parasolid-kit[cadquery]` | 3.11+ | CadQuery and full `cadquery-ocp` | CadQuery shapes plus OCCT conversion and STEP export |

The profiles must not coexist because their distributions can provide the same
`OCP` namespace. Before any optional import, both guards inspect installed
distribution metadata. A missing or conflicting profile raises
`InteropDependencyError` with a structured diagnostic and concrete install or
recovery commands; it does not leak a bare `ModuleNotFoundError`.

All package-defined optional failures derive from `InteropError`, which in turn
derives from `ParasolidError`:

- `InteropDependencyError`
- `OcctConversionError`
- `CadQueryConversionError`
- `StepExportError`
- `PreviewError`

`InteropLimits` separately bounds entity/subshape counts, curve samples,
triangles, vertices, output bytes, and retained conversion diagnostics. These
ceilings do not alter `ParseLimits`.

### Strict OCCT conversion

I3 introduced the public kernel adapter; I7 expands its exact geometry coverage:

```python
from parasolid_kit import read_brep
from parasolid_kit.interop.occt import ValidationTolerances, to_occt

parsed = read_brep("model.x_b", schema_dir="/caller/owned/schema")
result = to_occt(
    parsed.brep,
    source_unit="m",
    target_unit="mm",
    validation=ValidationTolerances(
        linear_absolute=1.0e-6,
        relative=1.0e-9,
    ),
)
```

`source_unit` is required and is never inferred from a suffix, model size, or
producer. Supported unit strings are `m`, `cm`, `mm`, `in`, and `ft`; the
default target is `mm`. Coordinates and radii are scaled, while angles,
orientation, and topology IDs are not.

The current exact path covers point, bounded line, vertex-free full circle and
ellipse, bounded parabola and hyperbola branches, explicitly parameterized
trimmed curves, plane, cylinder, cone frustum, untrimmed sphere and ring torus,
open non-periodic non-rational 3D NURBS, and exact offset surfaces under the
constraints reported by `geometry_coverage()`. It constructs shared vertices,
edges, wires, faces,
shells, and bodies directly from `BrepModel`; it does not recreate source
geometry with approximate primitives or Boolean operations. Periodic and
naturally bounded faces introduce topology required by OCCT. Those seam and
boundary edges and vertices are retained as `generated` source-map relations.

```python
from parasolid_kit.interop.occt import geometry_coverage

for item in geometry_coverage():
    print(item.category, item.kind, item.parser, item.occt, item.step)
```

This immutable table is the source for the generated table in
`docs/format-support.md`; tests reject documentation drift.

`OcctConversionResult` owns four related views:

- `.shape`: the root `TopoDS_Shape` runtime object;
- `.subshapes`: conversion-local `occt:<kind>:<number>` keys and owned objects;
- `.source_map`: bidirectional many-to-many `direct`, `split`, `merged`, and
  `generated` relations with source node IDs and byte ranges; and
- `.report`: schema-version-1 options, versions, independent source/conversion/
  OCCT statuses, topology counts, metrics, diagnostics, limits, usage, and
  healing fields.

`result.to_dict()` excludes raw OCP objects and is JSON-compatible. Conversion
keys are local to one result; they are not persistent OCCT IDs. In particular,
a vertex-free Parasolid circle can acquire an OCCT seam vertex, so edge and
vertex counts are validation evidence rather than identity claims.

The default is `require_complete=True, heal=False`; healing remains unavailable.
Direct vertex-trimmed circles/ellipses, rational or non-3D NURBS, pcurves,
intersection curves, blend surfaces, dummy loop topology, unknown orientation,
invalid OCCT shapes, and metric mismatches raise `OcctConversionError` instead
of being silently dropped. Explicit `TrimmedCurve` is supported because it
retains the basis, endpoint positions, and parameter interval needed to choose
one arc. `.report` contains the partial `ConversionReport` available at failure
time. `InteropLimits` is checked before amplified topology/control-net
construction and again on the actual output.

### CadQuery shape adapter

I5 reuses the strict conversion and wraps its body subshapes; it does not
rebuild geometry through CadQuery primitives:

```python
from parasolid_kit.interop.cadquery import to_cadquery, to_cadquery_shapes

shape = to_cadquery(
    parsed.brep,
    source_unit="m",
    target_unit="mm",
)
body_shapes = to_cadquery_shapes(parsed.brep, source_unit="m")

print(type(shape).__name__)
print(shape.BoundingBox(), len(shape.Faces()), shape.Area())
print(sum(solid.Volume() for solid in shape.Solids()))  # solid-only volume
```

`to_cadquery()` returns the most specific CadQuery `Shape` subclass for one
source body and a `cadquery.Compound` for multiple bodies.
`to_cadquery_shapes()` returns an immutable tuple with one shape per source body
in source order. Both functions require the `[cadquery]` profile, forward the
same unit, completeness, healing, tolerance, limit, and source-identity options
to `to_occt()`, and verify validity, face count, bounding box, area, and solid-only
volume against the OCCT result before returning. CadQuery's `Shape.Volume()`
reports a dimensional mass and therefore returns area for a sheet `Face`; the
adapter instead sums `Volume()` only over `shape.Solids()` for this check.

The source model contains no assembly or editable-operation semantics, so the
adapter never infers `cadquery.Assembly`, `cadquery.Workplane`, or feature
history. A CadQuery modeling operation can produce a new shape; the source
map describes only the original conversion and is not claimed to remain valid
after that mutation. Wrapper-specific failures raise `CadQueryConversionError`
and retain the underlying conversion as `.result`.

### Direct AP242 export

I4 writes STEP directly from the conversion result; it does not reconstruct geometry in
the writer and does not route through CadQuery:

```python
from parasolid_kit.interop.occt.step import write_step

exported = write_step(
    result,
    "model.step",
    output_unit="mm",
    validate=True,
)

print(exported.path)
print(exported.sidecar_path)
print(exported.report.validation.passed)
```

`output_unit` accepts `m`, `cm`, `mm`, `in`, or `ft`. The writer copies and
normalizes the conversion shape to OCCT's mm exchange basis before declaring
the requested AP242 unit, so a conversion whose `target_unit` is `m` can be
written in feet without changing physical size. The original
`OcctConversionResult.shape` is not mutated.

The default contract is strict:

- source, conversion, and OCCT validity must all be complete/true;
- existing `.step` and `.conversion.json` paths are rejected unless
  `overwrite=True`;
- the STEP and sidecar are staged in the destination directory and final paths
  are installed only after validation;
- an isolated `python -I` process reads the staged STEP with OCCT and checks
  reader/transfer status, non-emptiness, B-Rep validity, body/face counts,
  bounding box, area, and volume;
- edge/vertex counts and topology IDs are intentionally not compared because
  STEP can represent seam topology differently; and
- STEP plus sidecar bytes are bounded by `InteropLimits.max_output_bytes`.

`validate=False` is an explicit escape hatch and produces
`status="written_unvalidated"`; the CLI validates unless `--no-validate` is
passed. Validation failure raises `StepExportError`, retains a
`status="validation_failed"` report when reimport evidence exists, and leaves
no final output paths.

The default sidecar is `model.step.conversion.json`, schema version 1. It
contains parser/OCP versions, source/schema and source/target/output units,
scale, conversion and reimport statuses, input/output counts and metrics,
limits, healing/split/generated evidence, artifact SHA-256, and the complete
many-to-many source map. Raw `source_identity` is omitted by default to avoid
leaking a path; `sha256:<64 lowercase hex>` is retained as a source hash.
Passing `include_source_identity=True` opts in to the raw identity.

STEP timestamp and generated product labels are normalized, making repeated
exports of the same result and options byte-reproducible. The STEP and sidecar
are each atomically installed, but two filesystem paths cannot form one
portable transaction; consumers can verify the STEP hash recorded by the
sidecar.

### Bounded local preview

I6 consumes the conversion result directly; it does not tessellate a modified CadQuery
object or create a second geometry evaluator:

```python
from parasolid_kit.interop.preview import (
    PreviewOptions,
    create_preview_server,
    write_preview,
)

preview = write_preview(
    result,
    parsed.brep,
    "model.parasolid-preview",
    options=PreviewOptions(
        linear_deflection=0.1,  # conversion target units
        angular_deflection=0.5,  # radians
        include_edges=True,
    ),
)

with create_preview_server(preview.directory) as server:
    print(server.url)
    server.serve_forever()
```

`write_preview()` writes exactly five self-contained files: `index.html`,
`viewer.css`, `viewer.js`, `preview.glb`, and `preview.manifest.json`. Face
triangles and edge line strips are separate deterministic GLB primitives with
24-bit picking IDs. Edge line strips reuse polygons from the same bounded OCCT
face mesh as the triangles; there is no independent unbounded curve sampler.
The manifest reverses every primitive to its
conversion-local subshape key, Parasolid face/edge IDs, source node ID and byte
range, body, surface/curve kind, relation, and matching diagnostic codes.
Source completeness, conversion completeness, and OCCT validity remain three
separate values.

Tessellation is bounded by `InteropLimits.max_triangles`, `max_vertices`,
`max_curve_samples`, `max_occt_subshapes`, `max_diagnostics`, and
`max_output_bytes`. A limit raises `PreviewError` with
`preview.limit_exceeded`; the writer never silently coarsens or drops the mesh.
Missing source mappings are rejected by default.
`PreviewOptions(allow_partial=True)` is an explicit inspection mode: the
manifest lists every missing face/edge and the UI shows a red warning and list.

The static WebGL application is package-owned MIT code with fixed release
hashes. It uses no CDN, VTK, Node.js runtime, inline script, or unsafe
`innerHTML` insertion. The server exposes only the five reviewed resources,
adds restrictive browser headers, binds `127.0.0.1` with an ephemeral port by
default, and rejects any external bind unless `allow_external=True`. It does
not serve the input directory, source path, or raw Parasolid bytes. A path-like
conversion `source_identity` is omitted from the manifest; a valid
`sha256:<64 lowercase hex>` is retained as `source.sha256`.

The full CadQuery profile also supplies OCP to `to_occt()`, `write_step()`, and
the preview writer. STEP and preview generation continue to consume the direct
OCCT result rather than routing through the CadQuery adapter.

## Limits and failures

`ParseLimits` bounds file size, node count, schema type count, fields per type,
string bytes, variable elements, and retained B-Rep diagnostics. The same
relevant limits are enforced again inside Rust before allocation or traversal.

Package-defined failures derive from `ParasolidError`:

- `ParseError` carries a structured `.diagnostic` with stable code, severity,
  location, fatal flag, and details.
- `SchemaError` reports unavailable or invalid schema resolution.
- `LimitExceededError` reports the resource, actual value, and configured
  bound.

Ordinary Python contract errors use `TypeError` or `ValueError`.

## CLI

The installed `parasolid-kit` command and `python -m parasolid_kit` expose the
same interface:

```text
parasolid-kit inspect MODEL.x_b
parasolid-kit check MODEL.x_b --schema-dir /caller/owned/schema
parasolid-kit check MODEL.x_t --schema-dir /caller/owned/schema --json
parasolid-kit parse MODEL.x_t --schema-dir /caller/owned/schema
parasolid-kit parse MODEL.x_b --schema-dir /caller/owned/schema --brep
parasolid-kit compare LEFT.x_t RIGHT.x_b --schema-dir /caller/owned/schema
parasolid-kit export-step MODEL.x_t MODEL.step \
  --schema-dir /caller/owned/schema --source-unit m
parasolid-kit view MODEL.x_t \
  --schema-dir /caller/owned/schema --source-unit m
```

`check` writes a compact human-readable report by default and uses human-readable
errors; `--json` selects JSON stdout/stderr. Existing `inspect`, `parse`,
`compare`, `export-step`, and `view` output remains JSON. Exit status is `0` for a
complete/exported result or equivalent documents, `1` for an incomplete B-Rep
mapping or a valid comparison that is different, and `2` for input, schema,
parse, mapping, conversion, or export errors.
Auto-detection accepts only known suffixes or signatures; ambiguous files
require `--format x-b` or `--format x-t`. Complete parsing loads only the exact
`sch_<provider-schema>.sch_txt` path and never guesses a fallback catalog.
`export-step` additionally requires `--source-unit`; it hashes the source for
the sidecar, converts to an mm OCCT working result, writes AP242 in
`--output-unit` (default `mm`), and rejects existing outputs unless
`--overwrite` is supplied.
`view` also requires `--source-unit`, writes a persistent preview directory,
and serves it on `127.0.0.1` and an ephemeral port. `--no-open` suppresses
browser launch, `--write-only` generates artifacts and exits, and
`--allow-partial` opts into a visibly incomplete preview. `--host` cannot name
a non-loopback interface unless `--allow-external` is also present.

## Non-goals

This package does not parse iCAD `.icd` containers, product structures,
occurrence transforms, visibility, or appearance. A separate iCAD adapter may
extract a bounded Parasolid resource and call this API; the dependency remains
one-way from that adapter to `parasolid-kit`.
