# Changelog

## Unreleased

- Reuse prepared NURBS surface knots and homogeneous controls across repeated point samples.
- Reuse indexed POLYLINE lookups and one lazily initialized exact-schema MESH decode context during B-rep mapping.
- Compute mapped NURBS surface normals from knot-domain-scaled tangents, including at parameter bounds and on tiny domains.
- Split compiled TopSolid effective schema definitions by schema ID while preserving their layouts.
- Restrict XT `BLENDED_EDGE` rolling-ball evaluation to equal absolute range magnitudes (within relative `1e-12`) and `thumb_weights == [1, 1]`; preserve signed per-support offsets and fail closed otherwise.
- Add built-in TopSolid v6 X_T schema profiles and catalog support for five
  observed schema keys: the 13006 base subsets and bundled effective catalogs
  cover those keys only; this does not provide general TopSolid package or
  model decoding.
- Expand the native B-Rep model and X_T/X_B mapping with retained intersection,
  polyline, swept-surface, mesh, assembly hierarchy and attribute data, plus
  geometry evaluation and tessellation APIs. Rust and Python expose the added
  data and selected operations; this is not a claim of complete Parasolid
  geometry coverage or CAD-kernel compatibility.
- Expose intersection chart parameters, limit metadata, and UV data as immutable Python values while retaining unknown UV modes and raw values.

- Fix #13: declare the public-reference HELIX_CU_FORM (163) base layout for
  the SolidWorks and standalone-transmit profiles over schema 13006. Embedded
  schema edits now address its seven field groups, including one interval for
  the turn range, so these records no longer stop parsing with
  `schema.unknown_base_type`. Authored X_T/X_B fixtures cover the reported
  `SCH_3601228_36001_13006` key, family selection, CLI checks and STEP export.
  The original confidential assembly has not been validated.

- Intersection curves whose source chart continues past the domain of a
  bounded B-surface (the offsets a blend spine is cut from) now fit. Beyond a
  finite, non-periodic bound Parasolid continues a B-surface *linearly* along
  its boundary tangents (past a corner the two extensions multiply), and an
  offset follows the normal of that extension; OCCT evaluates the polynomial
  beyond its knots instead, 0.06 to 22 mm away. `SupportSurface` evaluates
  and projects onto the linear extension (237 of 238 out-of-domain chart
  points of four iCAD SX production bodies lie on it within 1e-12 mm; the
  last is on a periodic direction, see below). Before, such a chart point
  failed as "source intersection point is outside its support-surface
  tolerance". The source's own periodicity flags travel with the surfaces: a
  periodic XT B-surface arrives in OCCT as a non-periodic surface whose knots
  wrap past the period, and that direction is never extended.
- Intersection fitting projects each point with a Gauss-Newton search seeded
  by the previous foot and only falls back to OCCT's extrema search when the
  seed misses, and takes the parameters of the interpolated points from the
  interpolation knots instead of projecting them back; a 900-face blend part
  that spent 140 s in the fit now fits in a few seconds. The refinement step
  is Levenberg-Marquardt damped: two offsets meeting at a shallow angle (a
  blend between nearly tangent faces) made the exact Newton step slide along
  both surfaces and oscillate without converging.
- A blend spine whose own chart stops short of the blend face (contact
  curves and face vertices up to 18 mm past the last chart point on
  production parts) is continued along the intersection of its two supports
  in both directions until every such point projects onto its interior, and
  refitted; before, the contact construction clamped to the spine end and
  failed with "blend contact drift ... exceeds".
- Rolling-ball blends are swept with the discrete (rotation-minimising)
  trihedron of `BRepOffsetAPI_MakePipeShell` instead of `GeomFill_Pipe`'s
  corrected Frenet frame, which left the pipe 0.6 to 400 mm off its spine
  where the spine's curvature vanished or turned sharply while reporting
  success; the achieved approximation error is now checked against a tenth
  of the validation tolerance (`PIPE_TOLERANCE_FRACTION`; a hundredth made
  production pipes of thousands of poles that were slower to build, to
  project onto and to mesh, without changing any measured edge tolerance).
  The sweep is approximated with cubic pieces and with pieces of degree 11,
  and the pipe with fewer poles is kept: both join one piece per spine span
  with C0 knots, and the cubic pipe usually has a third of the poles (the
  blend pipes of seven blend-heavy production bodies went from 1.33 to 0.71
  million poles with unchanged edge tolerances).
  The blend contact curve keeps every
  chart point as a sample (two chart points sharing a spine parameter lost
  one before) and its drift check compares against the median chart spacing.
- A FIN whose SP-curve lives in a blend's own parameters (which the rebuilt
  canal surface does not reproduce) gets its pcurve by projecting the edge's
  3D curve and is left out of the source FIN validation; an edge whose own
  curve is such an SP-curve takes its 3D geometry from the FIN on the
  adjacent face. Before: "unsupported parameter-curve support surface".
- `OcctConversionOptions.fin_deviation_cap` (target units): a FIN whose
  sampled source geometry leaves its declared edge tolerance plus the
  conversion budget is accepted up to the cap, with the non-fatal
  `occt.source_fin_deviation` diagnostic and the operation
  `accept_source_fin_deviation_within_cap`; the output edge tolerance already
  covers the measured deviation. Without a cap the edge fails as before.
- Wire bodies convert: their edges, chained into wires where they connect
  within the precision (`ShapeAnalysis_FreeBounds`), one compound per body
  (`wire_body_from_edges`), through both builders and the conversion
  preflight; a wire body without a shell takes the edges listed on the body.
  Before: "shell ... has no boundary faces" or "body kind wire is not
  supported". The preview draws a free edge from the polygon the mesher
  stored on it and accepts an edge-only preview of a wire-only model
  (before: `NCollection_List::First` on the empty face list, then
  `preview.empty_mesh`).
- Edges known only in blends' parameters: an edge between two blend faces
  whose own curve is absent or an SP-curve on a blend is walked as the
  intersection of the two rebuilt canal surfaces from its start vertex to
  its end vertex (`blend_intersection_edge_walked`); where the two blends
  are tangent (consecutive fillets of one radius) it is the cross-section
  arc of the rolling ball through both vertices (`blend_section_arc_edge`).
- The vertices of an edge that uses an intersection curve directly join the
  curve's anchors, and the trim points of edges on a blend contact curve
  join its chart (`blend_contact_curve_through_trim_points`): the fitted
  curves pass through them instead of missing them by what the fit leaves
  between chart points (3 to 8 µm on production parts).
- The blend contact point is the support at the ball centre's parameters on
  the offset the spine was cut from (an offset keeps its basis parameters),
  no longer the nearest point of the support, which is wrong where the
  support curves more tightly than the radius (an offset's swallowtail: 0.1
  mm on a 20 mm blend) or where the contact runs past the support's domain.
  The drift check allows a tenth of the median chart spacing, ten times the
  precision or a hundredth of the radius, whichever is largest.
- A FIN that runs past the domain of its B-surface gets its 3D curve fitted
  through the source's (linearly extended) points instead of the pcurve on
  the OCCT surface, whose polynomial continuation left the trim end 0.06 mm
  from its vertex ("UV trim disagrees with its source point").
- `fit_intersection` accepts a per-support uncertainty (a blend pipe is only
  as exact as its fitted spine) and, with `fin_deviation_cap`, a point its
  supports meet only within the cap (an offset of a basis with a crease
  leaves a gap the source bridged): the largest such distance becomes the
  curve's approximation, carried into the edges on it, and the non-fatal
  `occt.intersection_deviation` diagnostic reports it. Without a cap such a
  fit fails as before.
- A loop that passes through one vertex twice (a sliver face thinner than
  its vertex tolerance: two spikes on a strip, every edge paired with another
  between the same vertices) is split into its simple wires before the face
  is oriented (`split_self_touching_loops`); OCCT's wire traversal of such a
  loop depended on the face's orientation and failed the reversed face.
- The linear extension applies to B-surfaces and to the B-curve section of a
  swept surface (which continues along its end tangent; a chart ran 55 mm
  past the section's knots on a production part); a sphere's v direction
  ends at the poles and a cone's or cylinder's v is unbounded, so neither is
  continued. A source-periodic B-surface that OCCT holds non-periodic wraps
  its parameter by the B-spline period (knot[n] - knot[p]) instead. An
  offset surface reports the normal of its basis, which stays defined on the
  offset's focal set where the offset's own derivatives vanish. A blend
  around a straight or circular spine stays the exact cylinder or torus of
  `GeomFill_Pipe`.
- A FIN whose SP-curve leaves the knots of its B-surface (into the linear
  extension or the next period) has its pcurve projected from the edge's 3D
  curve instead of taken from the source (`project_fin_curve_outside_knots`),
  since OCCT evaluates the stored pcurve there on the polynomial. A ring
  edge without a curve of its own is built from its closed FIN curve.
- A source-periodic B-surface is built as OCCT's periodic B-spline: the XT
  form (one period of knots, the degree's worth of imaginary knots on each
  side, the first poles repeated at the end) is the flat form of OCCT's
  periodic surface, and `SetUPeriodic`/`SetVPeriodic` keep one period of
  poles; the two agree inside the period (checked on five by five samples;
  2e-13 mm on eleven production surfaces). Non-periodic, OCCT continued the
  polynomial of the end spans past the period, where a face loop that
  crossed it (the pcurve of a 3D curve on the seam, which OCCT's own
  projection unwrapped to u = -0.038) landed 8 mm off; periodic, OCCT
  evaluates the wrap itself and the seam fixes apply as on a cylinder.
- A FIN trim stored a period away from its curve's range (a ring edge's FIN
  from 0 to -1 over a curve spanning [0, 1] whose image closes, on a
  periodic surface or as a periodic B-curve) is moved by whole periods into
  the range (`wrap_closed_fin_trim`), and a trim that crosses the end of a
  closed curve takes its points beyond the end from the curve's start: the
  edge is fitted through them and the face projects its pcurve
  (`sp_curve_fitted_across_its_closure`). Before, OCCT read the trim off its
  curve's end polynomial (`BRepBuilderAPI_ParameterOutOfRange`,
  `Geom2d_BSplineCurve::InsertKnots`). A FIN's trimmed SP-curve runs along
  the FIN from its start parameter to its end parameter, backwards when the
  start is the larger (on 200 bodies of a production part, all 10,406 FINs
  whose trim ends match their edge's vertices start at the FIN's own start
  vertex, 1,048 of them with the larger start parameter); a ring edge
  with neither vertices nor a curve of its own takes its direction from the
  FIN it is built from, and its half-edges and the pcurves of its other
  FINs follow from that, since its coinciding ends cannot tell (before:
  "half-edge ... has no orientation reference").
- `BRepLib::SameParameter` is retried with a looser target when it leaves an
  edge above ten thousand times the body's linear resolution (a tenth of a
  millimetre at the usual 1e-5 mm) and its declared tolerance:
  `Approx_SameParameter` discards a
  reparametrization that misses its target, which left a FIN mapped
  linearly onto an edge whose parameter runs at another speed (its 3D curve
  follows the other face's FIN) 0.33 mm off along the edge while its image
  was within micrometres. The looser target installs the reparametrized
  pcurve, the strict target is reached from it (2 µm there), and the vertex
  tolerances the first pass grew to the discarded deviation are set back to
  what the installed pcurve needs (`reparametrize_pcurve_through_looser_target`).
- A rolling-ball blend around a closed spine (a ring blend) is swept with a
  fixed binormal, the normal of the spine's mean plane, instead of the
  discrete trihedron: the rotation-minimising frame returns from a closed
  loop rotated about the tangent, so the pipe's end section was its start
  section with another parameter origin, the surface was not closed along
  the spine, and a pcurve projected across that end jumped by the whole
  sweep range (23 mm of edge tolerance). The spine is reparametrized over
  the circle's range first, since OCCT's seam fix adjusts the two wires of
  a torus-like face by one period taken for both directions
  (`ShapeFix_Face::FixMissingSeam`). The pipe's end row is set onto its
  start row where they agree within the validation tolerance and the
  surface is made periodic along the spine, so the band face between two
  closed contact curves gets its seam like a torus.
- A face whose only kernel complaint is the orientation of its wire, while
  the wire closes, does not cross itself and is otherwise sound, and whose
  mean width (twice the area over the perimeter) is within the body's
  declared tolerance cap, is accepted (`accept_sliver_face_orientation`):
  the signed area of a strip a hundredth of a millimetre wide is numerical
  noise, and the orientation fix and the check disagree about it.
- Face loops are assembled with `BRep_Builder` instead of
  `BRepBuilderAPI_MakeWire`, which merges vertices that lie within each
  other's tolerance and replaces the edges it joins there by copies carrying
  the merged vertex: an edge shorter than its vertex tolerance (a tolerant
  source sliver, 0.03 mm long under a 0.04 mm vertex) was copied in one face
  only, the neighbour kept the original and the shell did not close.
- A spun surface whose profile meets the spin axis at one end only, which
  XT records as that start (or end) point with its profile parameter and a
  null other end, is the revolution of the profile from that point to its
  other end (a full period of a periodic profile). The point must lie on
  the axis and on the profile at its parameter; any other one-sided record
  still fails closed. Before, every such surface (an iCAD SX revolved cap)
  failed as "spun surface with a one-sided profile parameter range".
- Intersection fitting on surface extensions uses the extension's own
  derivatives (forward differences only for an offset of an extension),
  keeps the seeded Gauss-Newton foot while the point is within a hundred
  thousand tolerances of the surface (a chord midpoint of a coarse chart),
  stops the iteration at 1e-10 of the parameter range and evaluates with
  coordinates instead of temporary OCCT vectors; with the coarser pipe
  approximation, a blend-heavy production body (306 faces, 240 blend
  contact curves) builds in 28 s instead of 140 s, and the 417 saved bodies
  of its file in 116 s instead of 285 s.
- Face construction: a wire whose edges were reordered by a neighbouring
  periodic face's seam split (the two pieces of a shared edge arrive in that
  edge's own direction) is stored in traversal order before the orientation
  fix, which judged such a wire by the signed area of its edges in stored
  order and reversed a correct loop (`order_wire_edges_after_seam_split`).
  `BRepLib::SameParameter` no longer lowers an edge's declared tolerance (a
  ring edge dropped from 1e-5 to 1e-7 and the planar face with a second ring
  failed its orientation check). An offset of a basis with a C0 knot is
  built span by span instead of being refused as "Offset with no C1 Surface".
- A zero-range blended edge (the construction surface a cliff-edge blend
  rolls through, never a face's surface) may store null thumb weights; they
  map to the unweighted (1, 1) rolling ball instead of failing the body with
  "double array contains null or non-finite data". A blend with a range still
  requires both weights.
- Partial conversion of a whole shape (after 0.3.11): with `partial=True`, a
  model where every face builds is still validated as in strict mode, but a
  shape that `BRepCheck` rejects or whose metrics differ from the source is
  returned with those findings as warnings and `occt_valid=False` instead of
  failing, since the caller asked for what could be built. Fifteen of the
  199 files of the second public corpus and an authored Onshape cylinder
  with a hole convert that way; strict mode and STEP export are unchanged.
- `read_brep(source_format="auto")` recognises the `.xmt_txt` and `.xmt_bin`
  suffixes and a common header (`**ABCDEF...`) in front of a text stream
  (the three Siemens example files named `.xmt_txt` failed with "cannot
  identify source encoding" when read by path).
- Partial OCCT conversion: `to_occt(..., partial=True)` drops a face whose
  construction fails instead of failing the conversion, returns a solid that
  lost a face as an open shell and skips a body left without a face. The
  report lists `faces_dropped` (with the reason and source node),
  `bodies_failed` and `bodies_partial`, carries the `occt.faces_dropped` and
  `occt.bodies_failed` warnings, and has `conversion_complete=False`; the
  source metrics are not compared and an invalid partial shape is a warning.
  A model where nothing fails converts as in strict mode, and a model where
  no body survives still fails. STEP export refuses a partial conversion; the
  preview accepts one with `allow_partial`, which the `viewer` option now
  sets for both. `OcctConversionOptions.partial`, `DroppedFace` and
  `FailedBody` are new, and the report dictionary gains a `partial` section.
- Attributes: `map_brep` returns the `ATTRIBUTE` nodes as
  `BrepModel.attributes`, each with its definition's `ATT_DEF_ID` string
  (`SDL/TYSA_COLOUR`, `SDL/TYSA_NAME`, ...), its `type_id`, its owner (a
  mapped body, face, edge, vertex, region, shell, loop, assembly, instance,
  point, curve or surface, or the raw node of an unmapped owner) and its
  fields as integer, real, text, unicode, point, vector, direction, axis or
  tag values from `INT_VALUES`, `REAL_VALUES`, `CHAR_VALUES`,
  `UNICODE_VALUES`, `POINT_VALUES`, `VECTOR_VALUES`, `DIRECTION_VALUES`,
  `AXIS_VALUES` (two vectors per axis) and `TAG_VALUES`; a pointer field is
  kept by name, and an empty field, transmitted as a null pointer, is typed
  by the definition. Bodies and faces list their attribute ids;
  `attributes_of`, `colour_of` and `name_of` read them. The attribute node
  types gain B-Rep roles, and an attribute whose definition, identifier or
  owner pointer does not resolve fails the mapping. Sixteen of the 22 public
  X_T files carry the system colour; the MicroStation files carry only an
  integer `BSI_OldFace`.
- Spun surfaces convert: surface-parameter curves on a `SpunSurface` are
  mapped from the source's (profile parameter, angle) onto OCCT's (angle,
  profile) parameters, with the angle reversed as the surface's axis is, and
  faces on spun surfaces take the parametric path. Rational NURBS surfaces
  are no longer rejected by the pre-flight check for their homogeneous
  control grid; the builder already carried their weights, and a closed or
  periodic surface is built over its stored knot vector.
- Instance placement: `to_occt(..., place_instances=True)` returns the
  bodies positioned by the assembly structure as transformed copies, one per
  instance chain from a root assembly, composing the stored transforms with
  the translation in the target unit; bodies no instance references stay in
  place, negative instances are placed and counted, and the source map is
  rewritten onto the copies with a `generated` relation from each placing
  instance (`SourceEntityKind.INSTANCE`). The report counts
  `instances_placed`, `instances_negative` and `bodies_unreferenced`; a
  transform that is not a rotation with a uniform scale fails.
- Conversion speed: the intersection fit keeps one surface projector per
  support instead of rebuilding the extrema search of a B-spline support for
  every point, which cut the 300-face MicroStation part from 49 s to 21 s;
  the fit was 41 s of it. The chart-residual correction of blend contact
  curves fails when the residual exceeds one percent of the chart spacing,
  so a wrong spine or support cannot hide behind it.
- Corpus regression: `scripts/corpus_matrix.py` runs every file of a
  directory through parse, mapping and OCCT in its own process and writes
  the rows as JSON; `tests/test_public_corpus.py` runs it over the directory
  named by `PARASOLID_KIT_CORPUS` and compares the failures with the
  directory's `expected.json`, and is skipped without the variable.
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
  34 derived families are listed in the support matrix. Motivation: of 22
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
  25001 gains its first family. `nx-3001232-30100-13006-r1`
  (`SCH_3001232_30100_13006`, three public NX parts) and
  `nx-3400176-34001-13006-r1` (`SCH_3400176_34001_13006`, one public NX part)
  anchor the first families of schemas 30100 and 34001, which failed with
  `schema.missing_base_schema` before; an embedded key of a schema no public
  file has shown still does.
- Attribute value nodes: the shared 13006 declaration set, hence every 13006
  family, declares POINT_VALUES (85), VECTOR_VALUES (86), AXIS_VALUES (87),
  TAG_VALUES (88) and DIRECTION_VALUES (89) from the public XT reference
  (printed pages 106-108); two public SolidWorks 2011 parts (VECTOR_VALUES)
  and three Solid Edge and Onshape parts (AXIS_VALUES) failed with
  `schema.unknown_base_type` before. The SolidWorks and transmit profile
  digests change.
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
- Older standard keys: `nx-900141-9003-r1` (`SCH_900141_9003`, NX
  assemblies of Parasolid 9.0.141), `nx-1000252-10004-r1`
  (`SCH_1000252_10004`, NX assemblies of Parasolid 10.0.252, and a SolidWorks
  98Plus part through the family), `ironcad-1100129-11004-r1`
  (`SCH_1100129_11004`, ten IronCAD parts), `solidworks-1200000-12006-r1`
  (`SCH_1200000_12006`, the reference key, declaring the node types of the
  reference's own transmit example) and `nx-1600000-16004-r1`
  (`SCH_1600000_16004`, an NX part) join the MicroStation key in a new
  legacy-keys module; schemas 9003, 10004, 11004, 12006 and 16004 gain their
  families. Parasolid 10 keeps the 13006 BODY but still writes fins without
  their attribute pointer; Parasolid 9 to 12 write `CURVE_DATA` with its
  self-intersection state alone and the 12006-style `ATTRIB_DEF`; schema 9003
  writes lists without the transmission flag; `KEY` (102) is declared from
  the reference as a character string. The MicroStation profile now declares
  the 44 node types of eight public files (four of other builds through the
  family), B-spline surfaces, groups, offset and spun surfaces included; a
  chart whose base scale is unset is accepted. `icad-2401000-20000-r3` adds
  the ellipse, sphere, torus, blends and B-spline surfaces of six public
  parts of another build, and `icad-2901000-28101-r2` the real and
  character values of twenty.
- A `B` (spine boundary) limit on an intersection curve is read like an
  arbitrary limit, with one position, as SolidWorks writes it; values after
  the declared knot, multiplicity and control-vertex counts of a NURBS node
  are ignored instead of being required to be padding (SolidWorks 2019
  sizes the arrays of a periodic curve alike and leaves earlier values
  there); an isolated loop (one vertex inside a face, whose fin points at
  itself with no edge and an insignificant sense, XT reference p. 91) is
  left out of the face's wires and its vertex is not built, instead of
  failing the conversion on the fin's unknown sense (NX writes them at cone
  apices).
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
