# Onshape current-key ellipse and NURBS evidence

Revision 2 of `onshape-sch37102-13006-r2` extends the exact internal key
`SCH_3701212_37102_13006` to elliptical edges and direct B-spline curves and
surfaces. It contains 35 base types / 292 field groups, with canonical hash
`c65102214f88d51a41758fde42a3a691cf3532d0f9341533c5f7c86fa53fa93b`.
This source-tree addition has not been published. Other profiles keep their
existing identities and definitions. The current source profile is revision 3;
[compound-geometry evidence](onshape-composite.md) describes its additional scope.
The revision-2 evidence below remains a historical campaign record.

The ten added types are ELLIPSE (32), BSPLINE_VERTICES (45), B_SURFACE (124),
SURFACE_DATA (125), NURBS_SURF (126), KNOT_MULT (127), KNOT_SET (128), B_CURVE
(134), CURVE_DATA (135), and NURBS_CURVE (136). Their reviewed 13006 base layouts
already had independent V13 evidence. This campaign checks their use with the
current Onshape embedded stream; it does not select neighboring modeller keys.

The parser preserves degrees, homogeneous control coefficients, distinct knots
and multiplicities, rational flags, and periodic/closed flags. For surfaces it
preserves the separate U/V axes and the transmitted periodic overlap. Embedded
fields must retain their reviewed base identity to acquire B-Rep roles; deleting
and reinserting an identically named required reference is rejected.

## Producer and geometry checks

The campaign uses one solid cylinder with an oblique planar cut and six explicit
NURBS sheets: open nonrational, open rational, U-periodic, V-periodic, rational
U-periodic, and rational V-periodic. The surfaces use asymmetric degree-3 × 2
or degree-2 × 3 control grids and nonuniform knots. Their source definitions
are authored through Onshape's
[`bSplineSurface`](https://cad.onshape.com/FsDoc/library.html#bSplineSurface-map)
and `opCreateBSplineSurface` interfaces.

The seven development shapes supplied X_T/X_B pairs, AP242 STEP in metres,
Body Details, mass properties and direct native geometry from the same immutable
Version. Native
[`evSurfaceDefinition`](https://cad.onshape.com/FsDoc/library.html#evSurfaceDefinition-Context-map)
and [`evCurveDefinition`](https://cad.onshape.com/FsDoc/library.html#evCurveDefinition-Context-map)
queries return the surface and boundary B-curve coefficients. Another native
query samples 63 surface positions and normals per sheet.

Every producer pair must parse completely, have valid source B-Rep topology,
and compare equivalent. Guarded API/CLI subprocesses prohibit catalog and network
access and optional CAD imports. The Rust corpus probe separately checks every
raw value and source range and reencodes decoded values. Original X_B bytes are
also retained exactly; this byte-retention check alone is not geometry evidence.

Parsed surface coefficients are compared with both authored and native values.
Independent SciPy basis, tensor de Boor and OCCT evaluation then compare positions
at a grid including interior knots. STEP is imported independently and evaluated
at common surface parameters. Periodic checks include translated parameters,
seam positions and derivatives. Deliberate U/V transposition and omitted rational
division must fail. Native B-curve coefficients are matched one-to-one against
the parsed boundary curves, including their degrees, knots and periodic flags.
These are coefficient and sampled-geometry checks, not a continuous error proof.

Fixed tolerances are 1e-12 for source/native coefficients and numerical surface
positions, 1e-14 for knots, 1e-10 for normals and derivatives, and 1e-8 m for STEP
surface positions. Analytic geometry uses 1e-9 m / 1e-10 direction tolerances.

## Fresh inputs after the implementation freeze

On 2026-09-15, 204 parser, runtime, test, evaluator, collector and recipe files
were frozen at 10:28:08 UTC. Seven new shapes were then created in the approved
public test document. The immutable Version
[`53268f0c63b3aecc1ff8eb7b`](https://cad.onshape.com/documents/d3e1c40fc704383e481210f4/v/53268f0c63b3aecc1ff8eb7b)
was created at 10:29:26 UTC. These inputs change the ellipse cut angle from
37 to 53 degrees, one nonrational surface control-point Z by 0.743021 mm,
or one rational surface weight to 0.583019. All were specified before creation.

All seven new pairs (14 streams) passed the fixed parser and geometry checks,
including native boundary B-curve coefficients, sampled surface geometry,
periodic seams/derivatives, Rust/Python parity, CLI checks and value reencoding.
No frozen file or tolerance changed after capture. The strict ellipse metric
comparison remained outside the verified metric scope, with its failure retained
as described below. The ten earlier analytic producer pairs also passed as
revision-2 regressions; they are not new holdouts for this expansion.

## Metadata and metric boundaries

The separate exact-catalog audit compares decoded values, byte ranges and
normalized B-Rep. It does not claim identical pointer-class metadata. The retained
base definitions differ from the local catalog in these fields:

| Type / field | Built-in pointer class | Local catalog class |
|---|---:|---:|
| 74 / `entries` | 0 | 1001 |
| 125 / `analytic_form` | 0 | 1030 |
| 125 / `swept_form` | 0 | 1031 |
| 125 / `spun_form` | 0 | 1032 |
| 125 / `blend_form` | 0 | 1033 |
| 135 / `analytic_form` | 163 | 1036 |

The SURFACE_DATA/CURVE_DATA forms remain raw metadata. The profile does not infer
semantic support for every referenced class from these examples. The catalog is
not part of runtime, build or installation.

The oblique cylinder's strict formula-versus-integrated-STEP area/volume check
fails in development, as retained in the earlier ellipse campaign. The errors
are approximately 2.01e-9 m² and 1.61e-12 m³; the fresh 53-degree case has
errors of 3.55e-9 m² and 2.83e-12 m³. Both formula and STEP values fall
inside the native producer's reported intervals. Those intervals and analytic
geometry are checked, while the tighter formula claim remains unverified;
tolerances were not enlarged to hide the discrepancy. Curved core metrics remain
unavailable. NURBS surface evidence does not establish general solid mass metrics.

Intersection/trimmed/SP_CURVE families, doubly periodic producer surfaces,
assemblies and other modeller keys are outside this expansion. Synthetic tests
cover framing, truncation, invalid knots/weights and embedded field identity;
real fixtures remain local. This parser work does not add rational/periodic
NURBS conversion to OCCT, STEP export or preview adapters.
