# SolidWorks partition profiles

Fifteen exact `SolidWorks` partition keys are compiled, one profile per key.
`solidworks-sch37102-13006-r2` accepts `SCH_3701229_37102_13006` (SolidWorks
2026); the fourteen earlier keys use `solidworks-<modeller>-<schema>-13006-r1`
identities. Their revisions, canonical hashes and the shared count of 47 base
types / 412 field groups are listed in the
[shared support matrix](format-support.md#supported-profiles). Every profile has
zero user fields, `verified_subset` coverage and exact-key admission; a nearby
key is never selected.

## Declarations

All fifteen profiles share one declaration set. Every observed `SolidWorks` key
carries the `13006` base suffix, so the reviewed 13006 subset, its SP_CURVE and
B-spline surface layouts, WORLD (101) and the reviewed surfaces TORUS (54),
BLENDED_EDGE (56), BLEND_BOUND (59), OFFSET_SURF (60) and SPUN_SURF (68) are
reused unchanged from the Onshape and iCAD profiles. Base type 204 is recorded
as absent, reusing the existing 13006 membership audit. Version differences
arrive as each stream's embedded schema edits; no definition is guessed from a
nearby key.

SWEPT_SURF (67) is new. Its layout follows the Parasolid XT Format Reference,
April 2008 (printed page 73): the common surface header, the section curve,
the sweep direction and the scale. Every sampled partition and local-body
stream that contains the type reads to its terminator with this layout. The
B-Rep mapper retains a swept surface as an unsupported surface with the
`geometry.unsupported_surface` diagnostic; the model is explicitly partial and
no sweep semantics are assigned. HELIX_CU_FORM (163) was tried with a guessed
layout and desynchronized two streams, so it stays unknown and fails closed.

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
partition failures are recorded, not worked around: two partitions of one Part
reach HELIX_CU_FORM (163), one reaches an unreviewed base type 184, and one
NURBS record carries values after its declared knot multiplicities. A sixteenth
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
