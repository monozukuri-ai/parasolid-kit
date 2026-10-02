# Built-in profile provenance

The [SolidWorks partition profile](solidworks-partitions.md) documents the
additional exact V37 key, WORLD base layout, local validation and delta boundary.

Thirty-two profiles are registered for default parsing. The complete internal
stream key must match; a caller-selected provider always takes priority. The
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

### V30 revision 3: new analytic producer pairs

On 2026-09-14, two new Onshape models were created after freezing the parser,
oracle implementation, dimensions and tolerances: a Z-axis frustum with lower
radius 13 mm, upper radius 7 mm and height 19 mm, and a Z-axis ring torus with
major radius 23 mm and minor radius 4 mm. Both centres lie on the origin axis.
The same immutable Onshape Version supplied V30 text/neutral binary pairs,
native Body Details, mass properties and independent STEP AP242 exports.

All four V30 streams passed the unchanged revision-3 parser, complete source
B-Rep/topology checks, and decoded analytic-geometry comparisons against native
and STEP data. The frustum has 59 nodes and three faces; the torus has 31 nodes
and one face. Both producer pairs compare equivalent and preserve X_B bytes.
The Rust corpus probe separately passed full consumption and decoded-value
reencoding. STEP area and volume match the native bounds and predeclared
dimensions, using absolute tolerances of 1e-12 m² and 1e-14 m³ and relative
tolerance 1e-8; geometry position/direction tolerances are 1e-9 m and 1e-10.

The initial API STEP captures declared metres despite requesting millimetres;
that acquisition failure remains recorded. Browser exports with custom
Millimeter units corrected the input settings without changing the model
Version, parser or tolerances. The initial files and failed check are retained.
These two models have no source vertices, so they add no vertex-position
evidence. Curved core metrics and general parametric-solid conversion were not
validated. The real fixtures remain local; they do not expand public CI coverage.

Normal/latest exports from the same Version use internal key
`SCH_3701212_37102_13006`. They initially failed default parsing and then became
development inputs for the [separate current-key profile](#onshape-current-embedded-analytic-profile).
They are not independent holdouts for that new profile.

## Onshape current embedded analytic profile

The current source tree uses revision 3 (41 types / 339 field groups); see
[compound-geometry evidence](onshape-composite.md) and the earlier
[ellipse and NURBS evidence](onshape-current-parametric.md). The revision-1
identity and campaign below are retained as historical evidence.

Revision 1 added `onshape-sch37102-13006-r1` revision 1 for the exact
internal key `SCH_3701212_37102_13006`, observed in Onshape 37.1.212 current
exports. This addition has not yet been published. It uses the reviewed 13006
base and input-defined embedded edits, with explicit membership of 25 types
and 204 base field groups:

```text
12 13 14 15 16 17 18 19 29 30 31 50 51 52 53 54 70 74 79 80 81 82 83 84 98
```

The definitions are in
[`onshape_current.rs`](../crates/parasolid-core/src/schema/profiles/onshape_current.rs).
They reuse 24 previously reviewed base definitions and add TORUS (54), whose
12 fields follow the public reference on printed pages 60–61. The canonical
hash is `da8fcfa794a26dd57caec6b944f5ebe8431657980ce033488ca1ce0b032fbcc6`.
Future additions to other 13006 profiles do not implicitly expand membership.

The source B-Rep roles cover topology, points, lines, circles, planes,
cylinders, cones, spheres and tori. An embedded insertion may move a field,
but only preserved copies of reviewed base fields acquire semantic roles.
Deleting a required field and inserting another with the same name fails
B-Rep mapping. Unknown base types remain unknown even when an input supplies
a full declaration; the profile has no declared-absent or unsupported-base
exceptions. Nonzero user fields and neighboring keys are rejected.

Development used five producer pairs: a box, a cylinder with a coaxial
through-hole, a sphere, and the earlier frustum and torus current exports.
All ten streams pass complete decoding and source B-Rep, Rust/Python value
and byte-range parity, producer-pair equivalence, decoded-value reencoding,
and native/STEP analytic geometry checks. Binary documents also preserve the
original bytes. Box core area and volume match the independent measurements;
curved models use native, STEP and formula metrics because curved core metrics
are unavailable.

On 2026-09-14, 192 implementation, validation and recipe files (including the
native module and Rust probe) were frozen before creating five further models.
Their immutable Onshape Version was created at 04:19:12 UTC, after the
04:18:14 UTC freeze. The models are a translated 19 × 29 × 37 mm box, a
translated coaxial through-hole cylinder (radii 11 / 3.5 mm, height 23 mm),
a translated sphere (radius 8.013 mm), an inclined frustum (radii 17 / 9 mm,
height 23 mm), and an inclined ring torus (radii 29 / 5 mm). The last two use
axis `(0, 0.6, 0.8)` and translated origins.

All five new X_T/X_B pairs passed the frozen validation: ten streams,
746 records, complete B-Rep/topology, API/CLI and Rust/Python parity, producer
pair comparison and value reencoding, native/STEP geometry and formula metric
checks, and byte-exact X_B retention. Source positions and directions use fixed
tolerances of 1e-9 m and 1e-10; area and volume use 1e-12 m² and 1e-14 m³,
with relative tolerance 1e-8. The box additionally verifies its eight vertices
and core area/volume. These STEP exports use metres, confirmed from the actual
SI declaration. Extra STEP seam/degenerate curves are recorded separately;
all required source primitives still match. No frozen file or tolerance was
changed after collection.

A separate exact-catalog audit agrees on decoded values, ranges and normalized
B-Rep. It does not establish identical field metadata: type 74 `entries`
retains generic pointer class 0 in the reviewed base, while the local catalog
labels it 1001. This difference is recorded explicitly. The guarded API/CLI
checks prohibit catalog and network access; the optional catalog audit runs
separately and is not a runtime dependency.

At revision 1, ellipse (32), intersection/trimmed/SP_CURVE and NURBS families
were outside the profile. Other modeller keys, assemblies and native saved-state
reconstruction remain unsupported. The recorded analytic source-B-Rep evidence does not establish
general curved-solid OCCT conversion, STEP export or viewer coverage. Real CAD
fixtures remain local; public tests use project-authored synthetic wire data.

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
values. Type 141's shared-owner ring is retained as raw data. At this
source-decoding milestone, numerical intersection evaluation, core area/volume,
and optional OCCT/STEP conversion were not implemented. The current optional
adapter provides [bounded intersection construction](v30-parametric-viewer.md);
that later implementation does not change this campaign's evidence or failures.

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
coverage remains synthetic. The revision-3 strict STEP distance failures remain
recorded. This campaign did not validate numerical intersection/OCCT export;
current adapter constraints are listed in [format support](format-support.md).

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
below. This campaign did not extend numerical intersection or OCCT conversion;
see [format support](format-support.md) for the current adapter constraints.

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

## iCAD V34 embedded key

The exact `SCH_3401212_34101_13006` key uses
`icad-sch34101-13006-r2`: the independently reviewed 13006 subset plus
the public-reference TORUS (54) declaration (31 types / 252 field groups).
The current identity is `icad-sch34101-13006-r3`; see
[further keys and revisions](#further-icad-keys-and-revisions). Unknown base types still fail closed. The only
confirmed absent type is 204, reusing the existing 13006 membership audit;
no vendor catalog field definitions are embedded. Nearby keys are not accepted.

Ten separately supplied resources from iCAD SX V8L3 cover boxes, a through-hole,
rotated cylinders, a sphere, face-colour conversion and a distinct cylinder
holdout. Candidate parsing used no catalog. A subsequent explicit-catalog
comparison matched all node values and byte boundaries, B-Rep topology and
geometry, except for expected schema-provider type labels. SDK measurements
separately check intrinsic dimensions of four newly authored solids. The SDK
edge-list query failed for the sphere, so its cross-system edge count is not
qualified; its radius, surface area and volume are checked independently.

This qualifies resource decoding, not ICD part ownership, global placement,
file units, native CSG evaluation or arbitrary V34 data. Curved core area/volume
remain unavailable. Public tests use synthetic unchanged POINT records, exact
selection, TORUS text/binary values and roundtrips, unknown-base rejection,
truncation and pinned B-Rep roles. Revision 2 retains the exact-key restriction
and changes both the profile identity and its canonical digest. Private
resources, CAD files and catalogs are not included in the package.

## Legacy iCAD embedded keys

Issues [#6](https://github.com/monozukuri-ai/parasolid-kit/issues/6) and
[#7](https://github.com/monozukuri-ai/parasolid-kit/issues/7) add five separate
revision-1 profiles in [`icad_legacy.rs`](../crates/parasolid-core/src/schema/profiles/icad_legacy.rs).
The [shared support matrix](format-support.md#supported-profiles) pins each
exact key, identity, canonical digest and base-definition count. These are
source-tree additions, not a new published release. Existing profile IDs,
hashes and membership claims are unchanged.

### Declarations and membership

All five profiles reuse the reviewed 13006 subset plus TORUS (54), as in the
V34 revision-2 profile. Additional definitions are scoped independently:

| Exact key | Extra base types beyond the 31-type subset | Raw / source B-Rep boundary |
|---|---|---|
| `SCH_1500137_15003_13006` | None | Existing analytic / topology subset |
| `SCH_1500245_15003_13006` | 56, 59, 68 | Blend and blend-boundary semantics; spun surface remains unsupported |
| `SCH_1700223_16100_13006` | None | Existing analytic / topology subset |
| `SCH_1700256_16100_13006` | 45, 68, 127, 128, 134, 135, 136, 137 | Surface-parametric and NURBS curve dependencies; spun surface remains unsupported |
| `SCH_1901315_19008_13006` | None | Existing analytic / topology subset |

The new 56/59/68 declarations follow the
[public XT reference](https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf),
printed pages 62-65 and 74-76. Type 56 stores the supporting surfaces, spine,
offset and weight pairs, boundary references and limits. Type 59 selects a
blend boundary. Type 68 retains the generating curve, axis, degeneracy values
and scale. Project field labels are independently assigned. SP_CURVE and its
six dependencies reuse the existing public-reference / Onshape V13 declarations.
No catalog field definitions generate the compiled profiles or authored tests.

A separate local membership check confirms that all ten additional types
exist in base 13006. The comparison catalog SHA-256 is
`0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3`.
Every observed extra-type declaration in the eight formerly failing resources
uses the unchanged-base marker. Their effective field layouts, values and
boundaries match the explicit-catalog path. No further missing raw dependency
was found after adding BLENDED_EDGE (56) for the type-59 input. Only type 204
is classified as absent, using the previous membership audit; unreviewed types
remain unknown even if the input looks like a full declaration.

### Local comparison, 2026-10-02

The retained development sample contains 539 resource occurrences from 214
container files. Candidate parsing preserves each original schema key and
uses no external catalog. A separate catalog parse supplies the comparison:

| Original exact key | Raw parity | Complete source B-Rep | Explicitly partial | Valid topology |
|---|---:|---:|---:|---:|
| `SCH_1500137_15003_13006` | 37 / 37 | 37 | 0 | 37 |
| `SCH_1500245_15003_13006` | 139 / 139 | 137 | 2 | 139 |
| `SCH_1700223_16100_13006` | 26 / 26 | 26 | 0 | 26 |
| `SCH_1700256_16100_13006` | 223 / 223 | 221 | 2 | 223 |
| `SCH_1901315_19008_13006` | 114 / 114 | 114 | 0 | 114 |
| Total | 539 / 539 | 535 | 4 | 539 |

All 125,619 nodes and 997,305 field groups match in scalar/array values,
node/field byte ranges, variable lengths, user fields and terminators. B-Rep
comparison includes topology, geometry parameters, metrics, diagnostics and
source node/type/range mappings. Only provider-specific type labels and raw
field names/pointer classes are excluded. Original-byte reconstruction also
matches, but is not used as a substitute for value comparison.

The four partial resources contain SPUN_SURF. Their source surfaces remain
explicit `UnsupportedGeometry` with `geometry.unsupported_surface` and
`complete=False`, matching the catalog path. No approximate replacement surface
is created. BLENDED_EDGE / BLEND_BOUND and SP_CURVE use the existing typed
source model. Neither successful mapping nor this comparison proves OCCT,
STEP or preview conversion.

### Independent checks and remaining limits

Public Rust tests pin the hashes, per-key type membership, nearby-key rejection,
authored point values, byte reconstruction, truncation and the role gate. Public
Python tests exercise both X_T and X_B with independently authored topology:
an 11-node wire from `(2,-1,3)` to `(5,3,3)`, known blend parameters and links,
spun-surface raw values, and known UV spline coefficients. They also cover
explicit-provider precedence, malformed references, truncated fixed/variable
arrays, limits, user fields and embedded edits that replace trusted fields.
Role validation rejects delete/insert impersonation for all newly mapped
geometry roles and rejects an unreviewed profile digest.

These are development samples and authored regression fixtures, not held-out
coverage or a new independent legacy CAD/SDK measurement campaign. The earlier
V13 producer evidence supports reused declarations; it does not establish
legacy whole-model geometry accuracy. Private payloads, filenames and catalog
contents remain outside the public package. Local inputs, hashes, comparison
script and reports are retained under `.internal/issues-6-7/`.

Native ICD framing, resource ownership, transforms, polygon extrusion and
feature-history evaluation remain downstream responsibilities. Unknown keys
do not inherit support from their `13006` suffix. Downstream readers can use
the compiled registry and remove their local raw-only profile after adopting
an upstream build containing these changes; dependency publication and that
downstream migration are separate steps.

## Later iCAD keys

Thirteen further revision-1 profiles cover keys written by later iCAD
releases. Eight are embedded keys in
[`icad_legacy.rs`](../crates/parasolid-core/src/schema/profiles/icad_legacy.rs);
five are standard keys in
[`icad_standard.rs`](../crates/parasolid-core/src/schema/profiles/icad_standard.rs).
The [shared support matrix](format-support.md#supported-profiles) pins each
exact key, identity, canonical digest and definition count. These are
source-tree additions, not a new published release. Existing profile IDs,
hashes and membership claims are unchanged.

### Embedded keys

The embedded profiles reuse the reviewed 13006 subset plus TORUS (54). Extra
base types are listed only where they occur under that exact key:

| Exact key | Extra base types beyond the 31-type subset |
|---|---|
| `SCH_2100293_20000_13006` | 45, 56, 59, 124-128, 134-137 |
| `SCH_2100311_20000_13006` | 45, 56, 59, 124-128, 134-137 |
| `SCH_2401260_20000_13006` | None |
| `SCH_2800188_28002_13006` | None |
| `SCH_2901199_28101_13006` | 45, 56, 59, 68, 124-128, 134-136 |
| `SCH_3200152_32001_13006` | 45, 68, 127, 128, 134-137 |
| `SCH_3200252_32001_13006` | 45, 56, 59, 68, 124-128, 134-137 |
| `SCH_3301231_33103_13006` | 45, 56, 59, 68, 124-128, 134-137 |

No declaration is new: B_SURFACE (124), SURFACE_DATA (125) and NURBS_SURF (126)
are the existing V13 declarations, now also admitted under the listed keys.
Every observed declaration of an extra type uses the unchanged-base marker.
Revision changes to BODY, REGION, INTERSECTION, LIMIT, LIST and the pointer
block arrive as embedded edits of each stream and are not declared by a
profile. Type 204 remains the only confirmed absent type. OFFSET_SURF (60)
occurs in 17 sampled resources and stayed unknown in this batch, so those
inputs failed closed until the [next batch](#further-icad-keys-and-revisions).

### Standard keys

A standard key transmits no schema. Each standard profile therefore declares
complete layouts, and only for the node types seen under that exact key:

| Exact key | Types | Layouts that differ from base 13006 |
|---|---:|---|
| `SCH_2401000_20000` | 19 | BODY, LIST, pointer block |
| `SCH_2800000_28002` | 41 | BODY, REGION, LIMIT, LIST, pointer block |
| `SCH_2901000_28101` | 24 | BODY, REGION, LIMIT, LIST, pointer block |
| `SCH_3200000_32001` | 30 | BODY, REGION, INTERSECTION, LIMIT, LIST, pointer block; type 204 |
| `SCH_3301000_33103` | 25 | BODY, REGION, INTERSECTION, LIMIT, LIST, pointer block; type 204 |

Each differing layout is the reviewed 13006 declaration with the edit sequence
that embedded iCAD streams of the same schema revision carry for that type.
Those streams are self-describing: the sequence gives the position, codec and
reference class of every added, removed or replaced field. BODY has five
revision layouts; at 28101 it equals the reviewed V30 declaration, which a
public test checks, as it does for REGION, LIMIT, INTERSECTION, LIST and the
pointer block. Type 204 has the two-field layout that embedded 32001 and 33103
streams declare in full. Added fields have project-assigned names; base fields
keep their reviewed names, so B-Rep roles are found by name after the mapper
confirms that every resolved type is exactly the compiled definition.

One layout lacks same-revision self-description: no embedded 28002 stream
contains LIMIT (41). The 28002 profile uses the three-field layout carried from
28101 on. All 627 sampled 28002 resources with LIMIT records decode to their
exact terminators with it, and the catalog comparison below agrees.

No catalog field definitions generate the compiled profiles or authored tests.
A separate local comparison used these catalogs (SHA-256):

| Schema | Catalog SHA-256 |
|---|---|
| 13006 | `0dd291ea706fc306f16a78140e05e595e75c85ab63e4077e68941b127fcfdcc3` |
| 20000 | `f66857fb80ce2c85669f0fbd0ba57244cf58b3266da1fae41821185fb66dc730` |
| 28002 | `454f3d62edf047649728ce3e5abdeb23c87cca2c44182c5f9c58583ba12b87f0` |
| 28101 | `8f06fa843ec7775a8567fa3278e7a7386fe5f1046ded76f55604d027ec22fa0a` |
| 32001 | `5ffe6a8942c686e400b3cad1edc19fbe39457589079769d9f7dbcafec535a159` |
| 33103 | `c6acc177a3b088bbe7ef2fc144b2bf1101f91521fccb14f0f9bb713652ab50d3` |

### Local comparison, 2026-10-02

The development sample contains 30,378 resource occurrences from 303 container
files, 17,099 of them distinct by payload SHA-256. Candidate parsing preserves
each original schema key and uses no external catalog. A separate catalog
parse supplies the comparison; counts below are distinct payloads:

| Original exact key | Occurrences | Raw parity | Complete source B-Rep | Explicitly partial | Same error on both paths |
|---|---:|---:|---:|---:|---:|
| `SCH_2100293_20000_13006` | 2,441 | 483 / 486 | 483 | 0 | 0 |
| `SCH_2100311_20000_13006` | 753 | 393 / 393 | 393 | 0 | 0 |
| `SCH_2401260_20000_13006` | 294 | 131 / 131 | 131 | 0 | 0 |
| `SCH_2800188_28002_13006` | 25 | 23 / 23 | 23 | 0 | 0 |
| `SCH_2901199_28101_13006` | 2,309 | 498 / 498 | 493 | 3 | 2 |
| `SCH_3200152_32001_13006` | 188 | 106 / 106 | 103 | 3 | 0 |
| `SCH_3200252_32001_13006` | 4,105 | 541 / 541 | 529 | 4 | 8 |
| `SCH_3301231_33103_13006` | 3,965 | 1,941 / 1,955 | 1,936 | 5 | 0 |
| `SCH_2401000_20000` | 28 | 28 / 28 | 28 | 0 | 0 |
| `SCH_2800000_28002` | 16,168 | 12,889 / 12,889 | 12,879 | 0 | 10 |
| `SCH_2901000_28101` | 21 | 10 / 10 | 10 | 0 | 0 |
| `SCH_3200000_32001` | 64 | 32 / 32 | 32 | 0 | 0 |
| `SCH_3301000_33103` | 17 | 7 / 7 | 7 | 0 | 0 |
| Total | 30,378 | 17,082 / 17,099 | 17,047 | 15 | 20 |

The 17 payloads without raw parity contain OFFSET_SURF and are rejected with
`schema.unknown_base_type`. For the others, all 8,041,842 nodes and 63,103,335
field groups match in scalar/array values, node/field byte ranges, variable
lengths, user fields and terminators, and original-byte reconstruction
matches. B-Rep comparison includes topology, geometry parameters, metrics,
diagnostics and source node/type/range mappings; only provider-specific type
labels are excluded. Every mapped topology is valid. The 15 partial resources
contain SPUN_SURF and keep `geometry.unsupported_surface`. Twenty resources
fail in both paths with `geometry.invalid_parameter`: a trimmed curve on a line
stores both parameters as null beside valid end points. No value is
substituted. Neither successful mapping nor this comparison
proves OCCT, STEP or preview conversion.

### Independent checks and remaining limits

Public Rust tests pin the hashes, per-key type membership, nearby-key
rejection and the role gate. For the standard keys they state each revision's
field codes and reference classes independently of the profile table, decode
authored BODY, REGION, LIMIT, LIST and pointer-block records with a distinct
value at every ordinal, and reject truncation at every byte. Public Python
tests map an authored wire from `(2,-1,3)` to `(5,3,3)` under all thirteen
keys in X_T and X_B, check that B-spline surface dependencies are admitted only
under the keys where they were observed, and cover malformed references,
limits, user fields, nearby keys and unreviewed types. Role validation rejects
a standard definition that differs from the compiled one, even by exchanging
two references of the same codec.

These are development samples and authored regression fixtures, not held-out
coverage or an independent CAD measurement campaign. Unknown keys do not
inherit support from a shared schema number: in the same sample
`SCH_2601246_26105_13006`, `SCH_2601000_26105` and other keys had no profile
until they were reviewed separately. Private payloads, filenames and catalog
contents remain outside the public package.

## Further iCAD keys and revisions

A second batch covers the remaining iCAD keys of the same development sample
that have self-describing evidence. It adds two embedded and six standard
revision-1 profiles, one base declaration, and new revisions of four existing
profiles. The [shared support matrix](format-support.md#supported-profiles) pins
every identity. These are source-tree additions, not a published release.

### OFFSET_SURF

OFFSET_SURF (60) follows the
[public XT reference](https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf),
printed pages 65-66: the common surface fields, a check character, an unused
logical, the underlying surface, the signed offset distance and an internal
scale that may be null. Project field labels are independently assigned. The
mapper already modelled this surface for caller-supplied catalogs; the pinned
roles now select the underlying surface and the distance by reviewed name.
Every observed declaration of the type uses the unchanged-base marker. The type
is admitted only where it was observed.

### Profiles

| Exact key | Profile | Change |
|---|---|---|
| `SCH_1901261_19008_13006` | `icad-1901261-19008-13006-r1` | New embedded key, no extra type |
| `SCH_2601246_26105_13006` | `icad-2601246-26105-13006-r1` | New embedded key: 45, 56, 60, 124-128, 134-137 |
| `SCH_2100293_20000_13006` | `icad-2100293-20000-13006-r2` | Adds 60 |
| `SCH_3301231_33103_13006` | `icad-3301231-33103-13006-r2` | Adds 60 |
| `SCH_3000310_30000_13006` | `icad-sch30000-13006-r6` | Adds 45, 54, 56, 60, 124-128, 134-137 |
| `SCH_3401212_34101_13006` | `icad-sch34101-13006-r3` | Adds 45, 56, 68, 127, 128, 134-137 |
| `SCH_1300218_13006`, `SCH_1302234_13006` | `icad-1300218-13006-r1`, `icad-1302234-13006-r1` | New standard keys: the reviewed 13006 declarations unchanged |
| `SCH_1500000_15003`, `SCH_1700000_16100` | `icad-1500000-15003-r1`, `icad-1700000-16100-r1` | New standard keys: LIST differs from base |
| `SCH_1901000_19008` | `icad-1901000-19008-r1` | New standard key: BODY, LIST, pointer block |
| `SCH_2601000_26105` | `icad-2601000-26105-r1` | New standard key: BODY, REGION, LIST, pointer block |

The revised profiles change identity and digest; a consumer that pins the
earlier IDs must update them. Their previous definitions are unchanged, so
inputs accepted before decode identically. The standard profiles follow the
method of the [later keys](#later-icad-keys): each differing layout is the
reviewed 13006 declaration with the edit sequence carried by embedded streams
of the same schema revision, and each profile lists only the node types seen
under its key. Embedded 15003 and 16100 streams change only LIST; 19008
streams carry the BODY and pointer-block edits also seen at 20000; 26105
streams carry the BODY edit also seen at 28002 and the REGION owner, and leave
LIMIT and INTERSECTION unchanged. A 13006 key transmits the base itself, which
a public test compares type by type with the reviewed V13 profile.

Not covered: schema 8008 and 12103 keys have no self-describing stream in the
sample, and two V30 standard resources need blend or offset types that the V30
standard profile does not declare. They still require a catalog.

### Unset trim parameters

Twenty resources of the earlier comparison failed in both paths because a
trimmed curve on a LINE stores both parameters as null. The mapper now takes
the parameters of the two stored points from `R(t) = P + tD`, which the public
reference (printed page 31) gives for a line with unit direction, and requires
each point to lie on the line within 1e-8. In the sample all 154 such curves
have a unit direction, a positive-sense line, points on the line within 2e-16
and an increasing derived order; 146 are edge curves whose two vertices sit at
the two points, and eight are construction lines owned by the body. Raw fields
keep their null values. Other bases and partly unset pairs are still rejected.

### Local comparison, 2026-10-03

All iCAD keys of the sample were compared again with a separate catalog parse:
18,634 distinct payloads in 33,941 occurrences.

| Result | Distinct payloads | Occurrences |
|---|---:|---:|
| Raw parity and complete source B-Rep | 18,567 | 33,836 |
| Raw parity, explicitly partial (SPUN_SURF) | 32 | 66 |
| Raw parity, same B-Rep error in both paths | 11 | 11 |
| No profile: schema 8008 or 12103 key | 22 | 22 |
| Uncovered type under the V30 standard key | 2 | 6 |

The 8,668,707 nodes and 67,991,664 field groups of the parsed payloads match
in values, byte ranges, variable lengths, user fields and terminators. Ten of
the eleven errors are intersection curves whose limits have kind `B`, which
the public reference describes for blends and not for intersection curves; the
mapper keeps rejecting them. The other is an invalid reference under the V30
standard key. Neither successful mapping nor this comparison proves OCCT, STEP
or preview conversion.

Public Rust and Python tests cover the new and revised identities, per-key
membership of OFFSET_SURF and the other extra types, an authored offset surface
with its basis reference and signed distance in X_T and X_B, the per-revision
LIST and pointer-block layouts with a distinct value at every ordinal, and
unset trim parameters on lines of both senses together with the cases that
stay rejected.
