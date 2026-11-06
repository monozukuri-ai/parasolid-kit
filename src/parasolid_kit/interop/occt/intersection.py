"""Bounded numerical reconstruction of a source-identified intersection branch.

CHART points identify a branch; they are not themselves an exact 3D curve.
The fit is constrained by both support surfaces and by source trim endpoints.
Closed branches become periodic curves; unresolved/singular refinement is rejected.
"""

from __future__ import annotations

from itertools import pairwise
from math import isfinite, sqrt


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


def _interpolation_parameters(curve: object, points: list, closed: bool, numerical: float) -> list:
    """Parameters of the interpolated points on their interpolating curve.

    ``GeomAPI_Interpolate`` places one knot at every point (a closed curve
    repeats the first knot one period later); evaluating there is far cheaper
    than projecting each point back onto the curve. The knots are verified
    against the points and projection remains the fallback.
    """
    knots = [curve.Knot(i) for i in range(1, curve.NbKnots() + 1)]
    if len(knots) == len(points) + (1 if closed else 0) and all(
        curve.Value(knot).Distance(point) <= numerical
        for knot, point in zip(knots, points, strict=False)
    ):
        return knots[: len(points)]
    return [project_curve(curve, point)[0] for point in points]


def _dot(a: tuple, b: tuple) -> float:
    return sum(x * y for x, y in zip(a, b, strict=True))


def _cross(a: tuple, b: tuple) -> tuple:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _solve3(matrix: list, rhs: list) -> tuple | None:
    """Cramer's rule for a 3x3 system; None when the matrix is singular."""
    columns = [[matrix[i][j] for i in range(3)] for j in range(3)]
    determinant = _dot(columns[0], _cross(columns[1], columns[2]))
    if abs(determinant) < 1e-300:
        return None
    return tuple(
        _dot(
            columns[0] if j else rhs,
            _cross(rhs if j == 1 else columns[1], rhs if j == 2 else columns[2]),
        )
        / determinant
        for j in range(3)
    )


class SupportSurface:
    """A support surface evaluated the way the source evaluated it.

    Inside its parameter domain the OCCT surface is exact. Beyond a finite,
    non-periodic bound a Parasolid B-surface continues linearly: each direction
    is extended by its boundary tangent, so past ``u2`` the point is
    ``S(u2, v) + (u - u2) Su(u2, v)``, and past a corner the two extensions
    multiply, ``S + du Su + dv Sv + du dv Suv``. An offset surface follows the
    normal of that extension. OCCT evaluates the polynomial beyond its knots
    instead, which leaves the CHART points of a blend spine 0.06 to 22 mm from
    its support; the linear extension holds them within 1e-12 mm on 237 of 238
    out-of-domain chart points of four production bodies.

    A projection first runs a Gauss-Newton search from the foot of the previous
    point (the points of one branch arrive in order, so that seed is close) and
    falls back to OCCT's extrema search over the polynomial domain extended by
    one span only when that foot is not within the tolerance.
    """

    def __init__(
        self, surface: object, numerical: float, periodic: tuple[bool, bool] = (False, False)
    ) -> None:
        from OCP.Geom import (
            Geom_BSplineCurve,
            Geom_BSplineSurface,
            Geom_OffsetSurface,
            Geom_SurfaceOfLinearExtrusion,
            Geom_TrimmedCurve,
        )

        self.surface = surface
        self.numerical = numerical
        self.basis = surface
        self.offset = 0.0
        if isinstance(surface, Geom_OffsetSurface):
            self.basis = surface.BasisSurface()
            self.offset = surface.Offset()
        u1, u2, v1, v2 = self.basis.Bounds()
        # Only a B-surface, or the B-curve section of a swept surface, is
        # continued linearly. An analytic surface is infinite or closes on
        # itself (a sphere's v stops at the poles) and a spun one is evaluated
        # as OCCT defines it. A periodic XT B-surface arrives as a non-periodic
        # OCCT surface whose knots already wrap past the period (p. 38); the
        # source flag says the direction closes on itself, so it is never
        # extended either.
        self.section = None
        if isinstance(self.basis, Geom_SurfaceOfLinearExtrusion):
            section = self.basis.BasisCurve()
            if isinstance(section, Geom_TrimmedCurve):
                section = section.BasisCurve()
            if isinstance(section, Geom_BSplineCurve) and not section.IsPeriodic():
                self.section = section
        fixed = not isinstance(self.basis, Geom_BSplineSurface) and self.section is None
        self.u_range = self._range(u1, u2, fixed or self.basis.IsUPeriodic() or periodic[0])
        self.v_range = self._range(
            v1, v2, fixed or self.section is not None or self.basis.IsVPeriodic() or periodic[1]
        )
        self.u_scale = u2 - u1 if isfinite(u1) and isfinite(u2) else 1.0
        self.v_scale = v2 - v1 if isfinite(v1) and isfinite(v2) else 1.0
        self.bounds = (u1, u2, v1, v2)
        # A source-periodic B-surface that OCCT holds non-periodic wraps its
        # parameter by the period of the B-spline, knot[n] - knot[p] of the
        # full knot sequence (n poles, degree p); past the knots OCCT would
        # continue the polynomial where the source repeats the surface.
        self.u_period = None
        self.v_period = None
        if (
            isinstance(self.basis, Geom_BSplineSurface)
            and periodic[0]
            and not self.basis.IsUPeriodic()
        ):
            self.u_period = self._period(
                self.basis.UKnotSequence(), self.basis.NbUPoles(), self.basis.UDegree()
            )
        if (
            isinstance(self.basis, Geom_BSplineSurface)
            and periodic[1]
            and not self.basis.IsVPeriodic()
        ):
            self.v_period = self._period(
                self.basis.VKnotSequence(), self.basis.NbVPoles(), self.basis.VDegree()
            )
        # The extrema search samples the surface when it is built; a support
        # that is only evaluated never pays for it.
        self.projector: object | None = None
        self.analysis: object | None = None
        self.seed: tuple[float, float] | None = None
        # The projections evaluate the surface hundreds of thousands of times
        # on a blend-heavy part: OCCT writes the derivatives into these
        # scratch objects, which are read back as coordinates at once.
        from OCP.gp import gp_Pnt, gp_Vec

        self._point_type = gp_Pnt
        self._point = gp_Pnt()
        self._base = gp_Pnt()
        self._vectors = tuple(gp_Vec() for _ in range(5))
        self._direction = (
            tuple(self.basis.Direction().Coord()) if self.section is not None else None
        )

    @staticmethod
    def _range(lower: float, upper: float, periodic: bool) -> tuple[float, float] | None:
        if periodic or not (isfinite(lower) and isfinite(upper)):
            return None
        return (lower, upper)

    @staticmethod
    def _period(sequence: object, poles: int, degree: int) -> tuple[float, float] | None:
        """The B-spline period and the start of its periodic domain, or None."""
        if sequence.Upper() < poles + 1 or sequence.Lower() > degree + 1:
            return None
        start, end = sequence.Value(degree + 1), sequence.Value(poles + 1)
        return (start, end - start) if end > start else None

    def _wrapped(self, u: float, v: float) -> tuple[float, float]:
        if self.u_period is not None:
            start, period = self.u_period
            u = start + (u - start) % period
        if self.v_period is not None:
            start, period = self.v_period
            v = start + (v - start) % period
        return u, v

    def within_bounds(self, u: float, v: float) -> bool:
        """Whether OCCT's own surface evaluates ``(u, v)`` as the source does.

        A periodic direction wraps any parameter; a bounded one evaluates the
        source only within its knots.
        """
        u1, u2, v1, v2 = self.bounds
        return (self.surface.IsUPeriodic() or u1 <= u <= u2) and (
            self.surface.IsVPeriodic() or v1 <= v <= v2
        )

    def inside(self, u: float, v: float) -> bool:
        u, v = self._wrapped(u, v)
        return (self.u_range is None or self.u_range[0] <= u <= self.u_range[1]) and (
            self.v_range is None or self.v_range[0] <= v <= self.v_range[1]
        )

    def value(self, u: float, v: float) -> object:
        """The point at ``(u, v)``; on the linear extension beyond the domain."""
        u, v = self._wrapped(u, v)
        if self.inside(u, v):
            return self.surface.Value(u, v)
        return self._point_type(*self._extended(u, v)[0])

    def _extended(self, u: float, v: float) -> tuple:
        """Point, unit normal and both derivatives on the extension, as coordinates."""
        uc = min(max(u, self.u_range[0]), self.u_range[1]) if self.u_range else u
        vc = min(max(v, self.v_range[0]), self.v_range[1]) if self.v_range else v
        du, dv = u - uc, v - vc
        if self.section is not None:
            # A swept surface S(u, v) = C(u) + v D: past the section's knots
            # the section continues along its end tangent.
            point, tangent = self._point, self._vectors[0]
            self.section.D1(uc, point, tangent)
            (px, py, pz), eu, ev = point.Coord(), tangent.Coord(), self._direction
            x = px + eu[0] * du + ev[0] * v
            y = py + eu[1] * du + ev[1] * v
            z = pz + eu[2] * du + ev[2] * v
        else:
            point = self._point
            su, sv, suu, svv, suv = self._vectors
            self.basis.D2(uc, vc, point, su, sv, suu, svv, suv)
            (px, py, pz), (ux, uy, uz) = point.Coord(), su.Coord()
            (vx, vy, vz), (wx, wy, wz) = sv.Coord(), suv.Coord()
            eu = (ux + wx * dv, uy + wy * dv, uz + wz * dv)
            ev = (vx + wx * du, vy + wy * du, vz + wz * du)
            mixed = du * dv
            x = px + ux * du + vx * dv + wx * mixed
            y = py + uy * du + vy * dv + wy * mixed
            z = pz + uz * du + vz * dv + wz * mixed
        nx = eu[1] * ev[2] - eu[2] * ev[1]
        ny = eu[2] * ev[0] - eu[0] * ev[2]
        nz = eu[0] * ev[1] - eu[1] * ev[0]
        magnitude = sqrt(nx * nx + ny * ny + nz * nz)
        if magnitude <= 1e-300:
            raise ValueError("intersection support surface is singular at a projected point")
        nx, ny, nz = nx / magnitude, ny / magnitude, nz / magnitude
        offset = self.offset
        return (x + nx * offset, y + ny * offset, z + nz * offset), (nx, ny, nz), eu, ev

    def _d1(self, u: float, v: float) -> tuple:
        """Point, both derivatives and the (unnormalized) normal, as coordinates.

        Exact inside the domain and on the extension itself; forward
        differences for the offset of an extension, whose normal varies with
        the mixed derivative.
        """
        u, v = self._wrapped(u, v)
        if (self.u_range is None or self.u_range[0] <= u <= self.u_range[1]) and (
            self.v_range is None or self.v_range[0] <= v <= self.v_range[1]
        ):
            point, (du, dv) = self._point, self._vectors[:2]
            self.surface.D1(u, v, point, du, dv)
            p, a, b = point.Coord(), du.Coord(), dv.Coord()
            if self.offset != 0.0:
                # An offset shares the normal of its basis, which stays
                # defined where the offset's own derivatives vanish (its focal
                # set: the basis curves there with the radius of the offset).
                su, sv = self._vectors[2:4]
                self.basis.D1(u, v, self._base, su, sv)
                return p, a, b, _cross(su.Coord(), sv.Coord())
            return p, a, b, _cross(a, b)
        point, normal, eu, ev = self._extended(u, v)
        if self.offset == 0.0:
            return point, eu, ev, normal
        hu, hv = 1e-6 * self.u_scale, 1e-6 * self.v_scale
        pu = self._extended(u + hu, v)[0]
        pv = self._extended(u, v + hv)[0]
        return (
            point,
            ((pu[0] - point[0]) / hu, (pu[1] - point[1]) / hu, (pu[2] - point[2]) / hu),
            ((pv[0] - point[0]) / hv, (pv[1] - point[1]) / hv, (pv[2] - point[2]) / hv),
            normal,
        )

    def _evaluate(self, point: object, u: float, v: float) -> tuple:
        (fx, fy, fz), _, _, (nx, ny, nz) = self._d1(u, v)
        magnitude = sqrt(nx * nx + ny * ny + nz * nz)
        if magnitude <= 1e-300:
            raise ValueError("intersection support surface is singular at a projected point")
        qx, qy, qz = point.Coord()
        distance = sqrt((qx - fx) ** 2 + (qy - fy) ** 2 + (qz - fz) ** 2)
        normal = (nx / magnitude, ny / magnitude, nz / magnitude)
        return self._point_type(fx, fy, fz), normal, distance, (u, v)

    def _newton(self, point: object, seed: tuple[float, float]) -> tuple | None:
        """The foot found by Gauss-Newton from ``seed``; the last item says it converged."""
        qx, qy, qz = point.Coord()
        u, v = seed
        converged = False
        u_stop, v_stop = 1e-10 * self.u_scale, 1e-10 * self.v_scale
        for _ in range(30):
            (px, py, pz), (ax, ay, az), (bx, by, bz), _ = self._d1(u, v)
            rx, ry, rz = qx - px, qy - py, qz - pz
            a11 = ax * ax + ay * ay + az * az
            a12 = ax * bx + ay * by + az * bz
            a22 = bx * bx + by * by + bz * bz
            determinant = a11 * a22 - a12 * a12
            if determinant <= 1e-300 or determinant <= 1e-24 * a11 * a22:
                return None
            b1 = ax * rx + ay * ry + az * rz
            b2 = bx * rx + by * ry + bz * rz
            step_u = (b1 * a22 - b2 * a12) / determinant
            step_v = (a11 * b2 - a12 * b1) / determinant
            u, v = u + step_u, v + step_v
            if not (isfinite(u) and isfinite(v)):
                return None
            if abs(step_u) <= u_stop and abs(step_v) <= v_stop:
                converged = True
                break
        return (*self._evaluate(point, u, v), converged)

    def project(self, point: object, tolerance: float) -> tuple:
        """The foot of ``point``, its unit normal and the distance."""
        if self.seed is not None:
            # The points of one branch arrive in order, so the foot found from
            # the previous one is the nearest point while it converged and the
            # point is within a hundred thousand tolerances of the surface (a
            # chord midpoint of a coarse chart being refined, a millimetre on
            # production parts); a point farther away may sit nearer to
            # another sheet and gets the extrema search.
            local = self._newton(point, self.seed)
            if local is not None and local[4] and local[2] <= 1e5 * tolerance:
                self.seed = local[3]
                return local[:3]
        if self.projector is None:
            from OCP.GeomAPI import GeomAPI_ProjectPointOnSurf
            from OCP.ShapeAnalysis import ShapeAnalysis_Surface

            u1, u2, v1, v2 = self.bounds
            if self.u_range is not None:
                u1, u2 = u1 - (u2 - u1), u2 + (u2 - u1)
            if self.v_range is not None:
                v1, v2 = v1 - (v2 - v1), v2 + (v2 - v1)
            self.projector = GeomAPI_ProjectPointOnSurf()
            self.projector.Init(self.surface, u1, u2, v1, v2, self.numerical)
            self.analysis = ShapeAnalysis_Surface(self.surface)
        self.projector.Perform(point)
        if self.projector.NbPoints():
            u, v = self.projector.LowerDistanceParameters()
        else:
            # The extrema search returns no candidate on some surfaces (a pipe
            # B-spline near its seam, a point close to a singularity); the
            # shape-analysis projection still finds the nearest parameters.
            uv = self.analysis.ValueOfUV(point, self.numerical)
            u, v = uv.X(), uv.Y()
            if not (isfinite(u) and isfinite(v)):
                raise ValueError("intersection point/surface projection failed")
        best = self._evaluate(point, u, v)
        if best[2] > tolerance or not self.inside(u, v):
            # The extrema search minimized the distance to the polynomial
            # extension; the foot on the linear extension is refined from it.
            refined = self._newton(point, (u, v))
            if refined is not None and refined[2] < best[2]:
                best = refined
        self.seed = best[3]
        return best[:3]


def refine_intersection_point(
    point: object,
    tangent: tuple,
    supports: list,
    tolerance: float,
    slack: tuple[float, float] = (0.0, 0.0),
) -> object:
    """Move ``point`` onto both supports within the plane through it normal to ``tangent``.

    ``slack`` widens the acceptance per support: a support that is itself an
    approximation (a blend pipe around a fitted spine) is met when the point
    is within the tolerance plus that support's own uncertainty.
    """
    from OCP.gp import gp_Pnt

    anchor = point.Coord()

    def residual(candidate: object) -> tuple:
        q1, n1, d1 = supports[0].project(candidate, tolerance)
        q2, n2, d2 = supports[1].project(candidate, tolerance)
        xyz = candidate.Coord()
        rhs = (
            _dot(tuple(x - y for x, y in zip(xyz, q1.Coord(), strict=True)), n1),
            _dot(tuple(x - y for x, y in zip(xyz, q2.Coord(), strict=True)), n2),
            _dot(tuple(x - y for x, y in zip(xyz, anchor, strict=True)), tangent),
        )
        return rhs, (n1, n2), max(d1 - slack[0], d2 - slack[1])

    rhs, normals, worst = residual(point)
    for _ in range(60):
        if worst <= tolerance:
            return point
        rows = (*normals, tangent)
        # A Gauss-Newton step with Levenberg-Marquardt damping. The system
        # [n1; n2; t] is nearly singular where two offsets meet at a shallow
        # angle (blends of faces that are almost tangent); its exact solution
        # slides far along both surfaces, where their curvature invalidates
        # the linearization and the iteration oscillates. The damping leaves
        # the well-conditioned directions (the normals, the tangent) alone
        # and bounds the slide; the step is then halved until the residual
        # norm drops.
        matrix = [
            [sum(row[i] * row[j] for row in rows) + (1e-6 if i == j else 0.0) for j in range(3)]
            for i in range(3)
        ]
        gradient = [-sum(rhs[k] * rows[k][i] for k in range(3)) for i in range(3)]
        step = _solve3(matrix, gradient)
        if step is None:
            raise ValueError("singular intersection refinement")
        merit = _dot(rhs, rhs)
        xyz = point.Coord()
        scale = 1.0
        for _ in range(24):
            candidate = gp_Pnt(*(x + scale * s for x, s in zip(xyz, step, strict=True)))
            trial = residual(candidate)
            if _dot(trial[0], trial[0]) < merit:
                point, (rhs, normals, worst) = candidate, trial
                break
            scale *= 0.5
        else:
            break
    # The Newton residual measures the distance along the normal of each foot,
    # which an offset surface near a cusp of its own (the basis curves more
    # tightly than the offset distance) reports as zero while the foot is far
    # from the point: its derivatives no longer describe it there. Alternating
    # projections use only the feet, which the extrema search still finds, and
    # converge for any transversal pair.
    previous = worst
    for _ in range(200):
        foot = supports[0].project(point, tolerance)[0]
        foot = supports[1].project(foot, tolerance)[0]
        xyz = foot.Coord()
        offset = _dot(tuple(x - a for x, a in zip(xyz, anchor, strict=True)), tangent)
        point = gp_Pnt(*(x - offset * t for x, t in zip(xyz, tangent, strict=True)))
        worst = max(
            support.project(point, tolerance)[2] - allowance
            for support, allowance in zip(supports, slack, strict=True)
        )
        if worst <= tolerance:
            return point
        if worst >= previous * 0.999:
            break
        previous = worst
    raise ValueError("intersection refinement did not converge")


def march_intersection(
    supports: list,
    start: object,
    direction: tuple,
    length: float,
    step: float,
    tolerance: float,
    slack: tuple[float, float] = (0.0, 0.0),
) -> list:
    """Continue an intersection branch from ``start`` along ``direction``.

    Each step seeds the next point one ``step`` ahead along the current
    heading and refines it onto both supports; the heading follows the
    points found. The walk stops after ``length``, or where the supports no
    longer meet. Returns the points found, in walking order.
    """
    from OCP.gp import gp_Pnt

    points: list = []
    previous = start
    heading = direction
    travelled = 0.0
    while travelled < length and len(points) < 5_000:
        seed = gp_Pnt(*(x + step * h for x, h in zip(previous.Coord(), heading, strict=True)))
        try:
            point = refine_intersection_point(seed, heading, supports, tolerance, slack)
        except ValueError:
            break
        advance = point.Distance(previous)
        if advance <= tolerance:
            break
        heading = tuple(
            (x - y) / advance for x, y in zip(point.Coord(), previous.Coord(), strict=True)
        )
        points.append(point)
        travelled += advance
        previous = point
    return points


def fit_intersection(
    surfaces: list,
    samples: list,
    anchors: list,
    tolerance: float,
    periodic: list[tuple[bool, bool]] | None = None,
    slack: list[float] | None = None,
    cap: float | None = None,
    report: dict | None = None,
) -> object:
    """Refine an open branch within the source body's linear resolution.

    Every interpolation span is checked against both surfaces at three interior
    parameters. Failed spans receive corrected chord samples. Twelve rounds and
    10,000 points bound the solver; failure never returns an unchecked fit.
    ``periodic`` carries the source's own periodicity flags per surface (see
    :class:`SupportSurface`); ``slack`` the uncertainty of each support (a
    blend pipe around a fitted spine), added to the tolerance for that support.

    With ``cap`` (target units) a point the supports do not meet within the
    tolerance is still accepted while it is within the cap: an offset of a
    basis with a crease leaves a gap the source bridged, and the source's own
    chart may sit a few micrometres off a support. The largest such distance
    is returned in ``report["deviation"]``; without a cap it fails as before.
    """
    from OCP.GeomAPI import GeomAPI_Interpolate
    from OCP.gp import gp_Pnt
    from OCP.TColgp import TColgp_HArray1OfPnt

    numerical = max(1e-14, tolerance * 1e-4)
    if len(samples) + len(anchors) > 10_000:
        raise ValueError("intersection input exceeds 10000 points")

    # One projector per surface: building the extrema search of a B-spline
    # surface samples it, which dominated the fit when every point built its own.
    flags = periodic or [(False, False)] * len(surfaces)
    supports = [
        SupportSurface(surface, numerical, flag)
        for surface, flag in zip(surfaces, flags, strict=True)
    ]
    allowances = tuple(slack or [0.0] * len(surfaces))
    deviation = 0.0

    def excess(point: object) -> float:
        # The largest distance beyond a support's own uncertainty.
        return max(
            support.project(point, tolerance)[2] - allowance
            for support, allowance in zip(supports, allowances, strict=True)
        )

    def tolerate(over: float, what: str) -> None:
        nonlocal deviation
        if cap is None or over > cap:
            raise ValueError(what)
        deviation = max(deviation, over)

    def finish(curve: object) -> object:
        if report is not None and deviation > 0.0:
            report["deviation"] = max(report.get("deviation", 0.0), deviation)
        return curve

    def distinct(items: list, *, periodic: bool) -> list:
        # The interpolation rejects points closer than its tolerance, which a
        # refined span or an inserted trim point can otherwise produce.
        kept: list = []
        for item in items:
            if not kept or item.Distance(kept[-1]) > numerical:
                kept.append(item)
        if periodic and len(kept) > 2 and kept[0].Distance(kept[-1]) <= numerical:
            kept.pop()
        return kept

    def refine(point: object, tangent: tuple) -> object:
        return refine_intersection_point(point, tangent, supports, tolerance, allowances)

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
        over = excess(point)
        if over > tolerance:
            tolerate(over, "source intersection point is outside its support-surface tolerance")
    for iteration in range(12):
        points = distinct(points, periodic=closed)
        if len(points) < 2:
            raise ValueError("intersection branch has fewer than two distinct points")
        array = TColgp_HArray1OfPnt(1, len(points))
        for index, point in enumerate(points, 1):
            array.SetValue(index, point)
        try:
            fit = GeomAPI_Interpolate(array, closed, numerical)
            fit.Perform()
        except Exception as error:  # OCCT raises Standard_ConstructionError without text
            raise ValueError("intersection interpolation rejected the branch points") from error
        if not fit.IsDone():
            raise ValueError("intersection interpolation failed")
        curve = fit.Curve()
        parameters = _interpolation_parameters(curve, points, closed, numerical)
        if iteration == 0 and anchors:
            # Source trim parameters are not OCCT curve parameters. Insert the
            # exact trim points in branch order before refining the interior;
            # a sample within the numerical tolerance of a trim point yields to it.
            ordered = list(zip(parameters, points, strict=True))
            for point in anchors:
                parameter, _ = project_curve(curve, point)
                ordered = [item for item in ordered if item[1].Distance(point) > numerical]
                ordered.append((parameter, point))
            points = [item[1] for item in sorted(ordered, key=lambda item: item[0])]
            continue
        refined = []
        worst = 0.0
        progress = False
        spans = list(pairwise(zip(points, parameters, strict=True)))
        if closed:
            spans.append(((points[-1], parameters[-1]), (points[0], parameters[0])))
        for (left, a), (right, b) in spans:
            if closed and b <= a:
                b += curve.Period()
            refined.append(left)
            for fraction in (0.25, 0.5, 0.75):
                point = curve.Value(a + (b - a) * fraction)
                over = excess(point)
                worst = max(worst, over)
                if over > tolerance:
                    chord = tuple(y - x for x, y in zip(left.Coord(), right.Coord(), strict=True))
                    magnitude = _dot(chord, chord) ** 0.5
                    if magnitude <= numerical:
                        raise ValueError("intersection refinement has a degenerate interval")
                    seed = gp_Pnt(
                        *(x + d * fraction for x, d in zip(left.Coord(), chord, strict=True))
                    )
                    try:
                        refined.append(refine(seed, tuple(x / magnitude for x in chord)))
                    except ValueError as error:
                        # The supports do not meet here within the tolerance;
                        # the interpolation stays as the bridge when the cap
                        # admits the gap.
                        tolerate(over, str(error))
                        continue
                    progress = True
        if worst <= tolerance or not progress:
            return finish(curve)
        if not closed:
            refined.append(points[-1])
        points = refined
        if len(points) > 10_000:
            raise ValueError("intersection refinement exceeds 10000 points")
    tolerate(worst, "intersection refinement exceeds 12 rounds")
    return finish(curve)


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

    def branches(scale: float) -> list:
        # Internal conditioning only; the returned curves retain the requested units.
        forward, backward = gp_Trsf(), gp_Trsf()
        forward.SetScale(gp_Pnt(), scale)
        backward.SetScale(gp_Pnt(), 1 / scale)
        operation = GeomAPI_IntSS(*(s.Transformed(forward) for s in surfaces), 1e-9)
        if not operation.IsDone() or not operation.NbLines():
            raise ValueError("OCCT surface intersection has no branch")
        return [operation.Line(i).Transformed(backward) for i in range(1, operation.NbLines() + 1)]

    # The branches of the surfaces scaled to unit extent are tried first; a
    # branch OCCT walks rather than solves (a plane a few nanoradians off the
    # equatorial plane of a torus, far from the origin) can miss the source
    # points by more than the tolerance once scaled back, where the branch of
    # the unscaled surfaces meets them.
    curves: list = []
    assignments: list = []
    for scale in (1 / extent, 1.0):
        curves = branches(scale)
        assignments = []
        for point in samples:
            choices = sorted((project_curve(curve, point)[1], i) for i, curve in enumerate(curves))
            if choices[0][0] > tolerance:
                break
            assignments.append(choices[0][1])
        if len(assignments) == len(samples):
            break
    else:
        raise ValueError("OCCT intersection does not match the source branch")
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
