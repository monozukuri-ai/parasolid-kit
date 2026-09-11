# SolidWorks partition profile

`solidworks-sch37102-13006-r1` accepts the exact key
`SCH_3701229_37102_13006` with zero user fields. Its revision, canonical hash
and stage-specific support are listed in the
[shared support matrix](format-support.md#supported-profiles).

The profile shares the 40 reviewed 13006 base definitions, including NURBS,
with the V13 profile and adds WORLD (101). Its eleven base fields are described
in the [Parasolid XT Format Reference, April 2008](https://ww3.cad.de/foren/ubb/uploads/Rainer%2BSchulze/XT_Format_April_2008_tcm73-62642.pdf),
printed pages 82-83 and 118. Modern indexed-transmit and mesh fields are read
from embedded additions; they are not invented base fields. Copied geometry
roles must still pass the existing embedded-role validation.

Local development validation used two partitions from an authored SolidWorks
2026 SP0.0 M5 Part, followed by two reserved partitions from two previously
authored Parts. These are existing local inputs, not newly generated producer
holdouts. All four partitions parsed to their terminators and mapped to complete
partition B-Rep models. The raw record counts were 173, 406, 224 and 109 (912
total). An independent value encoder reproduced every node-stream byte without
using the original field ranges. Embedded schema blobs were replayed, so this
does not independently validate schema serialization.
The partition body counts were 1, 3, 1 and 1. The three-body example is evidence
for that saved partition, not arbitrary multi-body/configuration compatibility.

For the M5 pair, comparison against sldkit's existing patched decoder confirmed
exact equality of all 28 point positions, the traversal/edge/loop/sense relations
of all 76 visible FINs, and all three NURBS definitions' poles, degrees, counts
and expanded knots after the documented metre-to-millimetre conversion. Surface
identity was joined through the native face reference: sldkit assigns a face
binding ID while this model preserves a distinct source surface ID. The original
sldkit output has separate SolidWorks/STEP evidence; this run did not recapture
SolidWorks or independently repeat STEP evaluation. M5 uses unit NURBS weights.

Synthetic Rust tests cover exact profile identity, unknown base membership,
WORLD text/binary values, source ranges, value reencoding, every truncated binary
prefix, invalid logical values and rejection of the first delta record.
Private CAD/STEP fixtures and their extracted streams are not distributed.

## State boundary

All four associated delta streams stop with `schema.unknown_base_type` at their
first type-3 record. Type 4 also remains unknown. Their unchanged-schema markers
require actual base definitions; a guessed field count or `Absent` classification
would be invalid. The current public reference does not establish these layouts
or the complete state-update rules.

A complete partition B-Rep is not evidence that later deltas were applied.
Callers must retain the stream identities and report unapplied deltas. This
profile does not replace a SolidWorks document's configuration-aware geometry
decoder, nor establish support for other V37 keys, arbitrary bodies, assembly
semantics, or numeric source trim intervals. Parsing and interpreting additional
delta framing/state rules is required before that migration can finish.

Embedded schema Copy/Delete/Insert/Append edits change field definitions;
they are separate from delta streams that change model state. The shared
`parasolid_core::partial` readers used by sldkit preserve its bounded recovery
and merge behavior, including explicit incomplete status. That integration
does not change the strict partition/delta boundary described here. Native
containers, configuration selection, stream pairing and length-unit conversion
remain in sldkit's adapter; see the
[caller contract](format-support.md#result-stages-and-caller-responsibilities).
