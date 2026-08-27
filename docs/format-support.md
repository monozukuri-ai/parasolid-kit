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

## Derived metrics

The package intentionally has no geometry-kernel or tessellation dependency.
It can derive:

- a bounding box from mapped topological vertex points;
- area when every face is planar and every loop is polygonal; and
- volume when the body is solid and its complete boundary is planar.

Metrics whose exact preconditions are not met are `None`. Curved area and
volume are not approximated from endpoints or from a hidden mesh.

## Known exclusions

- No X_T writer or general-purpose edited-document serializer.
- No native iCAD `.icd` or other CAD-container parser.
- No assembly/product structure, occurrence transforms, visibility, or
  appearance model.
- No tessellation, STEP export, rendering, or geometry-kernel object creation.
- Non-zero user fields are not decoded.
- A transmitted opaque `q` field is rejected because its neutral-file byte
  representation is not defined by the current schema model.

Resource use is bounded by `ParseLimits`, including file size, node and schema
counts, field and string sizes, variable-length arrays, and retained
diagnostics.

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
