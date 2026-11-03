# Changelog

## Unreleased

- Schema families: a key that no exact profile accepts is read with the
  profile derived for its schema family, the key without its modeller build
  (`<effective>[-<base>]`). The registry merges the reviewed definitions and
  base-membership claims of the exact profiles that share the family and
  excludes any type they dispute; `provider_for_key` selects an exact profile
  first and the family profile otherwise. Such documents report
  `coverage: family_inferred`, `profile_id: family-<family>` and the requested
  key. Uncovered types still fail closed; a key with no family still fails
  with `schema.missing_base_schema`, whose default message now names the
  family. `SchemaKey::family_id`, `BuiltinSchemaProfile::new_family`,
  `BuiltinProfileRegistry::families`/`family_for_key`, `FamilyMember` and the
  Python `builtin_profiles()` / `SchemaKey.family_id` are new;
  `BuiltinSchemaProvider` owns its selected key and is no longer `Copy`. The
  29 derived families are listed in the support matrix. Motivation: of 22
  public X_T files from SolidWorks, NX, Inventor, ANSYS, MicroStation and
  Onshape, 18 carried distinct keys and 17 had no exact profile, 14 of them
  under schema families that are already reviewed.
- Base contributors and exact-profile completion: an embedded family takes
  every reviewed profile of its base, the profiles of the same family as
  members and the other profiles of the same base as base contributors, since
  base 13006 declarations describe the same base under every effective schema.
  An exact profile is completed by its family: a type it does not declare
  itself (OFFSET_SURF or the B-spline surfaces under keys where they were not
  observed, WORLD under the Onshape current key) is read with the family's
  declaration while the provenance still names the exact profile, and B-Rep
  role selection validates the exact profile against the family's
  definitions. The family digest hashes `base_members` as well as `members`;
  `BuiltinSchemaProfile::base_members` and the Python
  `BuiltinProfileInfo.base_members` expose them.
- User fields: the `USFLD_SIZE` words follow every node, value nodes
  included, as the Siemens Parasolid 26 and 27 example transmit files show.
  Built-in profiles read them like caller catalogs into `RawNode.user_fields`
  instead of failing with `node.unsupported_user_fields`.
- Transmit-file profiles: `parasolid-2400164-20000-13006-r1`
  (`SCH_2400164_20000_13006`, the Parasolid 24 acceptance-test files, with
  GROUP (90) and MEMBER_OF_GROUP (91) after the public XT reference),
  `nx-2600169-25001-13006-r1` (`SCH_2600169_25001_13006`, NX) and
  `parasolid-2600217-25001-13006-r1` (`SCH_2600217_25001_13006`, the Parasolid
  26 acceptance-test files) reuse the reviewed 13006 declaration set; schema
  25001 gains its first family.
- Standard profiles `icad-1700000-16100-r2` (adds the 19 types seen in a
  public NX 7 part: INTERSECTION, CHART, LIMIT, CONE, SPHERE, TORUS,
  BLENDED_EDGE, BLEND_BOUND, REAL_VALUES, CHAR_VALUES, TRIMMED_CURVE, the
  SP_CURVE dependencies and the shared-geometry owner) and
  `icad-2401000-20000-r2` (adds REAL_VALUES and CHAR_VALUES seen in a public
  Inventor 2012 part and the 17 types seen in a public NX assembly: ASSEMBLY,
  INSTANCE, TRANSFORM, INTERSECTION, CHART, LIMIT, OFFSET_SURF, SWEPT_SURF,
  TRIMMED_CURVE, the SP_CURVE dependencies and the shared-geometry owner)
  replace the revision-1 identities.
- Assembly structure: `ASSEMBLY` (10), `INSTANCE` (11) and `TRANSFORM` (100)
  are declared from the public XT reference (printed pages 78-79 and 83-86)
  in the shared `SolidWorks` set, hence in every 13006 family, and in the
  standard `icad-2401000-20000-r2` profile, after public `SolidWorks` 2020 and
  NX assembly exports failed closed on their first node. `map_brep` returns
  the structure beside the topology as `BrepModel.assemblies` and
  `BrepModel.instances` (part, sign, owning assembly and the stored transform;
  `Transform::apply` evaluates `(rotation · x + translation) · scale`), with
  the Python `Assembly`, `Instance`, `Transform`, `AssemblyKind` and
  `InstancePartKind` types, the `assemblies` and `instances` entity counts and
  the `check` output. Transforms are not applied to geometry or conversion.
  The `SolidWorks` and transmit profile digests change.
- Light read path: `read_brep` parses and maps in one native call
  (`_read_xb_brep` / `_read_xt_brep`) and no longer builds the Python node
  tree. `ParasolidDocument.nodes` is materialized from the native snapshot on
  first access and then kept; the new `node_count` field is available without
  it, and `BrepSummary` uses it. In Rust, `RawNode.definition` and
  `RawField.definition` are now `Arc<TypeDefinition>` and
  `Arc<FieldDefinition>` shared by every node of a type
  (`EffectiveSchemaRegistry::shared`, `SharedDefinition`), which shrinks the
  retained document; field access is unchanged through `Deref`. A 10.7 MB
  SolidWorks X_T with 161,585 nodes reads in 3.2 s with a peak of 480 MB
  instead of 42 s and 3.5 GB. The isolated-install smoke test now
  expects another build of a reviewed schema to resolve through its family.
- SolidWorks partitions: fourteen exact earlier keys (SolidWorks 2015-2025)
  join the 2026 key, each as its own `verified_subset` profile with the shared
  declaration set; the 2026 profile becomes `solidworks-sch37102-13006-r2`.
  The set adds the public-reference SWEPT_SURF (67) layout, retained by the
  B-Rep mapper as an explicitly partial surface, and records base type 204 as
  absent. Deltas still stop at unknown base type 3. The ignored
  `solidworks_partition_evidence` test replays extracted partition, ghost and
  local-body streams from a directory. The Python check that a TORUS record
  does not parse under the SolidWorks key is retired, since every compiled
  embedded profile now declares TORUS. See `docs/solidworks-partitions.md`.
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
  that approximation in their tolerance. Unequal offsets remain unsupported.

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
- OCCT parametric topology selection and robustness: `to_occt` chooses the
  parametric builder itself whenever the analytic one cannot express the
  source (surface-parameter or intersection curves, circle or ellipse edges
  bounded by vertices as SolidWorks writes arcs, trimmed sphere and torus
  faces, NURBS or offset faces with several loops) instead of rejecting the
  model before the optional import. A source cone measures its v along the
  axis, so parameter curves on cones are scaled by 1 / cos(half-angle) onto
  OCCT's generator parameter; an NX ring had put FIN points 4 to 8 µm off
  their cone. An unbounded analytic intersection branch (a line or hyperbola,
  which OCCT bounds at ±2e100) is trimmed to its chart extent plus the same
  margin before a blend pipe is built around it
  (`bound_infinite_intersection_branch`). Intersection fitting drops
  coincident samples before each interpolation round, falls back to the
  shape-analysis projection when the extrema search has no candidate, and
  reports a rejected interpolation as a conversion error. Edges are made
  same-parameter again after the seam and orientation fixes. Two edges of a
  sliver face that cross in parameter space just before their shared vertex
  have their tolerances grown over the crossing, up to the largest tolerance
  the source declares on the body (`fix_wire_self_intersection`); this runs
  after every face is finished, because projecting a pcurve resets the edge
  tolerance to its measured deviation. In the analytic path, a generated ring
  edge of a cylinder or cone face that runs against its source circle
  (Onshape writes the bottom inner ring of a tube clockwise) is marked
  reversed, so the adjacent annulus keeps its inner wire direction.
  Cliff-edge blends (type E) are built like rolling-ball blends, as the pipe
  of the blend radius around their spine: the public XT reference represents
  the cliff edge as the blend's second support, a blend of range [0, 0] whose
  spine is the cliff edge curve, which is never a face's surface. An edge
  whose curve passes through a sphere pole or a cone apex, where no pcurve
  can follow it (a SolidWorks sphere is two halves cut along a great circle
  through its poles, with or without vertices on it), is split there with a
  generated vertex (`split_edge_at_surface_singularity`); the pieces keep the
  curve and the relations record the split. An edge lying on the seam of a
  periodic surface carries one pcurve per orientation, as a Boolean split of
  a sphere leaves it, so two faces of one surface can share it
  (`share_seam_pcurves`); the loops of a face that opposes its surface are
  reversed before the wire fixes rather than after, so those fixes see the
  orientation the face stores. The seam fix keys the pcurves of the edges it
  splits on its trimmed copy of the surface, so they are copied onto the
  shared original surface for a neighbouring face of the same surface. FIN
  trims of a split edge are mapped onto its pieces, the FINs that two faces
  of one periodic surface write for a shared seam edge are served by the
  seam representation, line edges take the same splitting path, and the
  pre-flight check admits rational 2D parameter curves as the coverage
  already stated. Of
  the 22 public X_T files, 18 convert strictly instead of 4; the other four
  are two incomplete sources with explicitly unsupported swept or spun
  surfaces and the two that do not parse. A 10.7 MB SolidWorks X_T with
  2,949 faces now converts to a valid solid in about 30 s.
- Swept surfaces: SWEPT_SURF (67) is decoded as `SurfaceKind::Swept` (the
  section curve and the unit sweep direction; the scale is internal and
  stays unread) instead of being retained as an explicitly partial surface,
  with the Python `SweptSurface` and the `swept` kind. The OCCT interop
  builds it as the surface of linear extrusion of the mapped section, whose
  parameters and normal are the source's `R(u, v) = C(u) + vD`; parameter
  curves on it keep u as the section's own parameter (a length along a line)
  and scale v as a length, and faces on it take the parametric path. An
  offset surface is now offset against the natural normal of a basis of
  negative sense, as the reference's "taking into account the surface sense"
  requires; an NX part offsetting swept surfaces of negative sense had its
  offsets on the wrong side. The two public X_T files whose sources were
  incomplete for swept surfaces (a SolidWorks 2011 plaque with 37, an NX part
  with 2) now map completely and convert, so 20 of the 22 convert strictly;
  the remaining two do not parse.
- Stream prefix and an observed node type: an ANSYS Electronics Desktop
  transmit written by Parasolid 34 under `SCH_3300000_32001`, a key without
  a base schema, puts a count before its first node, where the reference
  places one only after a base-schema key. For a writer of Parasolid 34 or
  later, a leading value followed by a node type and the index 1 is read as
  that count, recorded in the new `stream_prefix` header field (text and
  binary) and skipped; earlier writers are read as before. The same file
  opens with node type 3, which the public reference does not list (its
  table starts at ASSEMBLY, 10). `icad-3200000-32001-r2` declares it as
  observed and unverified from that one stream: an identifier, four null
  pointers and three body references; a stream whose type 3 node differs
  fails closed on the next node. The file then parses as three planar faces
  and converts, so 21 of the 22 public X_T files convert strictly; the
  MicroStation file under schema 9008 follows below.
  The release-corpus baselines record the new header field.
- MicroStation modeler key: `microstation-901297-9008-r1` accepts
  `SCH_901297_9008`, the key of the Bentley X_T files written by Parasolid
  9.0.129 inside MicroStation (four public files of the NIST
  engineering-design-models repository), and schema 9008 gains its family.
  No public schema file describes schema 9008; the 27 node types the files
  carry keep their reviewed 13006 layouts except that BODY has no
  `geometry_state` (the byte the reference marks "for future use"), FIN has
  no attribute pointer, and ATTRIB_DEF has no field names and thirteen legal
  owners instead of fourteen, as the reference's own schema-12006 transmit
  example (printed page 10) also writes it. The four files read to their
  terminators, map complete B-Reps and convert strictly. Two OCCT changes
  came with the 300-face part: an analytic intersection whose branch on the
  surfaces scaled to unit extent misses the source points is retried on the
  unscaled surfaces (a plane a few nanoradians off the equatorial plane of a
  torus far from the origin, which OCCT walks instead of solving), and the
  contact curve of a rolling-ball blend is corrected by its chart-point
  residuals, since the geometric contact is ill-conditioned along the
  surfaces when the spines are approximations; a blend whose support is
  another blend touches that pipe on the line to the pipe's spine, at the
  pipe radius. The report records the largest residual as
  `blend_contact_drift`. All 22 public X_T files now parse and convert
  strictly.
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
