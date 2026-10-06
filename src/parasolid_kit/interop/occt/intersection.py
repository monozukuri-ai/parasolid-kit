"""Bounded numerical reconstruction of a source-identified intersection branch.

CHART points identify a branch; they are not themselves an exact 3D curve.
The fit is constrained by both support surfaces and by source trim endpoints.
Closed branches become periodic curves; unresolved/singular refinement is rejected.
"""

from __future__ import annotations

from itertools import pairwise
from math import isfinite


def project_curve(curve: object, point: object) -> tuple[float, float]:
    """Include finite endpoints, which OCCT's orthogonal projection may omit."""
    from OCP.GeomAPI import GeomAPI_ProjectPointOnCurve

    projection = GeomAPI_ProjectPointOnCurve(point, curve)
    candidates = [
        (projection.Parameter(i), projection.Distance(i))
        for i in range(1, projection.NbPoints() + 1)
    ]
    for parameter in (curve.FirstParameter(), curve.LastParameter()):
        if isfinite(parameter) and abs(parameter) < 1e99:
            candidates.append((parameter, point.Distance(curve.Value(parameter))))
    if not candidates:
        raise ValueError("intersection curve projection has no solution")
    return min(candidates, key=lambda item: item[1])


def _dot(a: tuple, b: tuple) -> float:
    return sum(x * y for x, y in zip(a, b, strict=True))


def _cross(a: tuple, b: tuple) -> tuple:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def fit_intersection(surfaces: list, samples: list, anchors: list, tolerance: float) -> object:
    """Refine an open branch within the source body's linear resolution.

    Every interpolation span is checked against both surfaces at three interior
    parameters. Failed spans receive corrected chord samples. Twelve rounds and
    10,000 points bound the solver; failure never returns an unchecked fit.
    """
    from OCP.GeomAPI import GeomAPI_Interpolate, GeomAPI_ProjectPointOnSurf
    from OCP.gp import gp_Pnt, gp_Vec
    from OCP.TColgp import TColgp_HArray1OfPnt

    numerical = max(1e-14, tolerance * 1e-4)
    if len(samples) + len(anchors) > 10_000:
        raise ValueError("intersection input exceeds 10000 points")

    def project(point: object, surface: object) -> tuple:
        operation = GeomAPI_ProjectPointOnSurf(point, surface, numerical)
        if not operation.NbPoints():
            raise ValueError("intersection point/surface projection failed")
        u, v = operation.LowerDistanceParameters()
        q, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
        surface.D1(u, v, q, du, dv)
        normal = du.Crossed(dv)
        normal.Normalize()
        return q, normal.Coord(), operation.LowerDistance()

    def refine(point: object, tangent: tuple) -> object:
        anchor = point.Coord()
        for _ in range(30):
            q1, n1, d1 = project(point, surfaces[0])
            q2, n2, d2 = project(point, surfaces[1])
            if max(d1, d2) <= tolerance:
                return point
            xyz = point.Coord()
            rhs = [
                _dot(tuple(x - y for x, y in zip(xyz, q.Coord(), strict=True)), n)
                for q, n in ((q1, n1), (q2, n2))
            ]
            rhs.append(_dot(tuple(x - y for x, y in zip(xyz, anchor, strict=True)), tangent))
            columns = (_cross(n2, tangent), _cross(tangent, n1), _cross(n1, n2))
            determinant = _dot(n1, columns[0])
            if abs(determinant) < 1e-12:
                raise ValueError("singular intersection refinement")
            point = gp_Pnt(
                *(
                    xyz[j] - sum(rhs[k] * col[j] for k, col in enumerate(columns)) / determinant
                    for j in range(3)
                )
            )
        raise ValueError("intersection refinement did not converge")

    points = []
    for point in samples:
        if any(not isfinite(x) for x in point.Coord()):
            raise ValueError("intersection branch contains a non-finite point")
        if not points or point.Distance(points[-1]) > numerical:
            points.append(point)
    # A branch whose chart returns to its first point is a closed loop; the
    # periodic interpolation closes it, so the repeated point is dropped.
    closed = len(points) >= 3 and points[0].Distance(points[-1]) <= numerical
    if closed:
        points.pop()
    if len(points) < 2 or (not closed and points[0].Distance(points[-1]) <= numerical):
        raise ValueError("intersection branch has fewer than two distinct points")
    for point in [*points, *anchors]:
        if max(project(point, surface)[2] for surface in surfaces) > tolerance:
            raise ValueError("source intersection point is outside its support-surface tolerance")
    for iteration in range(12):
        array = TColgp_HArray1OfPnt(1, len(points))
        for index, point in enumerate(points, 1):
            array.SetValue(index, point)
        fit = GeomAPI_Interpolate(array, closed, numerical)
        fit.Perform()
        if not fit.IsDone():
            raise ValueError("intersection interpolation failed")
        curve = fit.Curve()
        if iteration == 0 and anchors:
            # Source trim parameters are not OCCT curve parameters. Insert the
            # exact trim points in branch order before refining the interior.
            ordered = [(project_curve(curve, p)[0], p) for p in points]
            for point in anchors:
                parameter, _ = project_curve(curve, point)
                ordered = [item for item in ordered if abs(item[0] - parameter) > numerical]
                ordered.append((parameter, point))
            points = [item[1] for item in sorted(ordered, key=lambda item: item[0])]
            continue
        refined = []
        worst = 0.0
        spans = list(pairwise(points))
        if closed:
            spans.append((points[-1], points[0]))
        for left, right in spans:
            a, _ = project_curve(curve, left)
            b, _ = project_curve(curve, right)
            if closed and b <= a:
                b += curve.Period()
            refined.append(left)
            for fraction in (0.25, 0.5, 0.75):
                point = curve.Value(a + (b - a) * fraction)
                distance = max(project(point, surface)[2] for surface in surfaces)
                worst = max(worst, distance)
                if distance > tolerance:
                    chord = tuple(y - x for x, y in zip(left.Coord(), right.Coord(), strict=True))
                    magnitude = _dot(chord, chord) ** 0.5
                    if magnitude <= numerical:
                        raise ValueError("intersection refinement has a degenerate interval")
                    seed = gp_Pnt(
                        *(x + d * fraction for x, d in zip(left.Coord(), chord, strict=True))
                    )
                    refined.append(refine(seed, tuple(x / magnitude for x in chord)))
        if worst <= tolerance:
            return curve
        if not closed:
            refined.append(points[-1])
        points = refined
        if len(points) > 10_000:
            raise ValueError("intersection refinement exceeds 10000 points")
    raise ValueError("intersection refinement exceeds 12 rounds")


def _closed(curve: object, tolerance: float) -> bool:
    """Whether a branch returns to its start within the conversion tolerance.

    OCCT's own closure test uses its confusion tolerance, which an approximated
    branch of a plane and a surface of revolution does not always meet.
    """
    first, last = curve.FirstParameter(), curve.LastParameter()
    # OCCT bounds an unbounded branch, such as a hyperbola, by a huge finite
    # parameter; evaluating there overflows.
    if any(not isfinite(value) or abs(value) >= 1e99 for value in (first, last)):
        return False
    return curve.IsClosed() or curve.Value(first).Distance(curve.Value(last)) <= tolerance


def _follows_source_order(curve: object, points: list, tolerance: float) -> bool:
    """Whether the curve parameter rises along the ordered source points.

    The parameters of a closed branch are unwrapped across its range, so an
    arc that crosses the parameter origin of the branch keeps its source
    direction instead of being judged by its two end parameters alone.
    """
    parameters = [project_curve(curve, point)[0] for point in points]
    if _closed(curve, tolerance):
        period = (
            curve.Period() if curve.IsPeriodic() else curve.LastParameter() - curve.FirstParameter()
        )
        for index in range(1, len(parameters)):
            step = parameters[index] - parameters[index - 1]
            parameters[index] = parameters[index - 1] + (step + period / 2) % period - period / 2
    return parameters[-1] >= parameters[0]


def intersect_analytic(surfaces: list, samples: list, tolerance: float) -> object:
    """Select and connect OCCT branches using ordered source points, not line IDs."""
    from OCP.GeomAPI import GeomAPI_IntSS
    from OCP.GeomConvert import GeomConvert, GeomConvert_CompCurveToBSplineCurve
    from OCP.gp import gp_Pnt, gp_Trsf

    extent = max(samples[0].Distance(point) for point in samples)
    if extent <= tolerance:
        raise ValueError("intersection branch has no resolved extent")
    # Internal conditioning only; the returned curve retains the requested units.
    forward, backward = gp_Trsf(), gp_Trsf()
    forward.SetScale(gp_Pnt(), 1 / extent)
    backward.SetScale(gp_Pnt(), extent)
    operation = GeomAPI_IntSS(*(s.Transformed(forward) for s in surfaces), 1e-9)
    if not operation.IsDone() or not operation.NbLines():
        raise ValueError("OCCT surface intersection has no branch")
    curves = [operation.Line(i).Transformed(backward) for i in range(1, operation.NbLines() + 1)]
    assignments = []
    for point in samples:
        choices = sorted((project_curve(curve, point)[1], i) for i, curve in enumerate(curves))
        if choices[0][0] > tolerance:
            raise ValueError("OCCT intersection does not match the source branch")
        assignments.append(choices[0][1])
    pieces = []
    for index in dict.fromkeys(assignments):
        curve = curves[index].Copy()
        subset = [p for p, which in zip(samples, assignments, strict=True) if which == index]
        if len(subset) > 1 and not _follows_source_order(curve, subset, tolerance):
            curve.Reverse()
        pieces.append(curve)
    if len(pieces) == 1:
        result = pieces[0]
    else:
        joined = GeomConvert_CompCurveToBSplineCurve(GeomConvert.CurveToBSplineCurve_s(pieces[0]))
        for piece in pieces[1:]:
            if not joined.Add(piece, tolerance, True, True, 100):
                raise ValueError("disconnected OCCT intersection pieces")
        result = joined.BSplineCurve()
    if _closed(result, tolerance) and not result.IsPeriodic():
        # A closed branch must admit the arc across its parameter origin.
        # Joining the ends moves the junction by at most the closure gap,
        # which the source boundary validation still measures.
        result = GeomConvert.CurveToBSplineCurve_s(result)
        result.SetPeriodic()
    # The whole branch follows the source order: a piece judged on its own
    # subset of points can run against the arc that crosses into its neighbour.
    if not _follows_source_order(result, samples, tolerance):
        result.Reverse()
    return result
