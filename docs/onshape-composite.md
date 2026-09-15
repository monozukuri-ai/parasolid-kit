# Onshape compound-geometry evidence

The unreleased `onshape-sch37102-13006-r3` profile applies only to
`SCH_3701212_37102_13006`. Its 41 compiled base types / 339 field groups have hash
`3f9489b24874ca7857e48d8daf106bcf821f612b49e0d60650b20e132e04abaa`.

The six added base types are INTERSECTION (38), CHART (40), LIMIT (41),
TRIMMED_CURVE (133), SP_CURVE (137) and shared geometry owner (141). Their
definitions reuse the explicitly reviewed V13 layouts. Unknown neighbouring
keys and types remain rejected; this does not enable arbitrary Parasolid parsing.

PART_XMT_BLOCK (176) and INTERSECTION_DATA (204) are absent from base 13006.
A separate membership-only audit of the complete base type table confirmed
those two absences; no catalog field layout was copied into the implementation.
The decoder consequently requires their full definitions from the input.
Type 176 is retained as raw multi-part transmit data. It adds no assembly,
occurrence-transform or native saved-state reconstruction contract.

## Input classification

Seven project-authored Onshape families were captured as X_T/X_B pairs and STEP
from one immutable Version on 2026-09-15. Native Body Details, mass properties,
curve samples and exposed B-spline definitions identify that same microversion.

| Family | Before revision 3 | Added records exercised |
|---|---|---|
| Cylinder with a transverse hole | Unknown base membership | 38, 40, 41, 141, fully declared 204 |
| Cylinder with an oblique hole | Unknown base membership | 38, 40, 41, 141, fully declared 204 |
| Near-tangent cylinder union | Unknown base membership | 133, 137, 141 |
| Spline contours imprinted on a cylinder | Unknown base membership | 38, 40, 41, 133, 141, fully declared 204 |
| Three-section elliptical loft | Already parsed | Existing direct NURBS surface definitions |
| Circular section swept along a spline | Already parsed | Existing direct NURBS surface definitions |
| Filleted drilled block and a separate cylinder | Unknown base membership | Fully declared 176 |

All fourteen development streams now reach complete source B-Rep and valid
topology; each producer X_T/X_B pair compares equivalent. Those development
inputs are regression evidence, not unseen holdouts.

## Fresh holdouts

The parser, native module, Rust probe, runtime tests, geometry oracles, collection
code, recipes and tolerances were frozen as 193 file hashes at
2026-09-15 15:08:26 UTC. Seven new models with predeclared different dimensions
were then created; their immutable Onshape Version was created at 15:10:03 UTC.
The frozen files were unchanged before and after validation.

All seven new X_T/X_B pairs (14 streams, 2,528 records) passed complete B-Rep,
native topology, Rust/Python/API/CLI agreement, paired-document equivalence,
decoded-value reencoding and the required geometry criteria below. No holdout
was used to modify the implementation or relax a tolerance.

The new intersection boundaries differ from STEP by about 3.3–9.2 µm; the
near-tangent SP_CURVE differs by 0.33 µm. These meet their declared tolerances
but fail the separate 10 nm criterion. The new loft retains the predeclared
STEP exception: approximately 8.0 µm deviation against a 10 nm declaration.
Its native NURBS coefficients and independent numerical checks pass. The sweep
and filleted multi-body cases also pass the strict distance checks.

## Geometry acceptance and retained discrepancies

The release oracle compares native topology, vertices and analytic primitives,
and the complete native coefficient/knot/flag arrays of exposed NURBS faces and
direct B-curves. An independent de Boor evaluator and OCCT evaluate the parsed
coefficients. Periodic translations, seam positions/derivatives, and deliberate
control-order or homogeneous-division mistakes are checked separately.

Intersection chart and limit points must lie on both supporting surfaces and
match the STEP boundary within the file's declared uncertainty. Trim endpoints
must meet the recorded source tolerance. Some auxiliary NURBS surfaces exist
only to define intersections: they are not native faces and their coefficient
arrays are not exposed by the native face query. Their numerical evaluation and
intersection consistency are checked; independent native coefficient agreement
is not claimed for these auxiliary surfaces.

Strict 10 nm comparisons remain visible separately from declared tolerances.
Development intersections differ from STEP by approximately 3.5–9.3 µm while
STEP declares 20 µm uncertainty. One near-tangent SP_CURVE differs by about
0.25 µm; its associated source edge carries 10 µm tolerance.

The development loft's native NURBS coefficients agree, but its STEP surface
differs by approximately 7.7 µm despite STEP declaring 10 nm uncertainty. The
loft family explicitly records `step_surface_checks: report_only` and an
exception reason. That STEP accuracy failure is retained; it does not pass the
declared-accuracy criterion. Native coefficient agreement and the independent
numerical checks remain required. No tolerance is enlarged to turn this result
into a STEP accuracy success.

This campaign does not certify curved area/volume, continuous geometric error,
or general OCCT/STEP/preview conversion. Real fixture files remain local-only.
The earlier [ellipse and direct NURBS evidence](onshape-current-parametric.md)
retains its own scope, native sampling and integrated-metric limitations.
