# Format support and limitations

`parasolid-kit` is a pre-alpha, read-focused Parasolid transmit-file parser.
This page describes the supported public behavior; it is not a claim of
compatibility with every Parasolid version, producer, or geometry type.

## File operations

| Operation | X_B | X_T |
|---|---:|---:|
| Inspect the common and internal header | Supported | Supported |
| Parse a complete node stream | Supported with the exact schema | Supported with the exact schema |
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

## Schema requirement

Complete parsing selects the schema from the internal stream key. For
`SCH_<modeller>_<effective>`, the parser requires `<effective>`. For
`SCH_<modeller>_<effective>_<base>`, it requires `<base>` and applies the
embedded definitions or deltas carried by the stream.

The required catalog must be supplied by the caller. The parser does not:

- bundle Siemens schema catalogs;
- download catalogs;
- choose a nearby schema version;
- infer fields that are absent from the transmitted data; or
- use the human-readable common-header `SCH` value in place of the internal
  X_T schema key.

`DirectorySchemaProvider` and the `--schema-dir` CLI option consider only the
exact `sch_<provider-schema>.sch_txt` file in the requested directory. They do
not recurse, follow symbolic-link catalogs, or select a similarly numbered
catalog.

To determine `<provider-schema>`, run `parasolid-kit inspect MODEL.x_b` or
`parasolid-kit inspect MODEL.x_t` and read `header.schema_key`. Use the second
number in `SCH_<modeller>_<effective>` and the third number in
`SCH_<modeller>_<effective>_<base>`. Obtain the resulting exact catalog from an
available Parasolid SDK or Parasolid-based product, then pass the directory
containing it as `--schema-dir`. The complete acquisition and file-location
walkthrough is in the README's
[Schema catalogs](../README.md#schema-catalogs) section.

## B-Rep topology

The typed source model represents bodies, regions, shells, faces, loops,
half-edges, edges, vertices, points, curves, and surfaces. It validates required
references, ownership chains, loop and edge rings, inverse relationships, and
solid edge manifold conditions before returning the model.

Topology and geometry IDs are local to one parsed document. Every mapped value
retains a reference to its source node and byte range; these IDs must not be
treated as persistent Parasolid identifiers.

## Typed geometry

The mapper currently provides typed values for these effective geometry types:

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

This table is rendered from
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
- Non-zero user fields are not decoded.
- A transmitted opaque `q` field is rejected because its neutral-file byte
  representation is not defined by the current schema model.

Resource use is bounded by `ParseLimits`, including file size, node and schema
counts, field and string sizes, variable-length arrays, and retained
diagnostics.

Optional `[occt]` and `[cadquery]` installation profiles preserve the lazy,
guarded runtime boundary. I7 adds exact ellipse, parabola, hyperbola, explicit
trimmed curve, cone frustum, full sphere, full ring torus, non-rational 3D
NURBS, and exact offset-surface paths to the I3 point/line/circle/plane/cylinder
baseline. Direct vertex-trimmed circles and ellipses remain rejected because
their two possible arcs are ambiguous without source parameters. Rational
NURBS remain conditional-coverage failures until the homogeneous control-vertex
storage contract is established. Closed or periodic NURBS also remain explicit
conditional-coverage failures until their source pole/knot relation is established.
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

Controlled X_T/X_B pairs produced from the same model state have been parsed
and compared for Parasolid V26 and V30. The current shape coverage includes a
rectangular solid and a cylindrical through-hole. V37 header inspection is
covered, but complete parsing remains dependent on a caller-provided Schema
36001 catalog.

These cases test the implemented paths but do not guarantee compatibility with
all exporters or modeling features. The public package deliberately contains
no proprietary schema catalog or CAD fixture; see the
[corpus policy](../corpus/README.md) for redistribution requirements.
