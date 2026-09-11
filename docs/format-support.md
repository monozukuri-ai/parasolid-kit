# Format support and limitations

`parasolid-kit` is a pre-alpha, read-focused Parasolid transmit-file parser.
This page describes the supported public behavior; it is not a claim of
compatibility with every Parasolid version, producer, or geometry type.

## File operations

| Operation | X_B | X_T |
|---|---:|---:|
| Inspect the common and internal header | Supported | Supported |
| Parse a complete node stream | Built-in subset or exact external schema | Built-in subset or exact external schema |
| Preserve source bytes and byte ranges | Supported | Supported |
| Reconstruct an unmodified document | Byte-exact | Not supported |
| Compare decoded documents | Supported | Supported |
| Map to the typed B-Rep source model | Supported subset | Supported subset |
| Compact `read_brep`/`check` summary | Supported | Supported |
| Optional OCCT conversion | Documented exact I7 subset | Documented exact I7 subset |
| Optional AP242 export with cold reimport | Documented I7/OCCT subset | Documented I7/OCCT subset |
| Optional CadQuery shape adapter | Documented I7/OCCT subset | Documented I7/OCCT subset |
| Optional GLB/local viewer with source picking | Documented I7/OCCT subset | Documented I7/OCCT subset |

Inputs can be filesystem paths or `bytes`, `bytearray`, and `memoryview`
values. File-like objects are not accepted. Inspection validates the header
only; it does not prove that the remaining node stream or geometry is valid.

## Schema selection and built-in scope

### Supported profiles

This is the shared support matrix for the Python, Rust, and CLI entry points.
Default parsing selects one compiled profile by the complete internal stream
key. Every profile has `verified_subset` coverage and requires zero user fields.
The type and field counts describe compiled base definitions; embedded input
definitions can change the effective layout. Profile hashes identify the
reviewed definitions, not the bytes or geometric correctness of an input.

| Exact internal key | Profile / revision | Base types / field groups | Canonical SHA-256 |
|---|---|---|---|
| `SCH_3000000_30000` | `onshape-sch30000-r2` / 2 | 24 / 202 | `adce41a88ebc4179212519144a5a627dba8d0b6572e3d77ac709f0b16840657f` |
| `SCH_1300000_13006` | `onshape-sch13006-r6` / 6 | 40 / 327 | `2748e9f9c28fa32b59edb9e0b16fea7a976ad081f698dd3d098d9cbd17e08fbb` |
| `SCH_3000310_30000_13006` | `icad-sch30000-13006-r5` / 5 | 30 / 240 | `1f090c87aef63e99af8dcb3aef9cc077a613af6749f177392989cca70ca3bfa5` |
| `SCH_3701229_37102_13006` | `solidworks-sch37102-13006-r1` / 1 | 41 / 338 | `5e05a32681cfa8bf4a3fb029124eb6fb2cf6e34e021f7ed7b60c3df654a9c480` |

The following cells summarize recorded local validation, not a new CAD capture
or a guarantee for every input with the same key. Real fixtures are outside
public CI. The detailed evidence and its retained failures are in
[profile provenance](builtin-profiles.md) and
[SolidWorks partitions](solidworks-partitions.md).

| Profile scope | Raw decoding | Source B-Rep | Independent geometry evidence | OCCT / STEP / preview | Native saved state |
|---|---|---|---|---|---|
| Onshape V30 | Paired X_T / neutral X_B for the documented basic solids, spheres and elliptical edges | Topology and supported analytic definitions | Producer / STEP comparisons; oblique-cut strict mass-property failures remain recorded | Planar box/prism path validated; other inputs must satisfy the adapter constraints | CAD container/configuration reconstruction is outside the parser |
| Onshape V13 | Paired X_T / neutral X_B, including the documented NURBS / SP_CURVE campaigns | Topology, analytic and bounded NURBS / wrapper definitions | Producer / STEP and sampled surface evidence; recorded curve-tolerance failures remain | Existing adapter subset only; rational/periodic NURBS, SP_CURVE and intersection evaluation are outside the public adapter | CAD container/configuration reconstruction is outside the parser |
| iCAD embedded V30 | 17 neutral X_B streams from one container; embedded X_T tests are synthetic | All 17 reach complete source B-Rep/topology | No independent CAD/STEP oracle; line-trim endpoint checks are internal consistency evidence | No real iCAD conversion validation; source B-Rep success does not establish conversion | `.icd` extraction and saved-state selection are caller responsibilities; not established by these streams |
| SolidWorks 2026 partitions | Four neutral X_B partitions; associated deltas stop at unknown base type 3 | Four complete partition B-Reps, including one three-body partition | Existing sldkit point/FIN/NURBS parity; no independent CAD/STEP evaluation repeated for this profile | No real partition conversion validation | Delta application and final saved configuration reconstruction remain unsupported |

Runtime, build, and installation of the built-in parser require no external
schema catalog or CAD installation. Normal build/package dependencies are
separate from parser runtime inputs. The implementation does not download
catalogs. Development provenance includes optional catalog comparisons and a
one-time catalog-header audit for type 204's absence from base 13006; see
[that evidence boundary](builtin-profiles.md#embedded-intersection-data-revision-4).
No catalog field layouts are generated into the profiles by those comparisons.

### Detailed verified scope

For V30, default parsing uses `onshape-sch30000-r2` revision 2 only for the exact internal
key `SCH_3000000_30000`. Its coverage is `verified_subset`: Onshape V30 text and
neutral binary exports, zero user fields, and single-solid boxes, prisms,
cylinders, through-holes, spheres, and verified solids with elliptical edges.
The same producer with a different modeller/key
component is not selected. See [profile provenance](builtin-profiles.md) for
sources, canonical hash, covered types, and local evidence.

The V30 built-in raw decoder covers 24 node types / 202 field groups, including
associated lists and attributes. The B-Rep role mapping covers 15 topology and
point/line/circle/ellipse/plane/cylinder/sphere types. Decoding an attribute record does not
mean the high-level model exposes its semantic meaning. A successful raw parse
is separate from a complete B-Rep and from successful OCCT/STEP conversion.
For the curved solid fixtures, core area/volume remain unavailable; the built-in
profile does not extend the adapter's arc-trimming coverage. Planar box/prism
exports are the validated built-in STEP/preview path.

Default parsing also accepts `SCH_1300000_13006` with `onshape-sch13006-r6`
and `SCH_3000310_30000_13006` with `icad-sch30000-13006-r5`. They share the
reviewed 13006 base subset of 30 types / 240 base field groups, including cone
(52), intersection (38), chart (40), limit (41), trimmed curve (133) and shared
geometry owner (141), and retain intersection chart/limit references.
The V13 profile additionally covers SP_CURVE (137), B_CURVE (134), NURBS_CURVE
(136), CURVE_DATA (135), BSPLINE_VERTICES (45), KNOT_MULT (127), and KNOT_SET
(128), plus B_SURFACE (124), NURBS_SURF (126), and SURFACE_DATA (125):
40 types / 327 field groups. It maps surface/parameter-curve references
and preserves homogeneous control coefficients, distinct knots and multiplicities.
CURVE_DATA and SURFACE_DATA remain raw metadata. Producer evidence includes
open degree-1 nonrational UV curves on planes and cylinders, degree-2 open and
periodic cylinder UV curves, and degree-3 rational periodic UV boundaries on
nonrational bilinear B-surfaces (planar and nonplanar). A separate
[surface campaign](nurbs-surface-evidence.md) verifies degree-3-by-2 open
nonrational/rational patches and nonrational periodic sheets in each axis,
including nonuniform knots and overlapping control rows or columns. A further
[rational periodic campaign](rational-periodic-surface-evidence.md) verifies
weighted periodic sheets in each axis, including seam positions and derivatives.
A [doubly periodic campaign](doubly-periodic-surface-evidence.md) verifies
degree-3-by-2 nonrational/rational sheets periodic in both axes, including their
shared control-grid overlap and seam intersections. Control coefficients,
homogeneous weights, imaginary knots and periodic flags remain as stored.
Malformed knot multiplicities and empty active parameter domains are rejected.
Cylinder angles remain unwrapped: an open UV line spanning one turn can describe
a closed 3D curve. The cylindrical campaign passes parsing and parameter checks,
but one curve per model exceeds the imported STEP edge tolerance by about 1.03%.
The later high-degree campaign stays within the STEP files' declared distance
accuracy, but exceeds the imported per-edge tolerances; both results remain
explicit. This does not establish general spline evaluation or STEP reconstruction.
The [STEP accuracy audit](step-accuracy.md) attributes these saved discrepancies
to existing tolerant FIN/EDGE geometry and STEP boundary approximation, and
separately measures imported pcurve consistency and area integration.
The separate Onshape V30 profile stays at revision 2.
The `TrimmedCurve` model retains the basis curve, endpoints and parameters.
V13 type 133 is verified using new producer text/binary pairs and independent
STEP comparisons; its scope is still the exact V13 key.
The embedded profile applies each transmitted definition/delta and retains its
source, edits, and byte range. Unknown base membership stops before decoding
the definition; an unimplemented type is never treated as absent. Revision 4
of the embedded profile explicitly classifies type 204 as absent from 13006,
so its full input-defined fields can be decoded. Its intersection-data pointer
is exposed as a source reference; numeric UV semantics remain uninterpreted.
B-Rep roles follow fields copied from the reviewed base, so insertions may
shift wire positions without assigning semantics to inserted names. The
optional `intersection_data` reference has a separate reviewed Append contract
requiring a transmitted scalar pointer of class 204.

V13 validation covers paired Onshape text/neutral binary exports of boxes,
spheres, cones/frustums, solids with elliptical edges, and intersecting-cylinder
booleans. Intersection curves remain source models: their point arrays identify
the branch; numerical curve evaluation, core metrics, and optional OCCT export
are not added. Embedded real-data validation uses
neutral binary streams extracted locally from one iCAD file; embedded text
has synthetic decoder and B-Rep tests, without a real paired producer export.
This does not add `.icd` container support or an independent geometry oracle
for its extracted streams. See [profile provenance](builtin-profiles.md).
The existing 17 extracted iCAD V30 streams now all reach complete raw parsing
and source B-Rep/topology. The 15 observed trims reference lines; their endpoints
agree with evaluation of the stored basis and parameters. This consistency check
does not establish general trimmed-curve evaluation or OCCT export support.

The V13 NURBS and sheet evidence is limited to the degrees and motifs in the
linked campaigns. It does not extend the V30 or iCAD profiles, establish arbitrary
NURBS compatibility, or add a public evaluator or rational/periodic OCCT path.
Other producers/keys or embedded bases, nonzero built-in user fields, and
assembly semantics remain outside these verified subsets. Onshape/iCAD evidence
does not establish general multiple-body support. The four SolidWorks partitions
include a three-body example, without establishing arbitrary multi-body or
configuration compatibility. Structural checks
control parsing; matching a key is not a guarantee that every shape emitted
under that key has been verified.

An explicit `SchemaProvider` or `--schema-dir` selects the caller's catalog
without fallback. For `SCH_<modeller>_<effective>` the required catalog is
`<effective>`; for `SCH_<modeller>_<effective>_<base>` it is `<base>`, even when
embedded definitions are present. `DirectorySchemaProvider` considers only
`sch_<provider-schema>.sch_txt`, rejects symbolic links, and does not recurse or
substitute a nearby version. The human-readable common-header `SCH` in X_T is
not used for selection. [Schema catalogs](../README.md#schema-catalogs) describes
the external-provider path.

Unsupported default keys use `schema.missing_base_schema`; uncovered built-in
standard-schema types use `schema.builtin_profile_uncovered_type`. Embedded
membership failures use `schema.unknown_base_type` or
`schema.unsupported_base_type`; nonzero built-in user fields
use `node.unsupported_user_fields`. None causes a guessed layout or a different
profile to be selected. Siemens catalogs and native CAD fixtures are excluded
from wheel/sdist; runtime code does not download them.

## Result stages and caller responsibilities

Success applies to the requested stage and the supplied stream. In particular,
`ParsedBrep.complete`, `BrepSummary.complete`, and `BrepModel.complete` describe
source B-Rep mapping. They do not certify interpreted attributes, a public
geometry evaluator, successful conversion, or the final saved state of a CAD
container. `complete=True` can coexist with unavailable curved metrics or
unsupported OCCT conversion.

| Entry point / stage | Meaning of success | Failure or remaining boundary |
|---|---|---|
| `inspect_xb` / `inspect_xt`; CLI `inspect` | Header validated | Node stream and geometry remain unvalidated |
| `parse_xb` / `parse_xt`; CLI `parse` | Complete raw stream decoded to its terminator using an exact provider | Unsupported layouts and malformed input fail; raw success does not require a complete B-Rep |
| `map_brep` / Rust `map_xb_brep` / `map_xt_brep`; `read_brep`; CLI `check` / `parse --brep` | Source topology checked and supported geometry mapped | Well-formed unmapped geometry is explicit with `complete=False`; invalid references/topology raise parse errors |
| `compare_documents`; CLI `compare` | Decoded structure and values agree after remapping within the selected tolerances | Equivalent documents are not an independent geometric oracle |
| `write_xb` | Original bytes retained for an unmodified binary document | Does not encode edited values; replay equality alone does not validate decoding |
| OCCT / STEP / preview / CadQuery | The requested adapter's separate completeness, validity and constraints pass | Conversion and preview coverage do not broaden the input profile |
| Rust `partial` | Known fragment records and available read spans recovered | No document-wide framing or full delta state; the caller retains unrecognized bytes and incomplete status |

CLI status `0` means success for that command's stage. `parse --brep` also
returns `0` when mapping returns an explicitly incomplete model; inspect its
JSON `brep.complete`, or use `check` to enforce completeness by exit status.
Status `1` reports an incomplete `check` result or non-equivalent `compare`
result, and `2` reports input/schema/parse/conversion errors. An explicitly
allowed partial preview can likewise be generated with status `0`.
See the [API diagnostics](api.md#inputs-and-schema-selection) for exact codes.

CAD adapters extract and bound the stream, select configurations, pair
partitions and deltas, and retain container/decompression provenance. The strict
parser's ranges start at the supplied complete stream; `partial` ranges start
at its supplied body slice. IDs are local to their document/stream. Physical
units must come from caller evidence. The caller converts model-space lengths
once and preserves knots, angles, directions, weights and surface UV values.
Strict NURBS models retain source homogeneous coefficients; partial readers
return Euclidean poles. See the
[partial-reader contract](../crates/parasolid-core/PARTIAL_READERS.md).

An iCAD container adapter is planned but has not been created. Existing iCAD
evidence concerns extracted Parasolid bytes only. SolidWorks container and
configuration handling belongs to sldkit; its shared partial readers do not
establish complete saved-state reconstruction.

## B-Rep topology

The typed source model represents bodies, regions, shells, faces, loops,
half-edges, edges, vertices, points, curves, and surfaces. It validates required
references, ownership chains, loop and edge rings, inverse relationships, and
solid edge manifold conditions before returning the model.

Topology and geometry IDs are local to one parsed document. Every mapped value
retains a reference to its source node and byte range; these IDs must not be
treated as persistent Parasolid identifiers.

## Typed geometry

With a suitable explicit schema provider, the mapper provides typed values for
the following effective geometry types. This broader mapper table does not
expand the built-in profile's raw/type coverage:

| Category | Effective types |
|---|---|
| Analytic curves | `LINE`, `CIRCLE`, `ELLIPSE`, `PARABOLA`, `HYPERBOLA` |
| Curve wrappers | `TRIMMED_CURVE`, `SP_CURVE`, `INTERSECTION` |
| Analytic surfaces | `PLANE`, `CYLINDER`, `CONE`, `SPHERE`, `TORUS` |
| Surface wrappers | `OFFSET_SURF`, `BLENDED_EDGE`, `BLEND_BOUND` |
| NURBS | `B_CURVE`, `NURBS_CURVE`, `B_SURFACE`, `NURBS_SURF`, and their control-point and knot records |

Valid geometry outside this table remains visible as `UnsupportedGeometry`.
It produces a recoverable diagnostic and sets `BrepModel.complete` to `False`.
Malformed references, arrays, or topology fail with `ParseError` instead of
being converted to an empty shape.

## Parse, OCCT, and STEP geometry coverage

This adapter table assumes an already mapped `BrepModel`; it does not imply
built-in raw support for every listed type. This table is rendered from
`parasolid_kit.interop.occt.GEOMETRY_COVERAGE`; a test requires the embedded
text to match that machine-readable contract exactly. `conditional` means the
listed constraints are checked before OCCT is imported. It does not mean that
unsupported variants are approximated.

<!-- BEGIN GENERATED I7 GEOMETRY COVERAGE -->
| Category | Geometry kind | Parser | OCCT | STEP | Exact constraints |
|---|---|---:|---:|---:|---|
| curve | `line` | exact | exact | exact | two vertices |
| curve | `circle` | exact | exact | exact | vertex-free full period; use trimmed for an arc |
| curve | `ellipse` | exact | exact | exact | vertex-free full period; major radius >= minor radius |
| curve | `parabola` | exact | exact | exact | two vertices on one exact branch |
| curve | `hyperbola` | exact | exact | exact | two vertices on one exact branch |
| curve | `trimmed` | exact | exact | exact | explicit basis, parameters, endpoint positions, and two vertices |
| curve | `nurbs` | exact | conditional | conditional | non-rational open non-periodic 3D control vertices; exact knots and multiplicities |
| curve | `surface_parametric` | exact | unsupported | unsupported | 2D pcurve coordinate/parameter contract not yet established |
| curve | `intersection` | exact | unsupported | unsupported | retained construction records do not define a reconstructible exact curve |
| curve | `unsupported` | unsupported | unsupported | unsupported | unknown source semantics are retained without inference |
| surface | `plane` | exact | exact | exact | explicit trim loops |
| surface | `cylinder` | exact | exact | exact | two vertex-free circular boundary loops |
| surface | `cone` | exact | exact | exact | frustum with two positive-radius circular boundary loops |
| surface | `sphere` | exact | exact | exact | untrimmed closed face; OCCT seam topology is generated |
| surface | `torus` | exact | exact | exact | untrimmed closed ring torus; OCCT seam topology is generated |
| surface | `nurbs` | exact | conditional | conditional | non-rational open non-periodic 3D row-major control grid; zero or one trim loop |
| surface | `offset` | exact | conditional | conditional | supported exact basis surface; I7 verifies a non-periodic NURBS basis |
| surface | `blended_edge` | exact | unsupported | unsupported | blend construction records are retained but not reverse engineered |
| surface | `blend_boundary` | exact | unsupported | unsupported | depends on unsupported blend reconstruction |
| surface | `unsupported` | unsupported | unsupported | unsupported | unknown source semantics are retained without inference |
<!-- END GENERATED I7 GEOMETRY COVERAGE -->

## Derived metrics

The parser-only package intentionally has no geometry-kernel or tessellation
dependency. It can derive:

- a bounding box from mapped topological vertex points;
- area when every face is planar and every loop is polygonal; and
- volume when the body is solid and its complete boundary is planar.

Metrics whose exact preconditions are not met are `None`. Curved area and
volume are not approximated from endpoints or from a hidden mesh.
`BrepSummary` labels these values as source transmit units because the parser
does not currently establish the physical length unit.

## Known exclusions

- No X_T writer or general-purpose edited-document serializer.
- No native iCAD `.icd` or other CAD-container parser.
- No assembly/product structure, occurrence transforms, visibility, or
  appearance model.
- No parser-core tessellation, remote/cloud viewer, appearance reconstruction,
  or general-purpose mesh export; I6 is an optional OCCT-derived inspection
  view.
- No STEP assembly/product semantics, names, colors, PMI, or edited feature
  history; I4 exports geometry/topology only.
- No inferred CadQuery assembly, Workplane chain, or editable feature history;
  I5 returns shapes only.
- Application-owned user fields are retained separately as integer words in
  `RawNode.user_fields` and included in document comparisons. Non-zero user fields
  are supported only for node types whose PK visibility is documented in the
  April 2008 XT reference; unknown visibility is rejected, not guessed. Synthetic
  X_T/X_B parity is tested, but compatibility with arbitrary non-zero-user-field
  producers is not established. This does not supply missing schema definitions
  or enable schema-free parsing.
- A transmitted opaque `q` field is rejected because its neutral-file byte
  representation is not defined by the current schema model.

Resource use is bounded by `ParseLimits`, including file size, node and schema
counts, field and string sizes, variable-length arrays, and retained
diagnostics.

Optional `[occt]` and `[cadquery]` installation profiles preserve the lazy,
guarded runtime boundary. The base parser and `[occt]` profile support Linux,
macOS, and Windows. The `[cadquery]` profile supports Linux and macOS only;
Windows CadQuery adapter calls fail with `interop.unsupported_platform` before
native import because the tested runtime crashes during process shutdown.
Windows users can use `[occt]` for conversion, STEP export, and preview.

I7 adds exact ellipse, parabola, hyperbola, explicit
trimmed curve, cone frustum, full sphere, full ring torus, non-rational 3D
NURBS, and exact offset-surface paths to the I3 point/line/circle/plane/cylinder
baseline. Direct vertex-trimmed circles and ellipses remain rejected because
their two possible arcs are ambiguous without source parameters. Rational
NURBS remain conditional-coverage failures in the public adapter. Closed or
periodic NURBS likewise remain outside its verified conversion path. The V13
source-storage campaigns establish bounded homogeneous-coefficient and
pole/knot evidence for parsing; they do not implement those adapter paths.
The adapter validates source references,
basis-reference cycles, OCCT topology, bounding box, area, and volume and
performs no implicit healing or approximation.

I4 can export any complete, valid documented OCCT result directly as geometry/topology-only
AP242. Output is staged, bounded, accompanied by a schema-version-1 conversion
sidecar, and cold-reimported in a separate process. Validation requires reader
success, non-empty valid shape, body/face counts, bounding box, area, and
volume; it deliberately does not require identical STEP edge/vertex counts or
topology IDs. Existing outputs are not overwritten by default, and partial or
incomplete conversions are rejected.

I5 wraps the body subshapes produced by that same conversion. One source body
returns its most specific CadQuery `Shape` subclass; multiple bodies return an
explicit `cadquery.Compound`, and `to_cadquery_shapes()` preserves source-body
order. The adapter compares validity, face count, bounding box, area, and
solid-only volume with the OCCT result and does not infer assembly or
feature-history semantics.
Source mapping is not claimed to survive later CadQuery modeling operations.

I6 tessellates the unchanged `OcctConversionResult` into deterministic GLB
face triangles and edge line strips. Edge polylines reuse the same bounded OCCT
face mesh, so I7 analytic and NURBS edges do not require an unbounded secondary
curve sampler. A separate JSON manifest maps picking IDs
to conversion-local face/edge keys, Parasolid IDs, source node IDs and byte
ranges, geometry kinds, bodies, and diagnostics. Triangle, vertex, curve
sample, diagnostic, subshape, and total output sizes are independently bounded;
limits fail with `preview.limit_exceeded` and never trigger hidden
simplification. Missing mappings fail by default, while explicit partial mode
shows a visual warning and missing-entity list.

The viewer is a fixed-hash, package-owned MIT HTML/CSS/JavaScript bundle. It
uses WebGL without VTK, CDN, or a Node.js runtime. Its server binds an ephemeral
`127.0.0.1` port by default, serves only the five generated/reviewed files, and
requires explicit permission for a non-loopback bind. The browser receives no
source path or raw Parasolid bytes.

This optional conversion/export/preview does not expand the parser's source-format
coverage and does not make OCCT, CadQuery, or STEP the canonical parse result.
GLB is likewise a derived inspection artifact, not the canonical parse result.

## Current interoperability evidence

The [supported profiles](#supported-profiles) distinguish current built-in
evidence by exact key and processing stage. Earlier controlled V26/V30 pairs
were also parsed using explicit catalogs; those results do not establish
built-in support for their other keys. In particular, the earlier
`SCH_3700000_36001` path needs its exact caller catalog, while
`SCH_3701229_37102_13006` has the separate partition-only built-in profile.

These cases test the implemented paths but do not guarantee compatibility with
all exporters or modeling features. The public package deliberately contains
no proprietary schema catalog or CAD fixture; see the
[corpus policy](../corpus/README.md) for redistribution requirements.
