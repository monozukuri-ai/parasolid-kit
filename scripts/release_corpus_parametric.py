"""Native coefficients and sampled source/STEP geometry for declared Onshape cases.

This is a release oracle, not an OCCT conversion API or a continuous error proof.
The strict distance result is retained separately from producer tolerances.
"""

from __future__ import annotations

import dataclasses
import json
import math
import re
from types import SimpleNamespace

from release_geometry import (
    curve_domain,
    curve_value,
    edge_distance,
    expand,
    occt_surface,
    surface_distance,
    surface_domain,
    surface_value,
    xyz,
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unwrap(value):
    if not isinstance(value, dict):
        return value
    kind, contents = value.get("btType", ""), value.get("value")
    if kind.endswith("BTFSValueMap"):
        return {unwrap(e["key"]): unwrap(e["value"]) for e in contents}
    if kind.endswith("BTFSValueArray"):
        return [unwrap(e) for e in contents]
    return contents


def close_values(left, right, tolerance):
    if isinstance(left, (list, tuple)):
        return (
            isinstance(right, (list, tuple))
            and len(left) == len(right)
            and all(close_values(a, b, tolerance) for a, b in zip(left, right, strict=True))
        )
    return (
        isinstance(right, (int, float)) and math.isfinite(right) and abs(left - right) <= tolerance
    )


def coefficients_match(source, native, tolerance, knot_tolerance):
    surface = hasattr(source, "u_degree")
    if source.rational != native["rational"]:
        return False
    points = native["points"]
    if surface:
        for axis in ("u", "v"):
            if (getattr(source, axis + "_degree"), getattr(source, axis + "_periodic")) != (
                native[axis + "Degree"],
                native[axis + "Periodic"],
            ):
                return False
            if not close_values(
                expand(
                    getattr(source, axis + "_knots"), getattr(source, axis + "_knot_multiplicities")
                ),
                native[axis + "Knots"],
                knot_tolerance,
            ):
                return False
        if len(points) != source.u_control_vertex_count or any(
            len(row) != source.v_control_vertex_count for row in points
        ):
            return False
        weights = [w for row in native["weights"] for w in row] if source.rational else None
        points = [point for row in points for point in row]
    else:
        if (source.degree, source.periodic) != (native["degree"], native["periodic"]):
            return False
        if not close_values(
            expand(source.knots, source.knot_multiplicities), native["knots"], knot_tolerance
        ):
            return False
        weights = native.get("weights") if source.rational else None
    if weights is not None:
        points = [[*(p * w for p in point), w] for point, w in zip(points, weights, strict=True)]
    return close_values(source.control_vertices, points, tolerance)


def parameters(start, end, knots=()):
    return sorted(
        {start + (end - start) * i / 32 for i in range(33)}
        | {k for k in knots if start <= k <= end}
    )


def verify(root, path, specification):
    import release_corpus_oracle as analytic

    from parasolid_kit import read_brep
    from parasolid_kit.brep import (
        CircleCurve,
        EllipseCurve,
        IntersectionCurve,
        LineCurve,
        NurbsCurve,
        NurbsSurface,
        SurfaceParametricCurve,
        TrimmedCurve,
    )

    require(specification["unit"] == "m", "parametric oracle requires explicit metre units")
    for name, value in specification.items():
        if name.endswith("_tolerance"):
            require(math.isfinite(value) and value > 0, "invalid oracle tolerance")
    tolerance = specification["linear_tolerance"]
    coefficient_tolerance = specification["coefficient_tolerance"]
    knot_tolerance = specification["knot_tolerance"]
    analytic.POSITION_TOL = tolerance
    analytic.DIRECTION_TOL = specification["direction_tolerance"]
    result = read_brep(path)
    model = result.brep
    require(model.complete and model.topology.valid, "incomplete source topology")

    def read(key):
        return json.loads((root / specification[key]["path"]).read_text())

    details, mass, native_response = (
        read("body_details"),
        read("mass_properties"),
        read("native_geometry"),
    )
    version = specification["microversion"]
    require(
        details["microversionId"]["theId"]
        == mass["microversionId"]
        == native_response["sourceMicroversion"]
        == version,
        "native geometry saved states differ",
    )
    require(not native_response["notices"], "native geometry query has notices")
    native = unwrap(native_response["result"])
    require(isinstance(native, dict), "missing native geometry")
    for name in ("bodies", "faces", "edges", "vertices"):
        expected = (
            len(details["bodies"])
            if name == "bodies"
            else sum(len(b.get(name, [])) for b in details["bodies"])
        )
        require(expected == len(getattr(model, name)), "native topology differs: " + name)
    step_path = root / specification["step"]["path"]
    text = step_path.read_text()
    require("SI_UNIT($,.METRE.)" in text, "STEP metre declaration missing")
    uncertainty = {
        float(v)
        for v in re.findall(r"UNCERTAINTY_MEASURE_WITH_UNIT\(LENGTH_MEASURE\(([^)]+)\)", text)
    }
    require(len(uncertainty) == 1, "ambiguous STEP uncertainty")
    declared = uncertainty.pop()
    require(
        0 < declared <= specification["max_step_tolerance"], "STEP uncertainty exceeds frozen limit"
    )
    step = analytic.step_geometry(step_path, include_parametric=True)
    require(step["faces"] == len(model.faces), "STEP face count differs")
    require(
        step["bodies"] == sum(b["type"] == "SOLID" for b in details["bodies"]),
        "STEP solid count differs",
    )

    primitive_kinds = {"line", "circle", "ellipse", "plane", "cylinder", "sphere", "cone", "torus"}
    primitives = analytic.source_geometry(
        SimpleNamespace(
            curves=[c for c in model.curves if c.kind in primitive_kinds],
            surfaces=[s for s in model.surfaces if s.kind in primitive_kinds],
        )
    )
    producer = [f["surface"] for b in details["bodies"] for f in b["faces"]]
    producer += [e["curve"] for b in details["bodies"] for e in b.get("edges", [])]
    for primitive in primitives:
        require(
            any(analytic.matches(primitive, p) for p in producer),
            "native analytic geometry mismatch",
        )
        require(
            any(analytic.matches(primitive, p) for p in step["geometry"]),
            "STEP analytic geometry mismatch",
        )
    vertices = [analytic.vec(v["point"]) for b in details["bodies"] for v in b.get("vertices", [])]
    for point in model.points:
        for reference in (vertices, step["points"]):
            require(
                any(math.dist(point.position, p) <= tolerance for p in reference),
                "vertex position mismatch",
            )

    old_surface_format = "uDegree" in native
    native_surfaces = (
        [native]
        if old_surface_format
        else [s["spline"] for s in native["surfaces"] if s.get("spline")]
    )
    native_curves = (
        native["curves"]
        if old_surface_format
        else [c["spline"] for c in native["curves"] if c.get("spline")]
    )
    surfaces = {s.id: s.definition for s in model.surfaces}
    built_surfaces = {i: occt_surface(s) for i, s in surfaces.items()}
    curves = {c.id: c.definition for c in model.curves}
    source_tolerance = max(
        [tolerance] + [e.tolerance for e in model.edges if e.tolerance is not None]
    )
    require(
        source_tolerance <= specification["max_source_tolerance"],
        "source edge tolerance exceeds frozen limit",
    )
    checks = []

    def boundary_check(points, label):
        require(points, label + ": no samples")
        residual = max(min(edge_distance(p, edge) for edge in step["edges"]) for p in points)
        require(residual <= declared, label + ": samples exceed STEP declared uncertainty")
        wrong = [p + 0.017 * (i + 1) for i, p in enumerate(points[len(points) // 2])]
        require(
            min(edge_distance(wrong, edge) for edge in step["edges"]) > declared * 10,
            label + ": negative control was accepted",
        )
        return dict(
            samples=len(points),
            step_max_distance_m=residual,
            strict_distance_passed=residual <= tolerance,
        )

    def curve_point(identifier, parameter):
        curve = curves[identifier]
        if isinstance(curve, TrimmedCurve):
            return curve_point(curve.basis_curve, parameter)
        if isinstance(curve, NurbsCurve):
            return curve_value(curve, parameter)
        if isinstance(curve, SurfaceParametricCurve):
            uv = curve_value(curves[curve.parameter_curve], parameter)
            return xyz(built_surfaces[curve.surface].Value(*uv))
        if isinstance(curve, LineCurve):
            return [p + parameter * d for p, d in zip(curve.point, curve.direction, strict=True)]
        if isinstance(curve, (CircleCurve, EllipseCurve)):
            x, normal = list(curve.x_axis), list(curve.normal)
            y = [
                normal[1] * x[2] - normal[2] * x[1],
                normal[2] * x[0] - normal[0] * x[2],
                normal[0] * x[1] - normal[1] * x[0],
            ]
            a, b = (
                (curve.radius, curve.radius)
                if isinstance(curve, CircleCurve)
                else (curve.major_radius, curve.minor_radius)
            )
            return [
                p + a * math.cos(parameter) * xx + b * math.sin(parameter) * yy
                for p, xx, yy in zip(curve.center, x, y, strict=True)
            ]
        raise ValueError("curve has no direct parameter evaluator in this oracle")

    for identifier, surface in surfaces.items():
        if not isinstance(surface, NurbsSurface):
            continue
        face_surface = any(face.surface == identifier for face in model.faces)
        native_match = any(
            coefficients_match(surface, s, coefficient_tolerance, knot_tolerance)
            for s in native_surfaces
        )
        if face_surface:
            require(native_match, "native NURBS face surface coefficients differ")
        else:
            require(
                any(
                    isinstance(c, IntersectionCurve) and identifier in c.surfaces
                    for c in curves.values()
                ),
                "auxiliary surface has no verified intersection role",
            )
        (ua, ub), (va, vb) = surface_domain(surface)
        uv = [
            (u, v)
            for u in sorted(
                set(parameters(ua, ub)[::4]) | {k for k in surface.u_knots if ua <= k <= ub}
            )
            for v in sorted(
                set(parameters(va, vb)[::4]) | {k for k in surface.v_knots if va <= k <= vb}
            )
        ]
        points = [surface_value(surface, u, v) for u, v in uv]
        numerical = max(
            math.dist(p, xyz(built_surfaces[identifier].Value(u, v)))
            for (u, v), p in zip(uv, points, strict=True)
        )
        require(numerical <= coefficient_tolerance, "de Boor and OCCT surface evaluation differ")
        # Auxiliary surfaces used to define intersections are not face surfaces
        # in either native Body Details or STEP. Their intersection points are
        # checked against both bases and the independent STEP boundary below.
        residual = None
        if face_surface:
            index = min(
                range(len(step["surfaces"])),
                key=lambda i: max(surface_distance(p, step["surfaces"][i]) for p in points[::8]),
            )
            residual = max(surface_distance(p, step["surfaces"][index]) for p in points)
            if specification["step_surface_checks"] == "required":
                require(residual <= declared, "NURBS surface differs from STEP")
        wrong = dataclasses.replace(
            surface, control_vertices=tuple(reversed(surface.control_vertices))
        )
        require(
            math.dist(
                surface_value(surface, ua + (ub - ua) * 0.173, va + (vb - va) * 0.619),
                surface_value(wrong, ua + (ub - ua) * 0.173, va + (vb - va) * 0.619),
            )
            > tolerance,
            "surface control-order negative control failed",
        )
        shifts = []
        for axis, periodic in enumerate((surface.u_periodic, surface.v_periodic)):
            if periodic:
                a, b = surface_domain(surface)[axis]
                original = [ua + (ub - ua) * 0.173, va + (vb - va) * 0.619]
                for k in (-3, -1, 1, 4):
                    shifted = original.copy()
                    shifted[axis] += k * (b - a)
                    shifts.append(
                        math.dist(
                            surface_value(surface, *original), surface_value(surface, *shifted)
                        )
                    )
        require(
            all(d <= coefficient_tolerance for d in shifts), "periodic surface translation failed"
        )
        from OCP.gp import gp_Pnt, gp_Vec

        seam_position, seam_derivative = 0.0, 0.0
        for axis, periodic in enumerate((surface.u_periodic, surface.v_periodic)):
            if not periodic:
                continue
            for fraction in (0, 0.173, 0.5, 0.619, 1):
                ends = []
                for end in surface_domain(surface)[axis]:
                    u, v = (
                        (end, va + (vb - va) * fraction)
                        if axis == 0
                        else (ua + (ub - ua) * fraction, end)
                    )
                    p, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
                    built_surfaces[identifier].D1(u, v, p, du, dv)
                    ends.append((xyz(p), xyz(du), xyz(dv)))
                seam_position = max(seam_position, math.dist(ends[0][0], ends[1][0]))
                seam_derivative = max(
                    seam_derivative, *(math.dist(ends[0][i], ends[1][i]) for i in (1, 2))
                )
        require(
            seam_position <= coefficient_tolerance
            and seam_derivative <= specification["direction_tolerance"],
            "periodic surface seam position/derivatives differ",
        )
        native_point_error, native_normal_error = 0.0, 0.0
        if old_surface_format:
            for sample in native["samples"]:
                p, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
                built_surfaces[identifier].D1(sample["u"], sample["v"], p, du, dv)
                normal = xyz(du.Crossed(dv).Normalized())
                native_point_error = max(
                    native_point_error,
                    math.dist(surface_value(surface, sample["u"], sample["v"]), sample["point"]),
                )
                native_normal_error = max(
                    native_normal_error,
                    min(
                        math.dist(normal, sample["normal"]),
                        math.dist(normal, [-n for n in sample["normal"]]),
                    ),
                )
            require(
                native_point_error <= coefficient_tolerance
                and native_normal_error <= specification["direction_tolerance"],
                "native surface sample position/normal differs",
            )
        if surface.rational:
            wrong = dataclasses.replace(
                surface,
                rational=False,
                vertex_dimension=3,
                control_vertices=tuple(tuple(p[:3]) for p in surface.control_vertices),
            )
            require(
                math.dist(
                    surface_value(surface, ua + (ub - ua) * 0.371, va + (vb - va) * 0.619),
                    surface_value(wrong, ua + (ub - ua) * 0.371, va + (vb - va) * 0.619),
                )
                > tolerance,
                "homogeneous division negative control failed",
            )
        checks.append(
            dict(
                kind="nurbs_surface",
                id=identifier,
                samples=len(points),
                native_coefficients=native_match,
                role="face" if face_surface else "intersection_support",
                numerical_max_m=numerical,
                step_max_distance_m=residual,
                step_declared_accuracy_passed=None if residual is None else residual <= declared,
                strict_distance_passed=None if residual is None else residual <= tolerance,
                periodic_shift_max_m=max(shifts, default=0.0),
                seam_position_max_m=seam_position,
                seam_derivative_max=seam_derivative,
                native_sample_count=len(native["samples"]) if old_surface_format else 0,
                native_point_max_m=native_point_error,
                native_normal_max=native_normal_error,
            )
        )

    uv_ids = {c.parameter_curve for c in curves.values() if isinstance(c, SurfaceParametricCurve)}
    for identifier, curve in curves.items():
        if isinstance(curve, NurbsCurve) and identifier not in uv_ids:
            require(
                any(
                    coefficients_match(curve, c, coefficient_tolerance, knot_tolerance)
                    for c in native_curves
                ),
                "native B-curve coefficients differ",
            )
            samples = [curve_value(curve, t) for t in parameters(*curve_domain(curve), curve.knots)]
            checks.append(
                dict(
                    kind="nurbs_curve",
                    id=identifier,
                    native_coefficients=True,
                    **boundary_check(samples, "B-curve"),
                )
            )
        elif isinstance(curve, SurfaceParametricCurve):
            basis = curves[curve.parameter_curve]
            require(isinstance(basis, NurbsCurve), "SP_CURVE basis is not a B-curve")
            samples = [
                curve_point(identifier, t) for t in parameters(*curve_domain(basis), basis.knots)
            ]
            checks.append(
                dict(
                    kind="surface_parametric", id=identifier, **boundary_check(samples, "SP_CURVE")
                )
            )
        elif isinstance(curve, IntersectionCurve):
            points = [
                list(p) for p in (*curve.chart_points, *curve.start_points, *curve.end_points)
            ]
            require(len(points) >= 3, "intersection chart/limits are empty")
            residual = max(
                surface_distance(p, built_surfaces[s]) for p in points for s in curve.surfaces
            )
            require(residual <= source_tolerance, "intersection samples leave supporting surfaces")
            checks.append(
                dict(
                    kind="intersection",
                    id=identifier,
                    supporting_surfaces_max_m=residual,
                    source_strict_distance_passed=residual <= tolerance,
                    **boundary_check(points, "intersection"),
                )
            )
        elif isinstance(curve, TrimmedCurve):
            basis = curves[curve.basis_curve]
            points = [list(curve.start_point), list(curve.end_point)]
            if isinstance(basis, IntersectionCurve):
                residual = max(
                    surface_distance(p, built_surfaces[s]) for p in points for s in basis.surfaces
                )
            else:
                residual = max(
                    math.dist(curve_point(curve.basis_curve, t), p)
                    for t, p in zip(
                        (curve.start_parameter, curve.end_parameter), points, strict=True
                    )
                )
            require(residual <= source_tolerance, "trim endpoints exceed source tolerance")
            checks.append(
                dict(
                    kind="trimmed",
                    id=identifier,
                    endpoint_max_m=residual,
                    source_tolerance_m=source_tolerance,
                    source_strict_distance_passed=residual <= tolerance,
                    **boundary_check(points, "trimmed"),
                )
            )
        elif not isinstance(curve, (LineCurve, CircleCurve, EllipseCurve, NurbsCurve)):
            raise ValueError("unhandled source curve in parametric oracle")
    return dict(
        status="passed",
        kind="onshape_parametric",
        unit="m",
        primitive_count=len(primitives),
        checks=checks,
        strict_distance_passed=all(
            c["strict_distance_passed"] is not False
            and c.get("source_strict_distance_passed", True)
            for c in checks
        ),
        step_declared_uncertainty_m=declared,
        source_tolerance_m=source_tolerance,
        step_surface_checks=specification["step_surface_checks"],
        step_surface_exception=specification.get("step_surface_exception"),
        criteria=(
            "native topology/primitives/face-NURBS coefficients, independent spline evaluation, "
            "intersection-support consistency, sampled boundaries within declared tolerances"
        ),
        metrics="not_certified",
        geometry_scope="sampled definitions; no continuous error or general conversion claim",
    )
