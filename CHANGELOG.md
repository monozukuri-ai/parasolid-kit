# Changelog

## Unreleased

- OCCT parametric topology: an edge whose own geometry is a surface
  parameter curve, or a trim of one, is built from that curve on its surface;
  a parameter curve on an offset surface uses the parameters of the offset's
  basis; and a FIN trim stored against its parameter direction pairs the
  vertex at the lower parameter first.
- NURBS geometry: rational curves and surfaces (XT weights stored as the last
  vertex component) and curves or surfaces stored in their periodic or closed
  form are built; the stored knot vector and poles define the same B-spline
  as a non-periodic OCCT curve or surface over the stored range, which is how
  it is evaluated. Rational 2D parameter curves are accepted too.
- Rolling-ball blends (BLENDED_EDGE with equal offset magnitudes) convert in
  the OCCT interop: the blend surface is the pipe of the blend radius around
  the spine, a B-spline within a hundredth of the validation tolerance, and
  an intersection curve with a BLEND_BOUND construction surface is the blend's
  contact curve on that support, interpolated from the spine and checked
  against the source chart points (`blend_contact_curve`). The pipe is made
  periodic in its closed directions, and edges on approximated branches carry
  that approximation in their tolerance. Cliff-edge blends and unequal
  offsets remain unsupported.

- OCCT parametric topology: a face that opposes its surface now has its source
  loops reversed before the periodic seam and orientation fixes, so the face
  keeps its source region instead of the complement, and the whole face is
  reversed afterwards as before (`reverse_opposed_face_loops`). Closed
  source-identified intersection branches become periodic curves, and the trim
  of such a branch follows the source parameter direction. Horn and apple tori
  (major radius not above the minor radius) are exact for explicitly trimmed
  faces; untrimmed closed torus faces still require a ring torus. A seam fix
  that returns its single face inside a shell is unwrapped instead of leaving
  a null face, and a loop through a sphere pole or a cone apex receives the
  degenerated edge of that singularity before the seam fix
  (`insert_degenerated_singularity_edges`). A closed analytic intersection
  branch now follows the source point order across its parameter origin and
  is made periodic, so an edge on it takes the source arc rather than the
  complementary one.
- Add OFFSET_SURF (60) to the reviewed 13006 declarations and admit it under
  the iCAD keys where it was observed. Add exact profiles for two further
  embedded keys and six further standard keys (13006, 15003, 16100, 19008 and
  26105). Extend four existing profiles with the types observed under their
  keys: `icad-sch30000-13006-r6`, `icad-sch34101-13006-r3`,
  `icad-2100293-20000-13006-r2` and `icad-3301231-33103-13006-r2`. Consumers
  that pin the earlier IDs or hashes must update them. Of 18,634 distinct
  sampled iCAD resources, 18,610 now parse without a catalog and match the
  catalog path. See [format support](docs/format-support.md#supported-profiles).
- Map SPUN_SURF (68) as a surface of revolution: the profile curve, the spin
  axis point and direction, the optional degeneracy points and parameters and
  the optional x axis (`SpunSurface`). The OCCT interop revolves the mapped
  profile about the axis (`Geom_SurfaceOfRevolution`), trimmed to the stored
  profile parameters when both are present, with stored end points checked
  against the profile; explicitly trimmed faces are supported in parametric
  topology. The iCAD keys that declared the type no longer retain it as
  unsupported geometry.
- Map a trimmed curve on a LINE whose two parameters are both unset: they are
  the parameters of the stored end points under `R(t) = P + tD`. The points
  must lie on the line and the order rule still applies; raw fields stay null.
  Other bases and partly unset pairs remain errors.

- Add thirteen exact iCAD profiles with pinned B-Rep roles: eight embedded
  13006 keys and five standard keys for schema revisions 20000, 28002, 28101,
  32001 and 33103. Standard profiles declare complete layouts only for the
  node types seen under each key; their revision-specific BODY, REGION,
  INTERSECTION, LIMIT and list layouts follow the edits carried by embedded
  streams of the same revision. Of 17,099 distinct sampled resources, 17,082
  now parse without a catalog and match the catalog path; 17 with OFFSET_SURF
  (60) still fail closed. See the
  [support matrix](docs/format-support.md#supported-profiles) for per-key
  revisions, hashes and limits.

- Add five exact legacy iCAD embedded 13006 profiles with pinned B-Rep roles
  for Issues #6 and #7. Qualify BLEND_BOUND (59), its BLENDED_EDGE (56)
  dependency, SPUN_SURF (68), and SP_CURVE (137) dependencies only for the
  observed keys. The sampled 539 resources now parse without a catalog;
  535 map completely and four retain explicit unsupported SPUN_SURF geometry.
  See the [support matrix](docs/format-support.md#supported-profiles) for
  per-key revisions, hashes and conversion limits.

- Extend the exact iCAD V34 profile to revision 2 with the reviewed 13006
  TORUS (54) definition and matching B-Rep roles. The profile ID is
  `icad-sch34101-13006-r2`; other schema keys and unknown base types retain
  their existing boundaries. This core patch supports catalog-free parsing
  of qualified toroidal saved solids in downstream iCAD readers.

- Add the exact iCAD `SCH_3401212_34101_13006` built-in profile for extracted
  Parasolid streams, including pinned source B-Rep roles. Unknown base types and
  nearby keys still require their own reviewed support. Native ICD extraction,
  part ownership, placement and unit interpretation remain caller responsibilities.

## 0.2.0 — unreleased

- Add the exact Onshape `SCH_3701212_37102_13006` profile, including analytic
  geometry, direct NURBS curves/surfaces, intersection and trimmed curves,
  surface-parametric curves, and fully declared multi-part transmit blocks.
  Other modeller keys still require their own reviewed profile or an explicit
  schema provider.
- Preserve intersection chart and limit positions in the source B-Rep.
- Add the bundled local viewer and bounded UV/intersection conversion.
  Parser, OCCT conversion, STEP export and preview retain separate support limits.
- Extend release validation with native NURBS coefficients, sampled parametric
  geometry, producer tolerance reporting and hashed native evidence.

### Migration from 0.1.0

- The V30 profile is `onshape-sch30000-r3`; the new current Onshape profile is
  `onshape-sch37102-13006-r3`. Consumers that pin profile IDs or hashes must
  update their expectations for these exact keys.
- Intersection definitions include `chart_points`, `start_points` and
  `end_points`. Strict JSON consumers must accept these additional fields.
- Core area and volume require complete planar polygonal boundaries. Curved
  boundaries return `None`, including cases previously assigned incorrect
  endpoint-polygon metrics. Bounding boxes still use source topological vertices.
- Reading a NURBS definition does not establish OCCT/STEP/preview conversion for
  every rational, closed or periodic variant. Assemblies, native CAD containers
  and SolidWorks delta application remain outside the complete parser contract.

See [format support](docs/format-support.md) for exact keys and stage boundaries.
