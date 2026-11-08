# Python API

`parasolid_kit` is the supported Python namespace. `parasolid_kit._core` is a
private implementation module and must not be imported by applications. Public
functions validate their arguments and return immutable typed values.

Starting with `0.1.0`, patch releases in the `0.1.x` series preserve the
documented public Python API. Incompatible API changes require a new minor
series. The same patch-compatibility policy applies within `0.2.x`; the
[0.2.0 migration notes](../CHANGELOG.md#migration-from-010) describe changed
profile identities, JSON fields and metric availability. This guarantee does
not apply retroactively to development releases.
Use structured diagnostic codes for matching; human-readable messages may change.

## Inputs and schema selection

`inspect_xb`, `inspect_xt`, `parse_xb`, and `parse_xt` accept a path or a
`bytes`, `bytearray`, or `memoryview` value. They do not accept a file-like
object. Inspection validates only the header and must not be treated as proof
that the node stream or geometry is valid.

Complete parsing selects by the internal stream key. Omitting `schema_provider`
(or passing `None`) selects a compiled profile from the
[shared support matrix](format-support.md#supported-profiles). That table is
the source for exact keys, revisions, hashes, and stage-specific evidence.
A key that no exact profile accepts is read with the derived profile of its
[schema family](format-support.md#schema-families) (`<effective>[-<base>]`,
the key without its modeller build) when any exact profile shares that
family. The document then reports `coverage="family_inferred"`,
`profile_id="family-<family>"` and the requested key; uncovered node types
still fail closed. `builtin_profiles()` lists the exact profiles and the
derived families with their members and base contributors.

The five [legacy iCAD profiles](builtin-profiles.md#legacy-icad-embedded-keys)
use this same default path in Python and Rust. In the released packages,
SPUN_SURF records parse, but `map_brep` reports `geometry.unsupported_surface`
and `complete=False` for documents containing them; the unreleased source maps
them to `SpunSurface` and converts them as surfaces of revolution, with their
surface-parameter curves mapped onto OCCT's (angle, profile) parameters. Raw
success alone does not establish conversion.

The [SolidWorks profile](solidworks-partitions.md) does not apply delta streams
or determine final configuration state. Unsupported delta records fail explicitly.

Each exact profile has `verified_subset` coverage. Non-TopSolid exact profile
declarations are completed by their [schema family](format-support.md#schema-families).
Five observed TopSolid v6 X_T keys select bundled base and effective catalogs
by exact key, independently of generic families; nearby builds may use a generic
family but do not select a TopSolid catalog. The embedded profiles supply the
reviewed 13006 base before applying stream edits. Runtime,
build, and installation require no external catalog; the parser does not use
the network. Development-only catalog comparisons and membership evidence
remain documented. The current source tree adds the exact Onshape internal key
`SCH_3701212_37102_13006` for the analytic, direct NURBS and compound subset documented in the matrix;
this addition is not yet published. See [profile provenance](builtin-profiles.md) for
the separate producer, geometry, and encoding validation scopes.

```python
from parasolid_kit import compare_documents, parse_xb, parse_xt

text = parse_xt("box-v30.x_t")
binary = parse_xb("box-v30.x_b")
assert compare_documents(text, binary).equivalent
print(text.schema_resolution.to_dict())
```

For inputs outside that scope, supply an explicit provider:

```python
from parasolid_kit import DirectorySchemaProvider, parse_xb

provider = DirectorySchemaProvider("/caller/owned/schema")
document = parse_xb("model.x_b", schema_provider=provider)
```

An explicit provider is authoritative, including an empty provider that returns
`None`. It never falls back to built-in definitions. For
`SCH_<modeller>_<effective>` it must supply `<effective>`; for
`SCH_<modeller>_<effective>_<base>` it must supply `<base>`. Embedded data does
not remove that base requirement. The parser never selects a nearby version or
the human-oriented common-header `SCH` value in X_T.

`DirectorySchemaProvider` constructs exactly `sch_<requested-schema>.sch_txt`,
rejects symbolic-link directories/catalogs, validates the internal identifier,
and caches a successful load. A missing exact file returns `None` to the parser;
`read_brep(schema_dir=...)` and the CLI report `FileNotFoundError` for that case.
The repository and package do not contain Siemens catalogs. See
[Schema catalogs](schema-catalogs.md) for the external-provider path.

| Condition | Diagnostic / error |
|---|---|
| Default selection with a key outside every exact profile and schema family | `schema.missing_base_schema`, with an explanation of built-in scope |
| Default selection with an unregistered build of a reviewed schema family | No error; `schema_resolution.coverage == "family_inferred"` |
| Explicit provider returns no catalog | `schema.missing_base_schema` |
| Explicit catalog has no requested type | `schema.missing_type_definition` |
| Standard built-in profile has no reviewed type | `schema.builtin_profile_uncovered_type`, including profile and node type |
| Embedded base-type membership is unknown | `schema.unknown_base_type`, before embedded definition decoding |
| Embedded base type exists but its definition is unavailable | `schema.unsupported_base_type`, before embedded definition decoding |
| Embedded edits replace or remove a required B-Rep role | Raw decoding may succeed; mapping raises `brep.invalid_field` |
| Built-in input has nonzero user fields | No error; the `USFLD_SIZE` words after every node are kept in `RawNode.user_fields` |

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
| `builtin_profiles()` | `tuple[BuiltinProfileInfo, ...]` | The compiled exact profiles followed by the derived schema families with their members and base contributors |

## High-level B-Rep result

For normal inspection, `read_brep()` is the shortest complete path:

```python
from parasolid_kit import read_brep

parsed = read_brep(
    "box-v30.x_t",
)

print(parsed.complete)
print(parsed.summary.counts.faces)
print(parsed.summary.curve_kind_counts)
print(parsed.summary.to_dict())
```

`ParsedBrep` contains:

- `document`: the `ParasolidDocument` with the source bytes, whose raw nodes are
  materialized from the native snapshot on first access;
- `brep`: the complete or explicitly incomplete `BrepModel` source model; and
- `summary`: a compact `BrepSummary` suitable for immediate inspection.

`read_brep` parses and maps in one native call and never builds the Python
node tree unless `document.nodes` is read, so the B-Rep of a large file costs
the Rust snapshot and the model only. A 10.7 MB SolidWorks X_T with 161,585
nodes and 2,949 faces reads in 3.2 s with a peak of 480 MB, against 42 s and
3.5 GB when the node tree was built eagerly; materializing its nodes afterwards
still takes about 44 s and 2.7 GB more. `parse_xt` and
`parse_xb` return the same lazy document.

The summary reports the format, modeller and schema identifiers, file/node and
resolved-schema counts, B-Rep completeness, topology/geometry counts, body,
curve and surface kinds, kernel-free metrics, and document/B-Rep diagnostics as
separate lists. Bounding boxes, areas, and volumes remain in source transmit
units; `unit_basis="source_transmit_units"` does not claim a physical unit.

`BrepModel.assemblies` and `BrepModel.instances` return the assembly structure
of a transmit file beside the topology. Each `Assembly` lists the instances it
positions and the instances that reference it; each `Instance` names its part
(a body or an assembly identifier with `part_kind`), its sign, its owning
assembly and the stored `Transform` (rotation rows, translation, scale and the
component flags), whose `apply` evaluates `(rotation · x + translation) · scale`.
The transforms are not applied to the bodies' geometry by `map_brep`; the
entity counts and the `check` output include both collections, and
`to_occt(..., place_instances=True)` places the copies (below).

`BrepModel.attributes` lists the `ATTRIBUTE` nodes with the identifier of
their definition (`SDL/TYSA_COLOUR`, `SDL/TYSA_NAME`, a producer's own
name), the definition's `type_id`, the owning entity (`AttributeOwner`: a
mapped body, face, edge, vertex, region, shell, loop, assembly, instance,
point, curve or surface, or the raw node of an unmapped owner such as a fin)
and the fields as `AttributeField` values: integers, reals, tags, text,
unicode text, points, vectors, directions or axes (two vectors each), or an
unsupported value node kept by name. Bodies and faces list the
ids of their own attributes; `attributes_of(kind, id)`, `colour_of(kind, id)`
(the three reals of the system colour, 0 to 1) and `name_of(kind, id)` read
them. An attribute whose definition, identifier or owner pointer does not
resolve fails the mapping; the attribute chains of the nodes themselves are
not followed.

`ParsedBrep.complete`, `BrepSummary.complete`, and `BrepModel.complete` describe
the same source-mapping stage. A complete model may still have unavailable
curved metrics or unsupported OCCT conversion. It does not certify all
attribute semantics, numerical curve/surface evaluation, applied SolidWorks
deltas, or the final saved configuration. The
[result-stage contract](format-support.md#result-stages-and-caller-responsibilities)
also applies to raw parsing, comparisons, byte replay, and CLI success.

`source_format="auto"` accepts only known `.x_b`/`.xb` and `.x_t`/`.xt`
suffixes or known binary/text signatures. Use `source_format="x-b"` or
`source_format="x-t"` for an ambiguous bytes-like source. `schema_provider` and
`schema_dir` are mutually exclusive. Omitting both selects the supported exact
built-in profile; unregistered keys require an external provider.

`ParasolidDocument.format` is `"binary"` or `"text"`. Its `nodes` remain in
physical source order and retain node indices, effective definitions, decoded
field values, exact byte ranges, schema-resolution provenance, terminator, raw
bytes, diagnostics, and schema-coverage data. `nodes` is materialized on first
access and then kept; `node_count` is available without materializing it.
Public model objects provide `to_dict()` where a JSON-compatible report is
required.

`ParasolidDocument.schema_resolution` and `BrepSummary.schema_resolution` hold
an immutable `SchemaProviderResolution`. Its `kind` is `"caller_supplied"` or
`"builtin"`; built-in results also expose `profile_id`, `profile_revision`, the
exact `schema_key`, `coverage`, and `profile_sha256`. This identifies the
provider independently of each type's `SchemaSource` (base or embedded).
Both fields are appended with default `None` for older manual constructions;
native parse results always carry the actual resolution.

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

## Dependency-free geometry operations

The following Python APIs operate on parsed immutable source values and do
not import OCCT, CadQuery, or NumPy. Coordinates remain in source transmit
units. They do not alter parser defaults, stored geometry kinds, or B-Rep
mapping completeness.

```python
from parasolid_kit import LineCurve, Vector3, evaluate_curve, sample_curve

line = LineCurve(Vector3(0.0, 0.0, 0.0), Vector3(1.0, 0.0, 0.0))
value = evaluate_curve(line, 2.5)
print(value.point, value.tangent)  # P(t), dP/dt
points = sample_curve(line, start=0.0, end=10.0, count=101)
```

`evaluate_curve(curve, parameter, *, model=None)` accepts a `CurveGeometry`
or its definition. Supported definitions are line, circle, ellipse, parabola,
hyperbola, and nonperiodic 3D NURBS. A `TrimmedCurve` additionally requires its
owning `BrepModel` to resolve its basis; parameters use the stored basis
interval, never a guessed normalized 0–1 range. The tangent is the derivative
with respect to increasing parameter; source curve sense does not reverse it.

`sample_curve(curve, start, end, count=100, *, model=None)` includes both
endpoints and prepares NURBS knots/controls once per call. Spacing is uniform
in parameter, **not arc length**. Descending intervals are supported; counts
must be between 2 and 1,000,000.

`evaluate_surface(surface, u, v)` accepts a `SurfaceGeometry` or plane,
cylinder, sphere, ring torus, or nonperiodic NURBS definition. It returns
`SurfaceEvaluation(point, du, dv, normal)`. NURBS derivatives use the exact
homogeneous derivative and quotient rule, including at active-domain endpoints.
The normal follows `du × dv`; a sphere pole uses its analytic limiting normal.
Face/surface senses and trim containment are not applied. Rational controls
contain homogeneous weighted coordinates and require positive finite weights.

Periodic NURBS forms, cones, intersection/SP_CURVE evaluation, and compound
surface definitions are not yet qualified for these new APIs. Invalid knots,
out-of-domain parameters, unsupported definitions, cyclic trim references,
and singular NURBS normals raise `GeometryEvaluationError`; they never fall
back to a straight chord or an invented normal.

### Assembly occurrences and placement

```python
from parasolid_kit import assembly_occurrences, read_brep, Vector3

parsed = read_brep("assembly.x_t")
for occurrence in assembly_occurrences(parsed.brep):
    print(occurrence.body_id, occurrence.instance_path)
    world_point = occurrence.placement.apply_point(Vector3(1.0, 2.0, 3.0))
```

`assembly_occurrences(model, root_assembly=None, *, max_instances=100_000)`
returns `BodyOccurrence` values. `instance_path` and `assembly_path` run from
root to leaf. Repeated references yield distinct occurrences; with no root
selected, uninstanced bodies also appear once at identity. Graph validation
rejects missing/duplicate IDs, nonreciprocal ownership, cycles, excessive
expansion and paths deeper than 64. Negative-instance signs are retained.

`Placement` stores a row-major 3×4 affine matrix. `from_transform` applies the
source `(R · point + translation) · scale` formula; `compose(local)` applies
the local transform first. `apply_normal` uses inverse transpose, including
nonuniform affine scale and reflection. Singular/nonfinite matrices,
perspective transforms and unknown flag bits raise `AssemblyPlacementError`.

To apply occurrences in the viewer, generate an **unplaced** OCCT conversion
and call `write_preview(converted, model, destination, assembly=True)`, or use:

```bash
parasolid-kit viewer assembly.x_t --source-unit mm --assembly
```

The GLB retains one source-body tessellation; its companion manifest supplies
placements. The viewer expands them within display budgets, transforms
normals, reverses reflected triangle winding, and tracks instance paths for
selection and per-occurrence visibility. The bare GLB alone remains unplaced.
An already-placed OCCT result is rejected to prevent double placement. Shared
or unknown body membership and negative instances require interpretation
beyond this display route and are rejected.

### Stored polylines and embedded meshes

```python
from parasolid_kit import DirectorySchemaProvider, parse_xt, faceted_geometry
from parasolid_kit.interop.preview import write_faceted_preview

document = parse_xt("faceted.x_t", schema_provider=DirectorySchemaProvider("schemas"))
stored = faceted_geometry(document)
preview = write_faceted_preview(stored, "faceted.preview", source_unit="mm")
```

`decode_polyline(document, node_index)`, `decode_embedded_mesh(document,
node_index)`, and `faceted_geometry(document)` are opt-in decoders for explicit
caller-supplied **named** catalogs. They do not infer roles from names in
built-in numeric profiles or consume the bundled TopSolid catalogs through these
Python helpers. The built-in TopSolid mapping is a separate path. Both X_T and
X_B use the same decoded-field contract. These operations are independent of
`map_brep`, so an unsupported or absent BODY does not prevent inspecting these
stored records. `max_pool_values` defaults to 1,000,000 and bounds the aggregate
decoded scalar pool values.

`Polyline` retains source-order points, closure, sense, base parameter and node
provenance. Its `display_points()` applies known sense and explicit closure
without assuming parameter spacing between vertices. Embedded meshes retain
pool encoding, used count, capacity, shift, unused values, normal pools and
indices. Supported storage is double-precision positions, 32-bit integer
indices, and absent or per-corner normal pools. Encoded two-component normal
values are retained without inventing decoded normal directions.

`EmbeddedMesh.triangles(index_origin=None)` selects 0- or 1-based indexing only
when the active position indices make it unambiguous; otherwise pass
`index_origin=0` or `1`. Linked affine transforms and surface sense are applied,
including reflected winding. Non-null transform precision corrections and
unsupported pool layouts are rejected. A mesh with no embedded PSM_MESH remains
inspectable, but cannot produce triangles; associated external files are not
loaded.

```bash
parasolid-kit viewer faceted.x_t --source-unit mm --stored-geometry --schema-dir schemas
# Add --mesh-index-origin 0 or 1 only when the source origin is known.
```

`write_faceted_preview` uses stored triangles and connected line strips,
computes flat triangle normals, and needs no OCCT. `--no-edges` omits polylines.
This is always labelled a **stored geometry subset** with `occt_valid=null`;
it does not establish full-document coverage, B-Rep face trims, watertightness,
solid volume, or kernel validity. Degenerate triangles, missing external data,
ambiguous indices and resource-limit failures stop generation before output
is installed. This route does not combine with `--assembly`, because stored
records alone do not establish topological BODY ownership.

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

| Profile | Platforms | Python | Installed runtime | Intended use |
|---|---|---:|---|---|
| `parasolid-kit[occt]` | Linux, macOS, Windows | 3.10+ | `cadquery-ocp-novtk` | Headless OCCT conversion and STEP export |
| `parasolid-kit[cadquery]` | Linux, macOS | 3.11+ (Intel macOS: 3.11–3.13) | CadQuery and full `cadquery-ocp` | CadQuery shapes plus OCCT conversion and STEP export |

On Windows, `require_cadquery()` and the CadQuery adapters raise
`InteropDependencyError` with code `interop.unsupported_platform` before
importing CadQuery or OCP. This avoids a process-shutdown crash observed with
the tested Windows CadQuery runtime. Use the `[occt]` profile for Windows
conversion, STEP export, and preview.

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

parsed = read_brep("box-v30.x_b")
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
3D NURBS (rational or not; a closed or periodic one is built over its stored
knot vector and checked to meet itself), and exact offset surfaces under the
constraints reported by `geometry_coverage()`. It constructs shared vertices,
edges, wires, faces,
shells, and bodies directly from `BrepModel`; it does not recreate source
geometry with approximate primitives or Boolean operations. Periodic and
naturally bounded faces introduce topology required by OCCT. Those seam and
boundary edges and vertices are retained as `generated` source-map relations.

The [parametric path](v30-parametric-viewer.md) additionally constructs trimmed
analytic/NURBS faces using open surface-parameter curves (rational or not) and
source-identified open intersections. It evaluates source UV curves on their
supports and uses bounded numerical 3D approximation or intersection fitting.
`to_occt()` selects it whenever the analytic path cannot express the source:
surface-parameter or intersection curves, circle or ellipse edges bounded by
vertices, trimmed sphere or torus faces, swept faces (a section curve swept
along a direction), spun faces (a profile revolved about an axis), and NURBS
or offset faces with several loops. A source cone measures its v along the axis, so parameter
curves on cones are mapped onto OCCT's generator parameter. An edge whose
curve passes through a sphere pole or a cone apex, where no pcurve can follow
it (a SolidWorks sphere is two halves cut along a great circle through its
poles), is split there with a generated vertex
(`split_edge_at_surface_singularity`), and an edge lying on the seam of a
periodic surface carries one pcurve per orientation, as a Boolean split of a
sphere leaves it, so two faces of one surface can share it
(`share_seam_pcurves`); FIN trims follow the pieces of a split edge, and the
FINs that two faces of one periodic surface write for a shared seam edge are
served by that same representation. An unbounded
analytic intersection branch is trimmed to its chart extent before a blend
pipe is built around it, and a face whose edges cross in parameter space just
before their shared vertex has its edge tolerances grown over the crossing,
up to the largest tolerance the source declares on the body
(`fix_wire_self_intersection`).
Reports retain construction operations and `occt.parametric_approximation`
warnings when sampled deviation exceeds a source edge tolerance but remains
within the conversion limit. These checks do not certify a continuous
whole-curve error bound.

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

`partial=True` keeps going when a face cannot be built: the face is dropped
with its reason, the solid it bounded comes back as an open shell, and a body
left without a converted face is skipped. The report then has
`conversion_complete=False`, lists `faces_dropped` (face and body ids, reason,
source node), `bodies_failed` and `bodies_partial`, carries the
`occt.faces_dropped` and `occt.bodies_failed` warnings (the first fifty entries
as JSON details), does not compare the source metrics, and reports an invalid
partial shape as a warning instead of failing. A model where every face
builds is validated as in strict mode, but a shape that `BRepCheck` rejects or
whose metrics differ from the source is returned with those findings as
warnings and `occt_valid=False` instead of failing, since the caller asked for
what could be built; a model where no body survives still raises
`occt.construction_failed`, with the dropped faces in the error's report. STEP
export refuses a partial or invalid conversion; the preview accepts a partial
one under `allow_partial=True` (the `viewer --allow-partial` option sets both).

`place_instances=True` returns the bodies positioned by the assembly
structure as transformed copies, one per chain of instances from a root
assembly (an assembly no instance references) down to a body, composing
`(rotation . x + translation) . scale` along the chain with the translation
in the target unit. Bodies no instance references stay where they are. The
source map follows the copies: every relation of a placed body is rewritten
onto the copy's subshapes with the instance chain in its note, and each copy
gains a `generated` relation from its placing instance. The report counts
`instances_placed`, `instances_negative` (negative instances are placed and
counted, not subtracted) and `bodies_unreferenced`; its metrics and validity
describe the placed shape, while the source metrics are still compared with
the bodies as built. A transform whose rows are not orthogonal and of equal
length fails the conversion.
Unsupported UV/intersection variants, blends with unequal offsets, invalid
loop topology, unknown orientation, invalid OCCT shapes, and metric mismatches
raise `OcctConversionError`. In the released packages rational, closed or
periodic NURBS and blends raise it too; the unreleased source builds them
(stored-form NURBS as non-periodic B-splines, rolling-ball and cliff-edge
blends as the pipe of the blend radius around the spine; a cliff-edge blend's
zero-offset support is a construction reference only). Two-dimensional NURBS
are accepted as parameter
curves used by `SP_CURVE`. Circular or elliptical arcs bounded by vertices
select the parametric path, which resolves them from the source orientation
and endpoints; the analytic path alone requires explicit trims.
Explicit `TrimmedCurve` is supported because it
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

The adapter converts bodies unplaced: it applies no instance transform from
`BrepModel.instances` and never infers `cadquery.Assembly`,
`cadquery.Workplane`, or feature history. A CadQuery modeling operation can
produce a new shape; the source
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

The `viewer` command (alias `view`) renders the package's GLB and source manifest
with the bundled three-cad-viewer UI. Generate a preview from the documented OCCT
subset with one optional profile installed. This source-tree viewer can be installed
from the repository root as follows; source builds require Rust 1.88+:

```bash
python -m venv .venv
source .venv/bin/activate
python -m pip install ".[occt]"
parasolid-kit viewer model.x_t --source-unit mm
```

On Windows, use `.venv\Scripts\Activate.ps1` in PowerShell instead of `source`.
Choose the input's actual unit. For an input outside the built-in schema subset,
add `--schema-dir /path/to/catalogs`; viewer support does not expand parser coverage.
The printed JSON contains a loopback `url` and the generated file paths. Open that
URL, and use Ctrl-C to stop serving; the output directory remains on disk.
Use an HTTP URL to view it: opening `index.html` as `file://` does not provide the
module/fetch environment needed to load the adjacent GLB and manifest.

```bash
# Keep a preview without starting a server or launching a browser.
parasolid-kit viewer model.x_t --source-unit mm --write-only --output preview

# Explicitly replace the same directory, serving it without opening a browser.
parasolid-kit view model.x_t --source-unit mm --output preview --overwrite --no-open
```

For Python, I6 consumes the conversion result directly. A complete example is:

```python
from parasolid_kit import read_brep
from parasolid_kit.interop.occt import to_occt
from parasolid_kit.interop.preview import (
    PreviewOptions,
    create_preview_server,
    write_preview,
)

parsed = read_brep("model.x_t")
converted = to_occt(parsed.brep, source_unit="mm", target_unit="mm")
preview = write_preview(
    converted,
    parsed.brep,
    "model.parasolid-preview",
    options=PreviewOptions(
        linear_deflection=0.1,  # conversion target units
        angular_deflection=0.5,  # radians
        include_edges=True,
    ),
)

with create_preview_server(preview.directory) as server:
    print(server.url, flush=True)
    server.open_browser()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
```

To reopen saved files without reconverting, use
`create_preview_server("model.parasolid-preview")` with the same server block.
Serving existing previews requires only the base Python package and a WebGL2
browser. Generation requires OCCT; viewing requires neither Node/npm nor a CAD app.

#### Viewer controls

| Operation | Control |
| --- | --- |
| Orbit / pan / zoom | Drag / Shift-drag / mouse wheel |
| Frame the model | Fit view; preset isometric, top, front or right view |
| Projection and axes | Orthographic and Axes checkboxes |
| Inspect sources | Choose Faces or Edges, then click; successive clicks add selections |
| Clear selections | Escape or Clear selection; Backspace removes the last selection |
| Show a subset | Body, Faces/Edges, Surface and Diagnostic controls; Reset filters restores filters |
| Section display | Select X/Y/Z, move Position, or Reverse the retained side; Off restores the whole display |

Selection details retain the complete source relations, face/edge IDs, source node
ID/type/index and byte range, body membership and matching diagnostic codes.
Body groups represent source membership, including shared/unknown membership;
they do not establish an assembly. Visibility, filter and section changes clear
the selection so hidden elements do not remain in the inspector. Camera changes
retain selection. `--no-edges` omits edge geometry and disables edge picking.

#### Units, partial previews and display limits

`--source-unit` is required and accepts `m`, `cm`, `mm`, `in`, or `ft`.
`--target-unit` defaults to `mm` and uses the same choices. Conversion applies
the scale once; the browser uses those target coordinates without an extra GLB
meter conversion. For example, a 0.04 m source edge becomes 40 mm in a preview
generated with `--source-unit m --target-unit mm`. Linear deflection (default
0.1) and section position use target units; angular deflection (default 0.5)
uses radians. Display colors carry no source-material meaning.

The Source, Conversion and OCCT badges describe separate states. An explicit
`--allow-partial` permits an incomplete source or missing source mappings for
inspection, showing a warning and the known missing entities. If the source is
incomplete without an enumerated missing mapping, the UI still warns that additional
geometry may be absent. It continues to require a complete, OCCT-valid conversion;
unsupported geometry, invalid topology or exceeded limits remain errors. In Python,
use `to_occt(..., require_complete=False)` for an incomplete source and
`PreviewOptions(allow_partial=True)` for the preview. Successful partial generation
returns CLI exit code 0; completeness must be read from the report and UI.

Sections clip the display mesh. Caps are shown only for complete known solids,
and are not exported as new B-Rep or exact section geometry. Measurement, CAD
vertex/whole-solid picking, material editing, animation and inferred assemblies
are outside this viewer. See the [display boundary](format-support.md#viewer-display-boundary).
Load failures, invalid input pairs and WebGL2 failures appear in the error panel.
After a WebGL context loss, reload to recreate the renderer.

#### Artifacts and provenance

`write_preview()` writes exactly five self-contained files: `index.html`,
`viewer.css`, `viewer.js`, `preview.glb`, and `preview.manifest.json`. Face
triangles and edge line strips are separate deterministic GLB primitives with
24-bit picking IDs. Edge line strips reuse polygons from the same bounded OCCT
face mesh as the triangles; there is no independent unbounded curve sampler.
The manifest reverses every primitive to its
conversion-local subshape key, Parasolid face/edge IDs, source node ID and byte
range, body, surface/curve kind, relation, and matching diagnostic codes.
Schema version 1 also includes `bodies: [{"id": ..., "kind": ...}]`, sorted by
body ID, with the source `BrepModel` classification (`solid`, `sheet`, `wire`,
or `general`). Primitive `body_ids` retain the complete membership set;
this grouping does not describe an assembly hierarchy.
Source completeness, conversion completeness, and OCCT validity remain three
separate values.

Tessellation is bounded by `InteropLimits.max_triangles`, `max_vertices`,
`max_curve_samples`, `max_occt_subshapes`, `max_diagnostics`, and
`max_output_bytes`. A limit raises `PreviewError` with
`preview.limit_exceeded`; the writer never silently coarsens or drops the mesh.
Missing source mappings are rejected by default.
`PreviewOptions(allow_partial=True)` is an explicit inspection mode: the
manifest lists known missing face/edge mappings and the UI shows a warning and list.

The static WebGL application bundles `three-cad-viewer@5.0.6` with fixed release
hashes. JS/CSS embed the original notices for three-cad-viewer and Three.js (MIT),
postprocessing (Zlib), and n8ao (ISC package metadata / CC0-1.0 license text).
`asset_bundle.license` is a notice summary preserving that upstream discrepancy,
not a single SPDX expression. It uses no CDN, VTK, or Node.js runtime.
Source and diagnostic values use text DOM APIs; upstream's fixed HTML templates
are covered by the reviewed asset hashes. CSP allows inline style attributes,
but no inline scripts, external connections, or dynamic code evaluation.
The server exposes only the five reviewed resources,
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
parasolid-kit check MODEL.x_b
parasolid-kit check MODEL.x_t --json
parasolid-kit parse MODEL.x_t
parasolid-kit parse MODEL.x_b --brep
parasolid-kit compare LEFT.x_t RIGHT.x_b
parasolid-kit export-step MODEL.x_t MODEL.step \
  --source-unit m
parasolid-kit viewer MODEL.x_t \
  --source-unit m
```

`check` writes a compact human-readable report by default and uses human-readable
errors; `--json` selects JSON stdout/stderr. Existing `inspect`, `parse`,
`compare`, `export-step`, and `viewer`/`view` output remains JSON. Exit status is `0` when
the requested stage succeeds, `1` for an incomplete `check` result or a valid
comparison that is different, and `2` for input, schema,
parse, mapping, conversion, or export errors.
`parse --brep` returns `0` even for an explicitly incomplete mapped model;
inspect `brep.complete` in its JSON or use `check` for a completeness exit code.
`viewer --allow-partial` can also generate its requested partial output with `0`.
Auto-detection accepts only known suffixes or signatures; ambiguous files
require `--format x-b` or `--format x-t`. The schema directory is optional for
the built-in subset. An explicit
`--schema-dir` selects only the exact `sch_<provider-schema>.sch_txt` catalog.
`compare` resolves each input by its own internal key.
`export-step` additionally requires `--source-unit`; it hashes the source for
the sidecar, converts to an mm OCCT working result, writes AP242 in
`--output-unit` (default `mm`), and rejects existing outputs unless
`--overwrite` is supplied.
`viewer` (`view` alias) also requires `--source-unit`, writes a persistent preview directory,
and serves it on `127.0.0.1` and an ephemeral port. `--no-open` suppresses
browser launch, `--write-only` generates artifacts and exits, and
`--allow-partial` opts into a visibly incomplete preview. `--host` cannot name
a non-loopback interface unless `--allow-external` is also present.

## Non-goals

This package does not parse iCAD `.icd` containers, product structures,
occurrence transforms, visibility, or appearance. A separate iCAD adapter may
extract a bounded Parasolid resource and call this API; the dependency remains
one-way from that adapter to `parasolid-kit`.
