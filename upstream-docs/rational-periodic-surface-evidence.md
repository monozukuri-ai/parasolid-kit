# Rational periodic NURBS surface evidence

This campaign combines rational weights with periodicity in each surface axis.
It extends the producer evidence from the earlier
[NURBS surface campaign](nurbs-surface-evidence.md), using the same exact V13
profile, `onshape-sch13006-r6`, for `SCH_1300000_13006`. The runtime decoder,
mapper, 40-type / 327-field-group profile and its hash remain unchanged.

The [evidence manifest](rational-periodic-surface-evidence.json) contains the
immutable public version links, source/export hashes, numerical results and
validation environment. All models are project-owned synthetic inputs.

## Controlled inputs

| Cases | Degrees U × V | Periodic direction | Unique control grid | Transmitted grid |
|---|---|---|---|---|
| D0 / H0 | 3 × 2 | U | 6 × 4 | 9 × 4 |
| D1 / H1 | 2 × 3 | V | 4 × 6 | 4 × 9 |

Both motifs are rational. D0 starts with the previous nonrational tube's control
points and applies weights 0.707007 at `[2][1]` and 1.301009 at `[0][2]`; all
other weights are one. D1 transposes the complete definition, including weights,
degrees, knots and periodic flags. The first three rows or columns overlap at
the periodic seam, including their nonunit homogeneous weights.

H0 and H1 change only the 0.707007 weight to 0.619031, at `[2][1]` and `[1][2]`
respectively. Each is a new immutable version created after the evaluator freeze.
An explicit comparison of the saved source definitions verifies that exactly one
scalar value changes in each prospective case.

The periodic direction retains its complete knot vector from −0.5 to 1.5 and
active domain [0, 1]. The other direction has an interior knot at 0.619.
These are transmitted coefficients and flags; no periodic pole or weight is
discarded, normalized away, or replaced by an analytic surface.

## Independent checks

The procedure follows the previous surface campaign: complete X_T/X_B parsing
without an external schema, decoded equivalence, valid B-Rep topology, binary
reconstruction, and independent Rust reencoding of node values. Source formulas,
direct native producer coefficients and positions, and OCCT surface evaluations
provide separate comparisons. All native responses use the same immutable
version microversion as the corresponding exports.

Onshape's [`bSplineSurface`](https://cad.onshape.com/FsDoc/library.html#bSplineSurface-map)
constructs the explicit weighted surface. Native coefficients come directly from
[`evSurfaceDefinition`](https://cad.onshape.com/FsDoc/library.html#evSurfaceDefinition-Context-map),
with the query restricted to the generated face and no approximation operation.
The source evaluator uses tensor-product basis functions and homogeneous
division; a separate evaluator applies de Boor interpolation to parsed values.
The local OCCT construction retains the full overlapping poles and imaginary
knots without applying periodic padding a second time.

Additional seam checks compare positions and both first parameter derivatives
at the beginning and end of the periodic domain. Derivatives from the authored
rational quotient formula are independently compared with OCCT. Parameter shifts
by several periods, transposed-axis controls and omitted homogeneous division
exercise the expected failure modes. Native tangent-plane normals are compared
without assuming the same face orientation.

The STEP comparison samples the full underlying surface at common parameters.
These tests do not certify continuous global error bounds, exact face areas or
general trimming/export reconstruction.

## Verified results

All four X_T/X_B pairs pass, including two prospective versions created after
the 176-file freeze at 2026-09-07 10:07:47 UTC. Validation covers 1,512 surface
samples, 252 native producer samples and 72 seam cross-sections. Frozen files
and the previous surface campaign's files remain unchanged.

| Comparison | Largest observed residual |
|---|---:|
| Native producer versus parsed homogeneous coefficients | 0 |
| Native producer versus parsed surface position | 2.78e-17 m |
| Authored source versus parsed surface position | 3.05e-17 m |
| Authored source versus imported STEP surface | 3.76e-17 m |
| Beginning versus end of periodic seam: position | 1.59e-17 m |
| Beginning versus end of periodic seam: first derivatives | 4.16e-17 m |
| Authored quotient derivatives versus OCCT derivatives | 8.84e-17 m |

Derivative units are metres per unitless surface parameter. The focused suite
passes 131 tests; Ruff and diff whitespace checks also pass. This campaign
requires no parser, profile or repository-test changes.

## Reproducibility and limits

Local sources, native responses, X_T/X_B/STEP exports, per-model samples, reports,
the freeze and PNG/PDF control-weight diagrams are under
`.internal/m8e-nurbs-rational-periodic/`. With the saved inputs and recorded
analysis environment:

```sh
.venv/bin/python .internal/m8e-nurbs-rational-periodic/validate.py
.venv/bin/python .internal/m8e-nurbs-rational-periodic/validate.py --heldout
```

The validation rejects network activity and external `.sch_txt` reads. Collection
requires Onshape access; the local artifacts are excluded from distributions.
The earlier campaign's files and reports are preserved unchanged.

The existing synthetic wire tests already cover rational periodic grids in each
axis. Those focused tests are rerun for this campaign; no duplicate tests or
runtime change are needed. The full-suite result from the preceding campaign
remains historical evidence rather than a new full-suite run here.

This establishes producer evidence for these singly periodic rational motifs.
The subsequent [doubly periodic campaign](doubly-periodic-surface-evidence.md)
adds degree-3-by-2 nonrational/rational sheets periodic in both axes. Arbitrary
degrees, implicit surface extensions and other schema keys remain outside
these campaigns. The public optional OCCT adapter
retains its existing rational/periodic restrictions; the investigation evaluator
does not add a public conversion path. No remote CI or release result is claimed.
