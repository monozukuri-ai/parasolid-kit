# Higher-degree, rational and periodic NURBS surface evidence

This campaign checks the existing `onshape-sch13006-r6` parser against explicit
Onshape surfaces and a direct producer geometry oracle. The exact key remains
`SCH_1300000_13006`; the profile still has 40 types / 327 field groups and hash
`2748e9f9c28fa32b59edb9e0b16fea7a976ad081f698dd3d098d9cbd17e08fbb`.
The field layouts and runtime mapper do not change.

The [machine-readable evidence](nurbs-surface-evidence.json) records immutable
public versions, source/export hashes, numerical residuals, the evaluator freeze,
and the test environment. All producer inputs are project-owned synthetic
models. The earlier [STEP accuracy audit](step-accuracy.md) and its reports remain
separate.

## Controlled surfaces

Each motif has a development case and a prospective case. The latter changes
one source value and is exported from a new immutable version after the parser,
test fixtures and numerical evaluator have been frozen.

| Cases | Degrees U × V | Rational | Periodic axis | Transmitted control grid | Prospective change |
|---|---|---|---|---|---|
| D0 / H0 | 3 × 2 | No | None | 5 × 4 | One control-point Z +0.587013 mm |
| D1 / H1 | 3 × 2 | Yes | None | 5 × 4 | One weight: 0.707007 → 0.619031 |
| D2 / H2 | 3 × 2 | No | U | 9 × 4 | One control-point Z +0.587013 mm |
| D3 / H3 | 2 × 3 | No | V | 4 × 9 | One control-point Z +0.587013 mm |

The open patch uses different interior knots in each direction: U = 0.371 and
V = 0.619. Its asymmetric control grid exposes a transposed-axis interpretation.
The rational version changes one interior weight while keeping the same
unweighted control points. The periodic tube starts with six control rows and
repeats the first three; its complete knot vector extends from −0.5 to 1.5 while
the active domain stays [0, 1]. Transposing that source grid and its degrees/knots
provides the independent V-periodic case.

Onshape's [`bSplineSurface` documentation](https://cad.onshape.com/FsDoc/library.html#bSplineSurface-map)
defines the control-grid axes and periodic overlap. Sources call
`opCreateBSplineSurface` with explicit control points, degrees, knots and weights;
weights are supplied as a FeatureScript `Matrix`. The initial array-valued weight
attempt failed the producer's precondition and is archived separately. It did
not produce a validation input or require a parser change.

## Source → producer → parser → independent evaluation

The validation keeps each representation distinct:

1. **Authored definition.** Build expected padded knots and overlapping controls
   from the saved FeatureScript source parameters. Evaluate the tensor-product
   homogeneous spline with SciPy basis functions.
2. **Native producer geometry.** Query the exact immutable version with
   [`evSurfaceDefinition`](https://cad.onshape.com/FsDoc/library.html#evSurfaceDefinition-Context-map),
   using `returnBSplinesAsOther: false`. Select only the face created by the
   controlled feature; selecting all faces would also include default reference
   planes. Save the unweighted control grid, weights, full knots and flags.
   Also save 63 positions and normals per model using
   [`evFaceTangentPlane`](https://cad.onshape.com/FsDoc/library.html#evFaceTangentPlane-Context-map).
   These parameters refer to the face's parameter-space bounding box. All
   campaign faces span the full [0, 1] × [0, 1] surface domain.
3. **V13 X_T/X_B.** Require complete raw and B-Rep parsing, paired equivalence,
   valid topology and byte-preserving X_B reconstruction. A separate Rust runner
   checks both encodings and independently reencodes decoded node values.
   Compare the transmitted homogeneous grid, dimensions, flags and expanded
   knots with both the authored definition and native producer output.
4. **Independent numerical evaluation.** Evaluate parsed coefficients with
   tensor-product de Boor interpolation. Compare with the source basis evaluator
   and a separately constructed OCCT B-spline surface over a grid including
   interior knots. Periodic checks shift parameters by several full periods.
   Deliberately transposed controls and omitted homogeneous division must fail
   the geometric comparison.
5. **STEP representation.** Import the producer's AP242 export in metres and
   compare surface positions at common parameters. This is a surface check;
   it does not certify continuous distance bounds, face area or general boundary
   reconstruction. Normal comparisons use the unoriented tangent plane, allowing
   the face orientation to reverse the normal.

The native query uses no approximation operation. Body Details independently
checks body/face/edge/vertex counts and the version microversion. The local
validation rejects socket activity and external `.sch_txt` reads.

## Verified results

All eight X_T/X_B pairs pass, including four prospective versions created after
the 164-file freeze on 2026-09-07 at 09:24:29 UTC. The frozen files remain
unchanged. There are 2,808 surface-grid samples and 504 native producer samples.

| Comparison | Largest observed residual |
|---|---:|
| Native producer versus parsed homogeneous coefficients | 0 |
| Authored source versus parsed surface position | 3.28e-17 m |
| Parsed surface versus native producer position | 2.89e-17 m |
| Parsed surface versus independent OCCT construction | 2.87e-17 m |
| Authored source versus imported STEP surface | 4.45e-17 m |
| Periodic parameter shifts | 7.76e-18 m |

These values are sampled numerical agreement for the stated motifs. They do
not establish exact continuous equivalence for arbitrary NURBS surfaces.
The focused Python tests pass 131 cases. The full suite passes 525 cases with
56 optional-dependency skips; Ruff and diff whitespace checks pass. The initial
sandbox run could not create the local preview server socket; the full suite
passes when local socket access is available.

## Regression coverage and limits

The repository tests now exercise asymmetric higher-degree grids, nonuniform
knots, homogeneous coordinates and periodic overlap in each axis, through both
text and binary entrypoints. Invalid V-direction degrees, grid counts, knots,
multiplicities and inconsistent periodic/closed flags must still fail B-Rep
mapping while preserving raw parsing.

The producer evidence in this campaign covers the four motifs above. A subsequent
[rational periodic campaign](rational-periodic-surface-evidence.md) adds producer
validation for weighted U-periodic and V-periodic sheets. A further
[doubly periodic campaign](doubly-periodic-surface-evidence.md) verifies
degree-3-by-2 nonrational/rational sheets periodic in both axes. Arbitrary
degrees, implicit surface extensions and other schema keys remain outside
these campaigns.
Preserving periodic/closed flags is distinct from proving the continuity of
arbitrary control grids.

The public optional OCCT adapter retains its documented restrictions on rational
and periodic NURBS. The local oracle builder is an investigation tool, not a new
public evaluator or adapter path. Core curved area/volume are not added.

Local artifacts are under `.internal/m8e-nurbs-surface/`: `campaign.json`,
FeatureScript sources, `collection.json`, `freeze.json`, native geometry responses,
per-model samples, the two validation reports, and PNG/PDF source-surface plots.
With those local inputs and the recorded analysis environment:

```sh
.venv/bin/python .internal/m8e-nurbs-surface/validate.py
.venv/bin/python .internal/m8e-nurbs-surface/validate.py --heldout
```

The collector needs Onshape access; replaying the saved validation inputs does
not. These local inputs/scripts are excluded from distributions. No remote CI or
release result is implied by the local evidence.
