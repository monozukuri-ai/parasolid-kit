# Built-in profile provenance

The [SolidWorks partition profile](solidworks-partitions.md) documents the
additional exact V37 key, WORLD base layout, local validation and delta boundary.

Four profiles are registered for default parsing. The complete internal stream
key must match; a caller-selected provider always takes priority. The
[shared support matrix](format-support.md#supported-profiles) records their
current identities and separate raw, B-Rep, independent-geometry, conversion,
and saved-state evidence. The campaign results below retain their original
scope and failures. The current V30 profile has revision-3 identity:

```json
{
  "kind": "builtin",
  "profile_id": "onshape-sch30000-r3",
  "profile_revision": 3,
  "schema_key": "SCH_3000000_30000",
  "coverage": "verified_subset",
  "profile_sha256": "67e0f3f90c9025c16269c0b03d2365834d949797e1f4c0eb4255b153960b7bf4"
}
```

The maintained definitions are in
[`sch30000.rs`](../crates/parasolid-core/src/schema/profiles/sch30000.rs).
The source model's field roles are in
[`profile_roles.rs`](../crates/parasolid-core/src/brep/profile_roles.rs).
The profile is project-owned code, not an embedded Siemens schema catalog.
Build, installation, and runtime use neither external catalogs, local evidence
files, nor a CAD installation. Normal build/package dependencies remain separate.

## Sources and method

The implementation uses the published
[Parasolid XT Format Reference, April 2008](https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf),
particularly printed pages 5–19 (wire format), 31–35, 54–60, 77–78
(analytic geometry), and 87–119 (records and classes). Per-type source comments
remain alongside the code. The PDF is not redistributed in the package.

For revisions 1 and 2, version-specific definitions were checked against paired
Onshape V30 X_T/X_B
exports and self-describing V30 model edits for types 12, 19, 70, and 74.
Project-owned names identify fields; the raw codec and semantic role tables
remain separate. Class 1040 is declared by observed inputs, without claiming
its complete membership. Type 74 retains generic pointers without inventing a
class constraint. No Siemens catalog was used to generate those revision-1/2 definitions.

Optional catalog comparisons were performed after candidate parsing, as an
independent comparison of complete record boundaries and field values. Those
catalogs were not parser input on the built-in path. Producer body details,
mass properties, simultaneous STEP exports, and isolated STEP reimports supplied
geometry evidence independent of the raw decoder.

For the embedded iCAD profile, a one-time developer audit of catalog-header
metadata established that type 204 is absent from base 13006. It supplied no
field layouts. This historical use is distinct from catalog-free runtime,
build, and installation; see the
[type-204 evidence](#embedded-intersection-data-revision-4).

## Verified scope

The profile covers Onshape V30 X_T and neutral X_B with zero user fields and
single-solid boxes, polygonal prisms, cylinders, through-holes, and spheres.
Revision 2 also decodes elliptical edges in the verified solids. The 24 raw
node types are:

```text
12 13 14 15 16 17 18 19 29 30 31 32 50 51 53 70 74 79 80 81 82 83 84 98
```

These contain 202 field groups. The 15 types through 53 in this list provide
B-Rep topology and point/line/circle/ellipse/plane/cylinder/sphere roles; the remaining types
preserve associated lists, attributes, and value arrays. Raw decoding alone
does not imply that all attributes have a public semantic mapping.

The revision-1 baseline used 13 development pairs and 12 new pairs collected after
freezing the implementation and validation conditions. Across all 50 streams,
7,282 records and 51,142 fields matched the saved boundaries and decoded values.
The frozen holdouts included transforms, dimension changes, a distinct
asymmetric octagonal prism, Unicode body names, and shifted through-holes.
All 25 pairs passed B-Rep and X_T/X_B comparison. Planar exports were validated
through OCCT/STEP reimport. Curved core area/volume remain unavailable, and
vertex-trimmed arc conversion is not extended by this profile.

Revision 2 adds type 32 (`ELLIPSE`, 12 fields) and type 53 (`SPHERE`, 11
fields). The April 2008 reference describes their geometry on printed pages
34–35 and 59–60. V30 ellipse records place the orientation character before
the centre vector, differing from the printed ellipse structure. This order
was checked against paired V30 bytes and producer geometry; it is not inferred
from the circle layout. Sphere radius precedes its axis and X-axis vectors.

Three existing real pairs (a solid with an elliptical edge and two sphere
radii) passed complete parsing, X_T/X_B comparison, decoded-value reencoding,
and B-Rep checks with revision 2. New geometry parameters matched producer
body details and independent simultaneous STEP imports. STEP area and volume
fell within the producer's reported error bounds. Synthetic tests cover
nonzero/negative centres, negative orientation, distinct radii, rotated axes,
field truncation, null geometry, and invalid radii. These synthetic variations
are not new producer-generated holdouts.

Four new pairs were then collected from immutable Versions in a dedicated
public Onshape document, after freezing 86 implementation and validation files.
They contain cylinders cut at 30 and 50 degrees (one elliptical edge each),
and translated spheres with radii 7.019 and 11.021 mm. All four pairs passed
complete parsing, X_T/X_B comparison, decoded-value reencoding, B-Rep topology,
and geometry checks against both producer body details and independent STEP
imports. Their centres and radii also matched the predeclared model dimensions.
No frozen file, fixture, or tolerance was changed after collection.

The original full validation passed for both spheres. For the two oblique-cut
cylinders, STEP area differed from the analytic expectation by approximately
1.12e-6 and 2.23e-6 relative, and volume by 3.60e-7 and 7.44e-7. These exceed
the frozen metric limits and remain recorded failures. The imported cylinder
faces use approximate pcurves even though their 3D ellipse parameters match;
STEP metrics still lie within the producer's reported bounds. These pairs
establish decoding and geometry coverage, not a passed strict mass-property
comparison or curved core area/volume support.

The revision-2 local regression inventory contained 100 streams: 96 yielded
complete B-Rep models and all 48 X_T/X_B pairs compare equal. The remaining
four streams use unsupported V26/V37 keys. These counts include earlier
development inputs and must not be interpreted as 48 fresh holdouts.

These are local real-data results. Public CI uses synthetic selection, codec,
limit, packaging, and bounded fuzz tests; it does not contain the real fixtures.
An isolated-install runner accepts separately supplied fixture pairs for local
wheel and sdist validation. The fixtures, catalogs, private evidence, and PDF
are excluded from both distributions.

Other keys/producers, user fields, assemblies, multiple bodies,
sheets, and NURBS are outside this verification claim. Runtime checks use exact
keys, reviewed types, and structural invariants; they do not classify a file's
shape to broaden the claim. See [format support](format-support.md).

## V30 revision 3: complex trimmed topology

Revision 3 adds 20 reviewed types, reaching **44 types / 356 field groups**:

```text
38 40 41 45 52 54 87 89 124 125 126 127 128 133 134 135 136 137 141 204
```

The shared geometry layouts follow the public XT reference (printed pages
44–52, 57–60, 64–72, 112, 116–118). V30-specific details are maintained
explicitly: type 38 has the additional intersection-data pointer; type 41
has a second character before its limit-point array; types 125 and 135 have
version-specific pointer classes. Vector-array types 87/89 and variable type
204 are retained as typed raw fields. A developer audit compared the candidate
with a local exact-key catalog. The catalog is neither bundled nor required
at build, installation or runtime. The earlier no-catalog layout provenance
above describes revisions 1 and 2; it does not describe this revision's audit.

One user-supplied V30 stream now parses all 161,585 nodes with complete, valid
B-Rep topology. Every raw field value and byte range, and the normalized B-Rep,
agree with caller-catalog parsing. Synthetic text/binary tests cover the new
roles and truncation boundaries. This is development evidence from one reused
input, not new producer-generated X_T/X_B holdouts or an independent CAD geometry
oracle. Earlier producer campaigns and their failures remain unchanged.

The [V30 parametric viewer report](v30-parametric-viewer.md) describes the
bounded OCCT conversion and local display checks. Intersection CHART/LIMIT
positions are retained in the source model separately from any reconstructed
curve. Raw support does not imply arbitrary schema, rational/periodic adapter,
STEP mass-property, or assembly reconstruction support.

## 13006 base and embedded profiles

The additional profiles are implemented in
[`sch13006.rs`](../crates/parasolid-core/src/schema/profiles/sch13006.rs):

Their current exact keys, revisions and canonical hashes are in the
[shared support matrix](format-support.md#supported-profiles).

Both have `verified_subset` coverage and require zero user fields. They share
30 reviewed types: the 24 listed above plus cone (52), intersection (38),
chart (40), limit (41), trimmed curve (133) and shared geometry owner (141),
with 240 base field groups. The iCAD revision 5 first added the trim layout;
Onshape revision 4 validated it with V13 producer pairs. Onshape revision 5
adds seven SP_CURVE dependency types, reaching 37 types / 277 field groups.
Onshape revision 6 adds B_SURFACE (124), SURFACE_DATA (125), and NURBS_SURF
(126), reaching 40 types / 327 field groups. These ten additional layouts are also reviewed independently by V30 revision 3;
no iCAD embedded membership is inferred.
Their canonical hashes are independent; the iCAD revision-5 identity is unchanged.
Revision 1 covered 24 types / 191 groups; revision 2 covered 25 types /
204 groups. Their validation results below remain historical evidence. Public
reference structures were checked against actual Onshape exports requested as
Parasolid 13.0, whose internal key is `SCH_1300000_13006`. This establishes the
base layouts independently of the embedded V30 deltas. In particular, LIST
(70) has 12 base fields, including its cursor block, cursor index, and logical
transmission flag. An observed V30 delta ends after its first nine fields;
that prefix alone cannot establish a complete base. BODY (12), REGION (19),
and pointer list block (74) also differ from the V30 layouts.

The Rust provider now distinguishes four states: a complete definition,
known present but unsupported, confirmed absent, and unknown membership.
Only confirmed absence authorizes interpreting the embedded bytes as a full
definition for a new type. Unknown or unsupported types fail before consuming
the embedded definition, even if its bytes could resemble a valid full
declaration. The embedded profile classifies type 204 as confirmed absent
from 13006; all other types outside its reviewed subset remain unknown.
Complete caller catalogs retain their existing missing-type behavior; a partial
Rust provider must override `SchemaProvider::lookup_type`.

Full declarations, unchanged markers, and Copy/Delete/Insert/Append/End edits
are exercised in synthetic text/binary tests. Those tests also cover truncation,
non-transmitted variable fields, exact-key rejection, and conflicting membership
claims. Real embedded streams exercise copied base fields and inserted fields.
B-Rep mapping validates that every required semantic role survives as exactly
one copied base field. An inserted field cannot replace a deleted role merely
by using its name or codec. A structurally valid raw document may therefore
still be rejected by the B-Rep mapper.

Revision-1 development validation used three V13 pairs (an oblique cylinder, a translated
sphere, and a box), covering all 24 types. Complete parsing, text/binary equality,
independent value reencoding, and B-Rep topology passed. Decoded analytic geometry
and points matched producer body details and independent simultaneous STEP
imports in metres (1e-9 position/radius and 1e-10 direction tolerances). STEP
area/volume stayed within producer bounds; available planar core metrics matched
STEP at 1e-8 relative tolerance. This does not change the revision-2 oblique-cut
cylinder failures against strict analytic mass properties described above.

Two previously examined neutral X_B streams extracted from a local iCAD file
also passed complete B-Rep, topology, and value reencoding. These establish
development compatibility only. Their geometry has no independent producer or
STEP oracle. The independent value encoder reconstructs record values without
using original field byte ranges; it replays embedded schema blobs unchanged,
so it is not an independent schema encoder.

After freezing 131 implementation and validation files, three additional V13
pairs passed the same checks: a different oblique angle, a different sphere
radius/translation, and a rotated box. These are new exports of existing
immutable model states, not newly created geometric models. Across all six
V13 pairs, 1,044 records and 7,084 field groups were decoded and reencoded.

The same frozen implementation was then tested against all 15 reserved neutral
X_B streams from the local iCAD container. Eight passed complete B-Rep,
topology, and value reencoding (2,373 records / 18,591 fields); seven stopped
with `schema.unknown_base_type`. The first uncovered types were 52 (four
streams), 38 (two), and 133 (one). All 15 hashes differed from each other and
the development streams. No implementation or tolerance was changed to turn
these failures into passes. These are unused streams from one container,
without an independent producer geometry oracle; their successful parsing does
not establish support for all iCAD solids. The two development streams plus
eight heldout successes total 3,501 records / 27,447 field groups.

Revision 2 adds cone (52), using the 13-field structure on printed pages
57–58 of the public XT reference. Its radius is measured at the stored origin;
it is not necessarily a cap radius. The signed sine/cosine half-angle values,
axis, X-axis, and orientation are preserved in the B-Rep source model. The
existing V30 profile has not acquired cone coverage from this base extension.

Two new V13 development pairs (a cone with an apex and a rotated frustum)
passed complete parsing, text/binary equality, value reencoding, and B-Rep
checks. Cone parameters matched both producer body details and independent
STEP imports. Decoded geometry also matched the declared cap positions/radii
and half-angle. STEP area/volume matched the analytic cone/frustum formulas
within 1e-8 relative tolerance. Synthetic probes cover negative orientation,
signed half-angle components, rotated axes, zero radius, rejected null/negative
radius, unchanged/copied embedded definitions, and every truncated binary
prefix through the cone payload and terminator.

After freezing 135 implementation and validation files, two further models
were newly created in immutable Onshape Versions: a translated frustum with
different radii and a reversed, tilted cone with its apex at the bottom.
Both new V13 pairs passed the same raw, B-Rep, producer/STEP geometry and
analytic metric checks without changing the frozen code or tolerances.
All four new cone/frustum pairs total 440 records / 2,904 field groups.
These results establish source-model decoding; curved core metrics and
optional OCCT/STEP export coverage are not expanded by this profile change.

The prior 17 iCAD streams were rerun as development regressions. Of the four
previously blocked first by type 52, two now reach complete B-Rep and topology
with 12 and four cone records respectively. Their first cone schemas use the
unchanged-base marker. The other two advance to an uncovered type 38. In total,
at revision 2, 12 of the 17 streams completed; four stopped at type 38 and one
at type 133.
Those reused inputs are not new holdouts, and the iCAD geometry still lacks an
independent producer/STEP oracle.

Embedded X_T coverage is synthetic; the real iCAD evidence is X_B only.
The profile does not parse `.icd` containers, claim arbitrary iCAD versions,
or cover every type under either accepted key. No local CAD file was uploaded
to create this evidence. Real inputs and local evidence remain excluded from
the wheel and sdist.

## 13006 intersection extension (revision 3)

Revision 3 adds surface intersections and their chart, limit, and shared-owner
records. The public XT reference (printed pp. 44–48, 80–81) supplies the field
structures; newly created Onshape V13 text/neutral binary exports verify their
wire layouts. An `h` field transmits three position coordinates only. Cached
surface parameters, tangent vectors, and curve parameters are not transmitted
and are not invented by the parser.

The B-Rep source curve references its two supporting surfaces and the chart,
start and end records. Mapping validates target types, chart point counts,
finite positions, and the H/L (one point) and T (two points) limit layouts.
The raw records retain array contents, schema edits, byte ranges, and original
values. Type 141's shared-owner ring is retained as raw data. This extension
does not implement numerical intersection evaluation, core area/volume, or
optional OCCT/STEP conversion for these curves.

Two new development models subtract an offset transverse or tilted cylinder
from another cylinder. Both pairs pass text/binary comparison, independent
value reencoding, complete source B-Rep and producer topology checks. All
analytic surfaces match producer body details and independent STEP surfaces;
chart/limit positions lie on both supporting cylinders within 1e-9 m. The
shared-owner rings identify both intersection branches.

The independent STEP export splits closed intersection edges and approximates
them as B-splines. Development point-to-boundary errors are 2.52e-6 to 7.90e-6 m:
they **fail the original 1e-7 m strict distance check**. They fall within the
STEP file's declared 2e-5 m global uncertainty; this looser result is recorded
separately and does not establish exact STEP curve equality. This acceptance
clarification was fixed before creating heldout Versions. STEP area/volume
also fall within the producer's reported bounds.

After freezing 134 source/test/validation files, two further models were
created in new immutable Onshape Versions: a larger transverse bore and a
translated bore with a reversed tilted axis. Both pass the same source, raw,
B-Rep, cylinder-distance, and declared STEP-uncertainty checks. Their strict
1e-7 m STEP comparison also fails (1.70e-6 to 5.49e-6 m). All four new pairs
total 712 records / 4,720 field groups. No parser or acceptance changes were
made for the heldout data.

At revision 3, the 17 reused iCAD development inputs included four that
advanced past type 38 to unknown base membership for type 204; one stopped
at type 133. Thus 12/17 completed. No absence classification was guessed,
and no proprietary schema catalog was used to construct the revision-3
field definitions. These iCAD inputs have no independent geometry
oracle and were not uploaded to Onshape.

## Embedded intersection data (revision 4)

At revision 4, only `icad-sch30000-13006` advanced. Its compiled base remained
29 types / 228 field groups. Type 204 is now explicitly marked **absent** from
13006, enabling the existing full embedded-schema decoder for that type.
All other unknown memberships still stop before decoding a definition;
neither Onshape profile is broadened.

This classification uses a one-time developer audit of the locally installed
13006 catalog **header**, whose maximum-type-plus-one is 185 (upper bound 184).
The catalog identifies schema 13006 and modeller build 1300120; its SHA-256 is
`0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3`.
Thus 204 lies outside its declared type-table range. This step uses catalog
metadata as membership evidence, distinct from constructing field definitions.
No catalog layouts are copied, and runtime/build/install need no catalog file.

The input supplies type 204's full declaration. The observed records contain
an unsigned-byte `uv_type` and a variable double array `values`; the decoder
retains the transmitted field names, codecs, lengths, raw schema, and ranges.
It does not impose a fixed value count or infer UV meaning. Repeated instances
reuse that stream's resolved definition.

The appended `intersection_data` field of type 38 maps to an optional source
reference only after checking its Append origin, unique name, scalar pointer
codec, class 204, and transmission flag. A non-null reference must target
an actual type-204 node. Unreviewed names remain raw; inserted impostor fields
cannot acquire this role. Existing copied base roles still follow insertions.

All four previously blocked iCAD streams now pass raw parsing, value
reencoding, exact byte replay and complete source B-Rep/topology checks. Their
13 type-204 nodes are preserved and referenced by the corresponding curves.
The full schema bytes can also be reconstructed independently from decoded
field metadata. At revision 4, the reused 17-stream set had **16/17 complete**, with only
type 133 remaining. These are development regressions, not new holdouts or
independent evidence for the geometry's physical correctness. Embedded X_T
coverage remains synthetic. The revision-3 strict STEP distance failures and
numerical intersection/OCCT export limitations remain unchanged.

## Embedded trimmed curves (revision 5)

The iCAD profile adds type 133 using the twelve field groups described in the
[public XT reference, pp. 48-50](https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf).
The remaining V30 input transmits an unchanged marker and 15 complete instances;
all reference already covered lines. No catalog field layout or new membership
classification is used. At the iCAD revision-5 milestone, Onshape V13 and V30 profile identities
were unchanged because no V13 producer evidence for type 133 was available.

Reviewed copied fields connect the basis curve, endpoints and parameters to the
existing `TrimmedCurve` model. Insertions can shift the fields; deletion followed
by insertion of the same name cannot acquire a reviewed role. Mapping requires
a non-null curve reference, finite endpoints and parameters, positive trim sense,
and distinct parameters ordered according to a known basis sense. Direct self
references are rejected. These checks do not evaluate arbitrary basis curves,
validate their full parameter domains or resolve general geometry cycles.

All **17/17** existing extracted iCAD V30 streams now pass raw parsing and complete
source B-Rep/topology. The newly completed stream has 524 nodes, 1 body, 20 faces,
54 edges and 36 vertices. Its 30 trim endpoints agree with evaluation of the stored
line and parameter to within `7.8e-18 m`. Values survive independent reencoding,
and the binary writer reproduces the input bytes. This is an arithmetic consistency
check on existing development data, not an independent producer/STEP oracle.

Synthetic X_T/X_B tests exercise copied and shifted fields, cached definitions,
forward references, both basis senses, invalid references, non-finite/null values,
truncation and exact-key scope. Embedded X_T evidence remains synthetic. A new
Onshape collection attempt at that milestone returned HTTP 403 for the existing
test document before creating any model; the resumed V13 evidence is described
below. Numerical
intersection and OCCT conversion limits remain as recorded above.

## V13 trimmed curves (Onshape revision 4)

After authentication was restored, fresh synthetic Onshape models established
the same twelve-group type-133 layout in V13. Two touching cuboids, one rotated
by `0.00001` degrees about their longitudinal axis, produce six trims with line
bases. An intersection-cut cylinder and a `0.0000001`-degree cuboid variant
produce no trims. A larger `0.001`-degree variant additionally needs type 137
(surface-parametric curve), which remains uncovered; it is not a successful
whole-model fixture for this revision.

The positive development pair was followed by two new immutable Versions,
created after freezing 128 source, test, collector and validator files. The
held-out cases reverse only the rotation sign and change only the second
cuboid's length. Each has six trims, 8 faces, 18 edges and 12 vertices.
All three X_T/X_B pairs pass complete parsing, value reencoding and source
B-Rep/topology checks. Their 18 trim records retain the basis, endpoints and
parameters without reading a catalog.

Producer Body Details and STEP imported by OCCT independently confirm analytic
geometry and topology. Trim endpoints agree with stored line/parameter evaluation
within `1e-12 m`; their maximum distance to the STEP boundary is `9.08e-9 m`,
below the `1e-8 m` threshold fixed before held-out collection. Perturbed endpoints
fail the distance check. Core and STEP area/volume lie within the immutable
producer mass-property intervals. These tolerance-sensitive solids validate the
observed line-based trims; they do not establish arbitrary basis-curve evaluation.

The built-in V13 profile advances to `onshape-sch13006-r4` with 30 types /
240 groups. The iCAD revision-5 canonical identity and its 17/17 regression
results remain unchanged, as does the separate Onshape V30 revision-2 scope.
The earlier intersection STEP distance failures remain historical results for
different geometries and thresholds.

## V13 surface-parametric curve revision 5

`onshape-sch13006-r5` adds SP_CURVE (137) and its observed dependencies:
B_CURVE (134), NURBS_CURVE (136), CURVE_DATA (135), BSPLINE_VERTICES (45),
KNOT_MULT (127), and KNOT_SET (128). This is 37 types / 277 field groups for
exactly `SCH_1300000_13006`. The iCAD revision-5 and V30 revision-2 profiles
retain their previous definitions and hashes.

Layouts follow the public *Parasolid XT Format Reference*, April 2008,
pp. 39–43, 52–53 and 116–118, and were checked against immutable Onshape V13
text/binary exports. No schema catalog supplied these field layouts. SP_CURVE
retains its supporting surface, parameter curve, optional original curve and
tolerance. The B_CURVE mapper preserves homogeneous control coefficients
(weighted coordinates followed by the weight), distinct knots, multiplicities,
flags and raw-node provenance. CURVE_DATA is retained as raw metadata; its
analytic-form pointer does not establish helix support.

Four synthetic solids contain 32 SP_CURVE instances per encoding. The two
held-out versions were created after freezing the parser, independent value
encoder, tests, collector, model sources and geometry checks. Their sole changes
are rotation sign and tool length. Every pair passes complete parsing, X_T/X_B
equality, independent value reencoding, B-Rep topology, and producer/STEP topology
counts. Surface geometry and vertex coordinates match both producer body details
and STEP. UV evaluation reconstructs stored trim endpoints within 1e-12 m, and
each fin references the same supporting surface as its face. Core and STEP
area/volume fall within immutable producer mass-property intervals.

Each SP_CURVE is sampled at five parameters. All samples fit one STEP edge
within that edge's imported tolerance; the largest distance/tolerance ratio is
about 0.988442. A separate strict 1e-8 m nearest-edge check fails for 24 of the
32 curves. These tolerance-sensitive seams therefore do **not** establish 10 nm
agreement. Moved-point and swapped-UV negative controls fail as expected. The
criteria were fixed on development inputs before collecting either held-out
version; the earlier intersection and trim campaigns retain their own results.

The initial revision-5 campaign covers open, nonrational degree-1 UV curves on planes.
Synthetic tests additionally exercise rational degree-2 coefficients and
nonuniform knots, forward/cached references, nulls, malformed dimensions,
weights, multiplicities, ordering, truncation and allocation limits. They do not
establish producer coverage for general NURBS, periodic curves, nonplanar SP_CURVE
or OCCT export. The local evidence is in `.internal/m8e-spcurve/`; model downloads
and schema catalogs are not distributed with the package.

## Cylindrical SP_CURVE evidence with revision 5

A subsequent campaign validates the existing revision-5 layouts on cylindrical
support surfaces. Definitions, profile revision and canonical hash are unchanged.
Two development and two prospective held-out V13 pairs each contain 95 records,
626 field groups, two SP_CURVEs, three trims, four faces, three edges and no
Parasolid vertices. The synthetic models join two equal-radius cylinders with a
small tilt. The held-out models change only the tilt sign or the shared radius;
the parser, tests, collector, sources and numerical criteria were frozen before
their immutable Onshape versions were created.

X_T/X_B equality, independent value reencoding, B-Rep topology, producer topology
counts and analytic surface/edge geometry pass. The observed parameter curves
are degree-1, nonrational, open UV lines with angles from pi to 3*pi, in either
direction. The angles must remain unwrapped: although the UV endpoints differ,
they embed as a closed 3D circle. A regression test checks this distinction and
the intermediate arc. Local evaluation using the public cylinder parameterization
is checked against OCCT's independent 2D spline and cylinder evaluation, stored
trim endpoints, and the fin's supporting face.

STEP contains three closed edges; OCCT import represents them with three vertices
and adds two cylindrical seams, giving five imported edges. These counts differ
from Parasolid's vertex-free closed-edge representation. The corresponding three
analytic circular edges are matched individually. STEP area/volume remain inside
producer intervals; core bounding box, area and volume remain unavailable for
these inputs.

All eight SP_CURVEs pass the parse/parameter checks. The separate STEP per-edge
tolerance gate **fails for four of eight curves**, with a largest distance/tolerance
ratio of about 1.010273; the fixed 1e-8 m gate fails for those same four curves.
The transmitted Parasolid seam-edge tolerance is 1e-5 m and covers the observed
producer-edge deviation, but it does not change the failed STEP result. Negative
controls reject shifted points and prematurely wrapped angular endpoints. No
threshold was enlarged after observing the held-out models.

Increasing tilt to 0.2 degrees failed during Onshape feature generation before
export, so that attempt supplies no parser or high-degree curve evidence. General
high-degree, rational and periodic UV curves were outside that campaign; the
revision-6 evidence below extends this boundary.
The reports in `.internal/m8e-spcurve-curved/` distinguish
`parse_and_parameter_checks_passed` from `step_edge_tolerance_passed: false`.
This extends the evidence for parsing on cylinders, without claiming general
nonplanar reconstruction or stricter geometric agreement.

## Hash and revision contract

`profile_sha256` hashes the canonical compiled wire definitions, not the Rust
source bytes. The canonical object contains `profile_id`, `revision`, sorted
`schema_keys`, and node types in ascending order. Each type contains
`node_type`, `name`, `variable`, and ordered fields represented as
`[name, codec, pointer_class, element_count, transmitted]`. JSON object keys
are sorted and serialization is compact UTF-8. Descriptions, source paths,
evidence paths, and the digest itself are excluded.

Embedded profiles additionally include sorted `unsupported_base_types` and
`absent_base_types` arrays in this canonical object. Changing membership claims
therefore changes the profile hash even when its known definitions stay equal.

The Rust integration test recomputes this hash. The distribution gate checks
that the reviewed profile, registry, and role source files are present unchanged
in the sdist; isolated wheel and sdist installs verify the reported ID, revision,
exact key, coverage, and hash. Runtime provenance is distinct from per-type
`SchemaSource` and is exposed through `document.schema_resolution` and the
B-Rep summary. `verified_subset` must not be presented as full-schema coverage.


## Higher-degree, rational and periodic UV evidence with revision 6

The new surface dependency layouts follow the public XT reference, printed
pages 67–72 and the type table on pages 116–118, and are checked against
immutable Onshape V13 X_T/X_B exports. No schema catalog supplied field layouts.
B_SURFACE maps to the existing `NurbsSurface` model. SURFACE_DATA retains its
four intervals, boundary flags and derived pointers as raw metadata; implicit
surface extensions and derived analytic forms are not evaluated.

Two generation paths provide different evidence:

- `opWrap` with `WrapType.SIMPLE` creates three cylindrical sheets from a cubic
  Bezier outline, a circle and a closed fitted spline. Their exported UV curves
  are degree 2 and nonrational; two are periodic and one is open. IMPRINT instead
  produces intersection curves and supplies no SP_CURVE evidence.
- `opCreateBSplineSurface` with explicit `boundaryBSplineCurves` preserves a
  degree-3 rational periodic UV curve. Four weighted control points become
  seven homogeneous coefficients, including the three overlapping points.
  Eleven distinct knots range from -0.75 to 1.75; the active domain is [0, 1].
  A planar bilinear sheet and a sheet with one corner raised by 2.307 mm validate
  the same boundary against independent source formulas.

The generation APIs are documented in the
[Onshape FeatureScript library](https://cad.onshape.com/FsDoc/library.html#opCreateBSplineSurface-Context-Id-map).
The source defines each UV weight and each surface pole; Body Details reports
these B-surfaces as `OTHER`, so it does not supply independent surface coefficients.
The mass-properties API supplies no mass/area interval for these sheets.

Development cases D2–D4 and the final prospective cases H3–H4 validate complete
raw/B-Rep parsing, X_T/X_B equivalence, binary value reencoding, producer topology
and endpoints, and 129 samples per SP_CURVE. Independent de Boor and OCCT 2D
B-spline evaluation agree; bilinear/cylinder formulas agree with OCCT 3D surface
evaluation. Checks include knot boundaries, periodic shifts by several periods,
trim endpoints, wrong-weight controls and displaced-point controls.

H3 changes one UV weight; H4 changes only the cylinder radius. Their immutable
versions are created after the final implementation/evaluator freeze. Earlier
H1–H2 results remain separate: an installation verifier still expected profile
revision 5, so that check was corrected and new, previously unused parameters
were collected after another freeze. The parser and geometry evaluator did not
change between those rounds.

Distance gates remain distinct. The new samples fit the STEP files' declared
20 micrometre accuracy, while exceeding the imported OCCT per-edge tolerances
and the separate fixed 10 nm gate. A rational boundary is exported to STEP as
an approximating nonrational 3D B-spline. Body Details/STEP area differences also
fail the separate fixed 1e-10 square-metre gate. These observations are retained;
neither edge tolerances nor area thresholds are widened to turn failures into
passes. They do not establish exact STEP reconstruction.

A subsequent [STEP accuracy audit](step-accuracy.md) reads the saved STEP
coefficients independently and reproduces the distance and area differences.
It separates export boundary approximation, existing tolerant FIN/EDGE geometry,
and importer pcurves, and explains why imported edge tolerances cannot serve as
cross-format parser-correctness thresholds. Historical results remain unchanged.

The mapper now rejects internal knot multiplicity greater than degree and an
empty active parameter domain. The same knot validation applies to curves and
surface directions; surface dimensions, closure and positive homogeneous weights
are checked before returning a B-Rep. Rational surface coefficients have synthetic
coverage; producer surface evidence in that campaign is nonrational and bilinear.
The subsequent [NURBS surface campaign](nurbs-surface-evidence.md) verifies
higher-degree open nonrational/rational sheets and nonrational periodic sheets
in each axis without changing the revision-6 layouts. The
[rational periodic campaign](rational-periodic-surface-evidence.md) additionally
validates weighted periodic sheets in each axis. The
[doubly periodic campaign](doubly-periodic-surface-evidence.md) verifies
degree-3-by-2 nonrational/rational sheets periodic in both axes. Implicit
extensions, arbitrary keys and general curve evaluation remain outside this
producer evidence.

Local reproducibility artifacts are under `.internal/m8e-spcurve-higher/`:
`campaign.json`, `collection.json`, `freeze.json`, `validation-development.json`,
`validation-heldout.json`, and the preserved `round1/` records. The public models
are project-owned synthetic inputs; iCAD inputs and local evidence archives are
excluded from wheel/sdist artifacts.

## Higher-degree NURBS surfaces with unchanged revision-6 layouts

Eight new immutable V13 X_T/X_B pairs cover degree-3-by-2 open patches with
nonuniform knots, an interior rational weight, and nonrational U-periodic and
V-periodic sheets. Four prospective versions change one control-point height
or weight after the implementation, test fixtures and evaluator freeze.

The typed homogeneous coefficients match Onshape's direct `evSurfaceDefinition`
output exactly. Independent source/tensor/OCCT evaluation, 504 native producer
points and normals, paired parsing and independent Rust value reencoding pass.
The [surface evidence report](nurbs-surface-evidence.md) records the observed
residuals and scope. This extends producer evidence for `NurbsSurface`; it adds
no schema types, public evaluator or optional-adapter coverage.

A further four immutable pairs combine rational weights with U or V periodicity;
two prospective versions change a single weight after a separate freeze.
The native homogeneous coefficients match exactly, and position/derivative
checks verify the periodic seam at 72 sampled cross-sections. The
[rational periodic surface report](rational-periodic-surface-evidence.md) retains
the separate source hashes, numerical evidence and scope without changing the
profile layouts or earlier reports.

Four additional immutable pairs verify nonrational and rational surfaces with
simultaneous U/V periodicity, including two prospective versions after a new
freeze. Their degree-3-by-2 surfaces transmit 9 × 7 control grids with overlap
in both directions. Native coefficients match exactly, and 168 seam
cross-sections pass position/derivative checks. The
[doubly periodic surface report](doubly-periodic-surface-evidence.md) records
the evidence and two added regression cases; the runtime and profile remain
unchanged.
