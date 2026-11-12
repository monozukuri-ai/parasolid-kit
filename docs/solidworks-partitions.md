# SolidWorks partition profiles

Fifteen exact `SolidWorks` partition keys are compiled, one profile per key.
`solidworks-sch37102-13006-r4` accepts `SCH_3701229_37102_13006` (SolidWorks
2026); the fourteen earlier keys use `solidworks-<modeller>-<schema>-13006-r3`
identities. Their revisions, canonical hashes and the shared count of 58 base
types / 468 field groups are listed in the
[shared support matrix](format-support.md#supported-profiles). Every profile has
`verified_subset` coverage and exact-key admission; another SolidWorks build of
the same schema is read through the derived
[schema family](format-support.md#schema-families), never through a nearby
exact profile.

## Declarations

All fifteen profiles share one declaration set. Every observed `SolidWorks` key
carries the `13006` base suffix, so the reviewed 13006 subset, its SP_CURVE and
B-spline surface layouts, WORLD (101) and the reviewed surfaces TORUS (54),
BLENDED_EDGE (56), BLEND_BOUND (59), OFFSET_SURF (60) and SPUN_SURF (68) are
reused unchanged from the Onshape and iCAD profiles. The
[assembly structure](builtin-profiles.md#assembly-structure-assembly-instance-and-transform)
ASSEMBLY (10), INSTANCE (11) and TRANSFORM (100), observed in a `SolidWorks`
2020 assembly export, follows the public reference. Base type 204 is recorded
as absent, reusing the existing 13006 membership audit. Version differences
arrive as each stream's embedded schema edits; no definition is guessed from a
nearby key.

SWEPT_SURF (67) is new. Its layout follows the Parasolid XT Format Reference,
April 2008 (printed page 73): the common surface header, the section curve,
the sweep direction and the scale. Every sampled partition and local-body
stream that contains the type reads to its terminator with this layout. The
released B-Rep mapper retains a swept surface as an unsupported surface with
the `geometry.unsupported_surface` diagnostic, so the counts below list those
models as explicitly partial; the unreleased mapper decodes the section curve
and sweep direction as a swept surface.

The helical forms of B-surfaces and B-curves are declared as the streams
write them: node type 163 with the `HELIX_SU_FORM` fields of the
[public XT reference](https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf)
(printed page 72: axis point and direction, hand, turns as one interval,
pitch, gap and tolerance, `vvcifff`), referenced by `SURFACE_DATA` (125) with
analytic form type `'H'`, and node type 184 with the `HELIX_CU_FORM` fields
(printed pages 43-44: axis point and direction, a point, hand, turns, pitch
and tolerance, `vvvciff`), referenced by `CURVE_DATA` (135). The reference's
node-type table (printed page 118) gives the two numbers the other way round,
and revision 2 of these profiles, written for
[issue #13](https://github.com/monozukuri-ai/parasolid-kit/issues/13) from that
table, declared 163 with the curve-form fields; SolidWorks streams then read
out of step at their first descriptor. See
[helical forms and attribute field names](builtin-profiles.md#helical-forms-and-attribute-field-names).
Embedded schema edits operate on the seven field groups of each form,
including the interval as one field. The descriptors are retained in the raw
document; B-Rep conversion continues to use the owning B-surface's and
B-curve's NURBS data, without replacing it by an ideal helix. `FIELD_NAMES`
(99), the names of an attribute definition's fields, is declared from the
same reference (printed pages 102-103).

Authored X_T/X_B fixtures cover both forms with unchanged and edited
definitions, both handedness values, null intervals, repeated descriptors,
application user fields and subsequent records. The reported key and another
build of its family are tested; an authored assembly passes `check` and STEP
export with the descriptors retained. These tests establish the documented
layouts, not complete support for arbitrary assemblies or other uncovered base
types. Unknown types still fail closed; their record lengths cannot be safely
inferred for skipping.

WORLD's eleven base fields are described in the same reference, printed pages
82-83 and 118. Modern indexed-transmit and mesh fields are read from embedded
additions; they are not invented base fields. Copied geometry roles must still
pass the existing embedded-role validation.

## Local evidence, 2026-10-23

Streams were extracted from 63 modern-container `.SLDPRT` files: the local
validation Parts and 50 Parts from eight public repositories pinned by
revision (the extraction and the CAD files stay outside this repository). Each
`Config-N-Partition`, `GhostPartition` and `FeatureBodies/LocalBodies` entry
was decoded as a chunk chain and split at validated `PS` headers. The ignored
test `solidworks_partition_evidence` replays a directory of extracted streams
and records parse, mapping and topology results.

| Stream role | Streams | Complete B-Rep | Explicitly partial | No BODY | Failed closed |
|---|---:|---:|---:|---:|---:|
| Partition (15 keys) | 75 | 47 | 12 swept-surface Parts | 12 | 4 |
| Ghost partition | 58 | 58 | 0 | 0 | 0 |
| LocalBodies transmit | 811 | 797 | 14 swept-surface bodies | 0 | 0 |
| Deltas | 76 | 0 | 0 | 0 | 76 at base type 3 |

Every complete model passes the structural topology validation. The twelve
bodiless partitions belong to Parts whose bodies are stored only in
`FeatureBodies/LocalBodies`; those local-body streams map completely. The four
partition failures are historical results: two partitions of one Part
reached the then-undeclared base type 163 and one base type 184 (now the
declared helical forms; these partitions have not been re-measured), and one
NURBS record carries values after its declared knot multiplicities (the
unreleased mapper ignores such values, as SolidWorks 2019 writes them on
periodic curves; the partition counts have not been re-measured). A sixteenth
key, `SCH_2501231_25001_13006`, was seen in one SolidWorks 2014 Part stored in
the OLE2 container and is not compiled; it fails closed as an unsupported key.

The partition and local-body B-Reps were also converted through the optional
OCCT adapter and compared with the saved display meshes of the same Parts
through sldkit's display-outline oracle. Those results are kept with the
validation inputs outside this repository; they measure the conversion subset
(unsupported surfaces, blends and NURBS validity), not these profiles.

## State boundary

All sampled delta streams stop with `schema.unknown_base_type` at their first
type-3 record. Type 4 also remains unknown. Their unchanged-schema markers
require actual base definitions; a guessed field count or `Absent`
classification would be invalid. The current public reference does not
establish these layouts or the complete state-update rules.

A complete partition B-Rep is not evidence that later deltas were applied.
Callers must retain the stream identities and report unapplied deltas. These
profiles do not replace a SolidWorks document's configuration-aware geometry
decoder, nor establish support for other keys, arbitrary bodies, assembly
semantics, or numeric source trim intervals. Parsing and interpreting
additional delta framing/state rules is required before that migration can
finish.

Embedded schema Copy/Delete/Insert/Append edits change field definitions;
they are separate from delta streams that change model state. The shared
`parasolid_core::partial` readers used by sldkit preserve its bounded recovery
and merge behavior, including explicit incomplete status. That integration
does not change the strict partition/delta boundary described here. Native
containers, configuration selection, stream pairing and length-unit conversion
remain in sldkit's adapter; see the
[caller contract](format-support.md#result-stages-and-caller-responsibilities).
