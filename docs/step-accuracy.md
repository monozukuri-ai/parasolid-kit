# STEP comparison accuracy audit

The saved V13 fixtures show two different causes for the previously reported
curve discrepancies: approximate STEP boundaries in the higher-degree campaign,
and an existing FIN/EDGE discrepancy inside the original tolerant cylinder models.
Neither result identifies a parser defect. The area differences are explained
by the different boundary representations, including the pcurves constructed
during STEP import.

This is a **local replay on 2026-09-07**, using the existing immutable synthetic
exports: seven higher-degree X_T/X_B pairs with 13 SP_CURVEs, and four cylinder
pairs with eight FIN SP_CURVEs. It adds no prospective holdout or support scope.
The parser, profiles, source tolerances and historical validation thresholds
remain unchanged. Aggregate results, environment versions, immutable public
model links and input hashes are in [the audit data](step-accuracy-data.json).

## Curve discrepancy before and after import

The higher-degree audit follows this path:

1. Parse paired X_T/X_B files with the existing built-in profile and check their
   decoded equivalence. Evaluate homogeneous UV coefficients using SciPy's
   B-spline basis and quotient rule, with the earlier independent de Boor
   evaluator as a cross-check. Map UV to the cylinder or bilinear source surface.
2. Read the STEP file's nonrational `B_SPLINE_CURVE_WITH_KNOTS` coefficients
   directly, without OCCT. The audit reader only handles the simple entities
   present in these metre-valued fixtures; it is not a general STEP parser.
3. At 257 source parameters per curve, find the closest point on every active
   STEP polynomial span. Check stationary roots and endpoints, using normalized
   span coordinates. Cross-check selected points and each sampled peak with a
   separate bounded optimizer, and all distances with OCCT's edge-distance query.
4. Compare the raw STEP and imported OCCT curves at 1,025 common parameters each.

SciPy documents the coefficient basis, active domain and derivatives in its
[BSpline reference](https://docs.scipy.org/doc/scipy/reference/generated/scipy.interpolate.BSpline.html).
Analytic line, multiple-minimum parabola and rational quarter-circle checks
exercise the independent distance and area calculations, including a deliberately
displaced point.

| Higher-degree case | SP_CURVEs | Largest sampled source-to-raw-STEP distance (µm) |
|---|---:|---:|
| D2, cylinder wraps | 3 | 3.147793 |
| D3, planar rational boundary | 1 | 2.651924 |
| D4, warped rational boundary | 1 | 2.667353 |
| H1, initial heldout rational boundary | 1 | 6.868673 |
| H2, initial heldout cylinder wraps | 3 | 3.107625 |
| H3, final heldout rational boundary | 1 | 3.438940 |
| H4, final heldout cylinder wraps | 3 | 3.123691 |

Across all 13 curves, raw STEP versus imported OCCT positions differ by at most
**4.35e-17 m**. The two independent closest-point searches differ by at most
9.96e-14 m at their cross-check points. OCCT's closest-distance result differs
from the polynomial search by at most 3.70e-9 m across all 3,341 source samples;
this numerical difference cannot explain the micrometre-scale peaks.

For example, D3's source is a degree-3 rational periodic UV curve with seven
homogeneous coefficients. STEP stores a degree-3 nonrational 3D curve with 19
control points. Its 2.65 µm discrepancy is reproducible from those saved
coefficients before import. This locates the mismatch between the exported
representations and supports boundary approximation as its cause; the Onshape
exporter's internal fitting algorithm was not inspected.

All sampled distances are below the files' declared 20 µm uncertainty. This is
a consistency observation, not a certified continuous or bidirectional Hausdorff
bound. H3's denser sampling finds a slightly larger peak than the original
129-point report; the original report is retained unchanged.

## Existing tolerant cylinder FIN/EDGE discrepancy

The earlier degree-1 cylinder campaign has a separate explanation. Each relevant
source EDGE has a trimmed circle, while its two FINs use trimmed SP_CURVEs on
their respective surfaces. Comparing each FIN with its own source EDGE already
reproduces the discrepancy seen against STEP.

Across the four models and eight FIN curves, 257 samples each:

- The largest source FIN-to-source EDGE distance is **3.844262 µm**, within
  the source edge's **10 µm** tolerance.
- Replacing the source circle with the circle read directly from STEP changes
  those distances by at most **7.76e-18 m**.
- The STEP circle's centre, radius and normal are preserved on OCCT import.

Thus the approximately 1.03% overrun of the imported edge tolerance in the old
reports does not demonstrate a newly introduced geometric error. These observed
source relationships must be retained rather than replacing FINs with circles
or wrapping their UV angles.

## Area discrepancy follows the boundary

Area is integrated independently using Green's theorem in source UV coordinates.
For cylinders the surface Jacobian is the radius. For the bilinear sheets,
`x = 0.013 + a*u`, `y = -0.017 + b*v`, `z = 0.023 + h*u*v`, where
`a = 0.020013 m`, `b = 0.030019 m` and `h` is zero or 0.002307 m. The Jacobian is
`a*b*sqrt(1 + (h*v/a)^2 + (h*u/b)^2)`. Integrating its antiderivative in `u`
against `dv` gives the enclosed area. Open cylinder patches include the exact
generator and constant-height arc contributions closing the boundary.

Four quantities are kept separate: the original UV boundary's area, the saved
Body Details area, the raw STEP 3D boundary projected onto the source surface,
and the imported face's area. Projection uses the source cylinder angle/height
or the bilinear surface's inverse XY map; it is explicitly a diagnostic projection,
not an exact STEP pcurve or an orthogonal closest-point claim.

| Case | Source − Body Details (mm²) | Source − projected STEP (mm²) | Projected STEP − imported face (mm²) |
|---|---:|---:|---:|
| D2 | +0.000039253 | +0.019324922 | +0.000011695 |
| D3 | −0.000003770 | +0.002627542 | 0 |
| D4 | −0.000003694 | +0.002626112 | +0.000023399 |
| H1 | −0.000057892 | −0.020443811 | +0.000124262 |
| H2 | +0.000026243 | +0.018968024 | +0.000011606 |
| H3 | −0.000005130 | +0.003773693 | +0.000018757 |
| H4 | +0.000028261 | +0.019109818 | +0.000011649 |

Every source-boundary area agrees with Body Details within the original
1e-10 m² gate; the largest absolute difference is **5.79e-11 m²**. No producer
mass-property uncertainty interval is available for these sheets, so this is a
numerical comparison, not proof of exact producer area.

The dominant area change follows the STEP boundary approximation. To investigate
the smaller final column, a further integral uses each imported face's actual
pcurve and surface derivatives. It reproduces OCCT's face areas within
**2.85e-19 m²** per model. Thus that column measures the change from the explicit
diagnostic projection to the importer-created boundary; it is not unexplained
area quadrature error. Changing OCCT's relative integration request from 1e-8 to
1e-12 changes a face area by at most 2.72e-20 m² in these fixtures. OCCT documents
this `Eps` argument as an integration accuracy parameter in
[BRepGProp](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_9_3/src/BRepGProp/BRepGProp.hxx).

## Interpretation of tolerances

An imported edge tolerance concerns the edge's internal geometry, including its
3D curve and face pcurves. It is not an error allowance between independently
exported Parasolid and STEP curves. OCCT's translator initializes some edges at
`Precision::Confusion()` and adjusts tolerances during translation; the resulting
value can be smaller or larger than the STEP file uncertainty. See the
[OCCT 7.9.3 STEP translator tolerance documentation](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_9_3/dox/user_guides/step/step.md).

The audit also samples imported 3D-curve/pcurve consistency. Nine of the 13
higher-degree edges exceed their own recorded tolerance at some sampled common
parameters; the largest ratio is 1.332, despite passing the default shape-validity
check. Therefore neither that check nor tolerance metadata supplies a certified
continuous error bound. No tolerance healing was forced for this audit.

Future evidence should report separate checks for:

- **Parsing and source geometry:** decoded values, topology, homogeneous weights,
  knots, active domains and independent source evaluation.
- **Source tolerance relationships:** FIN-to-EDGE or adjacent-FIN consistency
  with the tolerance supplied by that same source model.
- **Cross-format approximation:** sampled source-to-STEP distance with the file
  uncertainty, sample coverage and direction stated explicitly.
- **Importer consistency:** raw STEP versus imported 3D geometry, imported
  3D-curve/pcurve residuals, and integration convergence.

The previous per-edge, fixed 10 nm and fixed 1e-10 m² failures remain observable
results. Their thresholds are not widened or silently substituted with 20 µm.
They alone do not establish a parser failure in the presence of these different
boundary representations.

## Local replay artifacts

`.internal/m8e-step-accuracy/` contains `reference.py`, `audit.py`, `seams.py`,
`pcurve_area.py`, analytic numerical checks, per-curve samples, JSON reports and
PNG/SVG/PDF plots. On this checkout, replay with the recorded analysis environment:

```sh
.venv/bin/python .internal/m8e-step-accuracy/run.py
```

The replay rejects socket activity and external `.sch_txt` reads. It verifies
147 frozen source/evaluator files and confirms that all 448 prior campaign files
have identical hashes before and after the run. The saved inputs and replay
scripts are local investigation artifacts excluded from distributions; this
command requires those local artifacts and the versions listed in the audit data.
SciPy and Matplotlib were installed only in the analysis environment, with no
package dependency change. No fresh export, remote CI or release is claimed.
