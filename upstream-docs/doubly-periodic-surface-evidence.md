# Doubly periodic NURBS surface evidence

This campaign verifies nonrational and rational surfaces that are periodic in
both U and V. It follows the [single-direction rational periodic campaign](rational-periodic-surface-evidence.md)
and uses the unchanged exact V13 profile `onshape-sch13006-r6` for
`SCH_1300000_13006`.

The [evidence manifest](doubly-periodic-surface-evidence.json) records the
immutable public versions, source/export hashes, evaluator freeze and numerical
results. All models are project-owned synthetic inputs.

## Controlled inputs and topology

Each surface has degree 3 in U and degree 2 in V. An asymmetric ring-shaped
control grid has six unique U rows and five unique V columns. Periodic overlap
adds three rows and two columns, giving a transmitted 9 × 7 grid. This includes
the corner block shared by both overlapping directions.

| Cases | Rational | Prospective change |
|---|---|---|
| D0 / H0 | No | Control point `[1][1]` Z increases by 0.587013 mm |
| D1 / H1 | Yes | Weight `[1][1]` changes from 0.707007 to 0.619031 |

D1 uses the same unweighted control points as D0. Its other nonunit weight is
1.301009 at `[4][3]`; all remaining weights are one. The changed `[1][1]` point
or weight appears in both directions' overlap, exercising the shared corner.
Each prospective definition changes exactly one scalar before the constructor
adds the repeated controls.

The expanded U knots range from −0.5 to 1.5 and the expanded V knots from −0.4
to 1.4. Both active domains are [0, 1]. The producer stores each closed sheet
as one face with zero edges and zero vertices. This is a sheet-body observation,
not a claim that the parser reconstructs a solid or computes its volume.

The sources use explicit control points, knots, degrees and optional weights
with [`bSplineSurface` and `opCreateBSplineSurface`](https://cad.onshape.com/FsDoc/library.html#opCreateBSplineSurface-Context-Id-map).
The latter creates the full surface domain when no boundary curves are supplied.

## Validation

The earlier tensor-product evaluator and native producer queries are reused
with these new inputs. Validation requires:

- Complete X_T/X_B parsing without external schema files, paired equivalence,
  valid B-Rep topology, binary reconstruction and independent Rust value reencoding.
- Agreement of the full homogeneous control grid, expanded knots, dimensions
  and both periodic/closed flags with the authored source and direct native
  `evSurfaceDefinition` output from the same immutable version.
- Independent source-basis, parsed de Boor, OCCT and imported STEP surface
  positions, plus 63 native producer positions and tangent-plane normals per
  model. Normals are compared without assuming identical face orientation.
- Position and both first derivatives at both periodic seams, including their
  intersections. Authored rational quotient derivatives are compared with OCCT.
- Parameter shifts in each direction separately and both directions together.
  Wrong-axis and omitted-weight controls must expose a geometric discrepancy.

The corner coefficients are compared along with the entire transmitted grid;
they are not inferred by wrapping only the final surface positions. The two
prospective versions are created after the implementation, tests and evaluator
freeze, with no subsequent change to those frozen files.

The results are sampled numerical agreement for these motifs. They do not
certify continuous global error bounds, exact face areas or a general STEP
conversion path.

## Results

All four immutable X_T/X_B pairs (eight streams) pass. Two pairs are prospective
versions created after the 188-file freeze at 2026-09-07 10:19:44 UTC. All frozen
files and all 360 saved files from the preceding two surface campaigns remain
unchanged.

| Check | Result across all four models |
|---|---|
| Source / parsed / OCCT / STEP surface samples | 1,764 |
| Direct native producer samples | 252 |
| Seam cross-sections, including shared corners | 168 |
| Native homogeneous coefficient discrepancy | Exactly zero |
| Maximum source-to-STEP position discrepancy | 6.62 × 10⁻¹⁷ m |
| Maximum parsed-to-native position discrepancy | 1.76 × 10⁻¹⁷ m |
| Maximum seam position discrepancy | 9.86 × 10⁻¹⁸ m |
| Maximum seam first-derivative discrepancy | 6.06 × 10⁻¹⁷ m per unit parameter |
| Maximum source-to-OCCT first-derivative discrepancy | 1.16 × 10⁻¹⁶ m per unit parameter |

Derivative units refer to the dimensionless U/V parameters. The position and
derivative maxima above are rounded upward; the manifest retains the full
values and per-model results.

Two regression cases were added for simultaneous U/V periodicity. The focused
surface/curve tests pass 133 cases, and the full base-environment suite passes
527 with 56 optional-dependency skips. Ruff and diff whitespace checks pass.
The runtime, 40-type / 327-field-group profile and its SHA-256 remain unchanged:
`2748e9f9c28fa32b59edb9e0b16fea7a976ad081f698dd3d098d9cbd17e08fbb`.

## Reproducibility and limits

The repository's existing tensor-surface test now covers simultaneous U/V
periodicity for both rational and nonrational wire examples. It checks the
asymmetric grid and the doubly repeated corner through both text and binary
entrypoints. No runtime mapper or schema-layout change is required.

Local artifacts are under `.internal/m8e-nurbs-doubly-periodic/`: source
FeatureScripts, `campaign.json`, `collection.json`, native responses, paired
exports, `freeze.json`, per-model samples, validation reports and PNG/PDF seam
plots. With those local inputs and the recorded analysis environment:

```sh
.venv/bin/python .internal/m8e-nurbs-doubly-periodic/validate.py
.venv/bin/python .internal/m8e-nurbs-doubly-periodic/validate.py --heldout
```

Replay rejects network activity and external `.sch_txt` reads. The collector
requires Onshape access; the local collection and scripts are excluded from
distributions. Earlier surface-campaign inputs and reports remain unchanged.

This establishes producer evidence for these degree-3-by-2 doubly periodic
motifs. Arbitrary degrees, implicit surface extensions and other schema keys
remain outside the campaign. The public optional OCCT adapter keeps its existing
rational/periodic restrictions. No public evaluator, curved metrics, remote CI
or release result is added by this investigation.
