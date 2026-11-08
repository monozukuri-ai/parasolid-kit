# V30 parametric viewer conversion

The exact stream key `SCH_3000000_30000` now uses
`onshape-sch30000-r3` (44 types / 356 field groups). This extends the existing
viewer pipeline through source UV curves, intersection curves and trimmed
analytic/NURBS faces. It does not infer coordinate units.

## Source decoding

The additional types are 38, 40, 41, 45, 52, 54, 87, 89, 124–128, 133–137,
141 and 204. V30's intersection-data pointer, additional LIMIT character and
version-specific pointer classes are explicit. The V13, iCAD and SolidWorks
profile identities remain unchanged. Their base definitions do not inherit
the new V30 types.

The public [XT format reference](https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf)
describes FIN/EDGE tolerance relationships on printed pages 23–24, intersection
CHART/LIMIT records on 44–48, SP_CURVE on 52, and torus parameterization on 60.
A local exact-key catalog supplied a version-specific audit; it is not a build,
installation or runtime dependency and is not redistributed.

A 10,747,019-byte local X_T was compared through built-in and caller-catalog
parsing. Both produced 161,585 nodes with identical typed field values, field and
node byte ranges, and normalized B-Rep values. The raw comparison digest is
`1f4424b4ff57d7b55a9e9e586cb9879b9b94b0317335d64886695e58e93b7eff`.
The internal stream key governs selection even when the outer export header
names a newer modeller. Synthetic text/binary tests separately exercise the
added roles, arrays and truncation boundaries.

This is development evidence from one local model, plus synthetic regression
oracles. No new producer-generated X_B pair, simultaneous STEP oracle, or
prospective producer holdout was collected for this extension.

## OCCT construction and error boundaries

Open nonrational 2D NURBS are evaluated in their supporting surface's parameter
space. Planar parameters and cylinder/cone lengths scale with source units;
angles and NURBS parameters retain their values. Both FIN representations are
attached to the shared edge. OCCT constructs its 3D curve and reparameterizes
pcurves numerically. Rational/periodic parameter curves remain rejected.

Intersections retain ordered source CHART points and both LIMIT point arrays.
Analytic branches are selected using these positions rather than OCCT line
indices. Other open branches are fitted against both supporting surfaces,
with source trim points as anchors. Each interpolation span is sampled at three
interior parameters; failed spans receive corrected points. The solver stops
at 12 rounds or 10,000 points and rejects unresolved branches. These are
numerical approximations, not a claim that a CHART polyline is exact or a
certified continuous Hausdorff bound.

The source body linear resolution controls intersection refinement. For UV
curves the OCCT 3D approximation budget is the greater of the source edge
tolerance and OCCT's minimum construction precision. Thirty-three samples per
FIN compare the original surface-evaluated curve with the output 3D curve.
The conversion limit combines source edge tolerance, that representation
budget, and the existing `ValidationTolerances` linear threshold. Vertex
neighborhoods retain their separate source vertex tolerances. Exceeding the
conversion limit fails conversion; exceeding the source edge tolerance alone
is retained as `occt.parametric_approximation` in the conversion and preview
reports. Source tolerance values are never rewritten.

The local model has a small adjacent-FIN discrepancy already in its source
geometry: independent curve-on-surface projection on one edge finds about
`2.96e-7` source units against a stored edge tolerance of `1.68e-7`. This is
separate from conversion error. Four converted edges have sampled FIN-to-output
differences above their stored source tolerance; the bounded conversion checks
pass and a warning remains visible.

Lemon tori with negative major radius use the exact source parameterization as
a surface of revolution. Periodic UV shifts, generated seams, ring splitting
and wire orientation are recorded construction operations. All adjacent faces
receive seam replacements before final validation. Face/edge/FIN/loop and
underlying curve mappings retain split, merged and generated relationships.
Material-region shells are kept separate, including cavity boundaries.

Core bounding boxes contain source vertices. Curved OCCT shape bounds must
contain them; equality is required only for polygonal planar models. Curved
planar trim loops do not acquire area or volume estimates from their endpoint
polygons. Available independent source metrics remain checked.

## Meshing and local display evidence

The local run explicitly uses **source mm → target mm, scale 1**. The model name
is not used to rescale it. The following command uses the normal parser,
conversion, mesher, writer and loopback server:

```sh
uv run parasolid-kit viewer 'S - 104mm Mecanum (GB).x_t' --source-unit mm
```

OCCT's `AdjustMinSize` accounts for differently sized faces. Only faces without
triangles receive up to three finer retries, bounded by their own size and the
requested maximum deflection. The manifest records each retry and effective
linear deflection. The [OCCT 7.9.3 parameter definitions](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_9_3/src/IMeshTools/IMeshTools_Parameters.hxx)
explain local minimum-size adjustment. There is no partial-preview bypass.

With the default `0.1 mm` linear and `0.5 rad` angular deflection, the local run
produces one valid solid, 27 shells, 2,949 faces, 6,636 displayed edges and
50,104 triangles. Two faces need a finer retry at about `1.506e-6 mm`.
All 2,949 source faces and 6,338 source edges are represented; the additional
OCCT edges come from periodic seams and boundary splits. Missing face and edge
counts are both zero. These counts describe representation completeness,
not independent producer mass-property qualification.

Local evidence is kept outside distributions:

- `.internal/v30-catalog-parity.json`: complete built-in/caller-catalog comparison.
- `.internal/mecanum-viewer-final-cli.log`: actual CLI result with explicit mm units.
- `.internal/mecanum-preview-mm/`: generated GLB, source manifest and bundled UI.
- `.internal/mecanum-viewer-mm.png`: browser screenshot.
- `.internal/mecanum-browser-verification.json`: actual pointer clicks checked
  against source node IDs and byte ranges extracted before conversion.

The real CAD input and local audit files are excluded from wheel and sdist.
Browser evidence is local Linux/Chromium with SwiftShader; other operating
systems, hardware GPU rendering and remote CI are not qualified by this run.
