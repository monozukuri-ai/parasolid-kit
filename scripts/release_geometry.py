"""Independent numerical helpers for release oracles, outside the parser runtime."""

from __future__ import annotations

import bisect
import math


def expand(knots, multiplicities):
    return [k for k, m in zip(knots, multiplicities, strict=True) for _ in range(m)]


def de_boor(coefficients, knots, degree, parameter):
    count = len(coefficients)
    if not knots[degree] <= parameter <= knots[count]:
        raise ValueError("parameter outside the transmitted spline domain")
    span = min(count - 1, bisect.bisect_right(knots, parameter) - 1)
    values = [list(row) for row in coefficients[span - degree : span + 1]]
    for level in range(1, degree + 1):
        for j in range(degree, level - 1, -1):
            i = span - degree + j
            denominator = knots[i + degree - level + 1] - knots[i]
            alpha = (parameter - knots[i]) / denominator if denominator else 0.0
            values[j] = [
                (1 - alpha) * a + alpha * b for a, b in zip(values[j - 1], values[j], strict=True)
            ]
    return values[degree]


def curve_domain(curve):
    knots = expand(curve.knots, curve.knot_multiplicities)
    return knots[curve.degree], knots[curve.control_vertex_count]


def curve_value(curve, parameter):
    knots = expand(curve.knots, curve.knot_multiplicities)
    start, end = curve_domain(curve)
    if curve.periodic and not start <= parameter <= end:
        parameter = start + (parameter - start) % (end - start)
    point = de_boor(curve.control_vertices, knots, curve.degree, parameter)
    return [p / point[-1] for p in point[:-1]] if curve.rational else point


def surface_domain(surface):
    return [
        (
            expand(getattr(surface, a + "_knots"), getattr(surface, a + "_knot_multiplicities"))[
                getattr(surface, a + "_degree")
            ],
            expand(getattr(surface, a + "_knots"), getattr(surface, a + "_knot_multiplicities"))[
                getattr(surface, a + "_control_vertex_count")
            ],
        )
        for a in ("u", "v")
    ]


def surface_value(surface, u, v):
    parameters = [u, v]
    for i, (axis, (start, end)) in enumerate(zip(("u", "v"), surface_domain(surface), strict=True)):
        if getattr(surface, axis + "_periodic") and not start <= parameters[i] <= end:
            parameters[i] = start + (parameters[i] - start) % (end - start)
    u_knots = expand(surface.u_knots, surface.u_knot_multiplicities)
    v_knots = expand(surface.v_knots, surface.v_knot_multiplicities)
    count = surface.v_control_vertex_count
    rows = [
        de_boor(surface.control_vertices[i : i + count], v_knots, surface.v_degree, parameters[1])
        for i in range(0, len(surface.control_vertices), count)
    ]
    point = de_boor(rows, u_knots, surface.u_degree, parameters[0])
    return [p / point[-1] for p in point[:-1]] if surface.rational else point


def array(values, cls):
    result = cls(1, len(values))
    for i, value in enumerate(values, 1):
        result.SetValue(i, value)
    return result


def occt_surface(surface):
    """Build the transmitted basis, independently of the public conversion adapter."""
    from OCP.Geom import (
        Geom_BSplineSurface,
        Geom_ConicalSurface,
        Geom_CylindricalSurface,
        Geom_Plane,
        Geom_SphericalSurface,
        Geom_ToroidalSurface,
    )
    from OCP.gp import gp_Ax3, gp_Dir, gp_Pnt
    from OCP.TColgp import TColgp_Array2OfPnt
    from OCP.TColStd import TColStd_Array1OfInteger, TColStd_Array1OfReal, TColStd_Array2OfReal

    from parasolid_kit.brep import (
        ConeSurface,
        CylinderSurface,
        NurbsSurface,
        PlaneSurface,
        SphereSurface,
        TorusSurface,
    )

    if isinstance(surface, NurbsSurface):
        nu, nv = surface.u_control_vertex_count, surface.v_control_vertex_count
        poles, weights = TColgp_Array2OfPnt(1, nu, 1, nv), TColStd_Array2OfReal(1, nu, 1, nv)
        for u in range(nu):
            for v in range(nv):
                point = surface.control_vertices[u * nv + v]
                weight = point[-1] if surface.rational else 1.0
                poles.SetValue(u + 1, v + 1, gp_Pnt(*(p / weight for p in point[:3])))
                weights.SetValue(u + 1, v + 1, weight)
        # Preserve the transmitted overlapping poles/imaginary knots once.
        return Geom_BSplineSurface(
            poles,
            weights,
            array(surface.u_knots, TColStd_Array1OfReal),
            array(surface.v_knots, TColStd_Array1OfReal),
            array(surface.u_knot_multiplicities, TColStd_Array1OfInteger),
            array(surface.v_knot_multiplicities, TColStd_Array1OfInteger),
            surface.u_degree,
            surface.v_degree,
            False,
            False,
        )
    if isinstance(surface, PlaneSurface):
        return Geom_Plane(
            gp_Ax3(gp_Pnt(*surface.point), gp_Dir(*surface.normal), gp_Dir(*surface.x_axis))
        )
    origin = surface.center if isinstance(surface, (SphereSurface, TorusSurface)) else surface.point
    axis = gp_Ax3(gp_Pnt(*origin), gp_Dir(*surface.axis), gp_Dir(*surface.x_axis))
    if isinstance(surface, CylinderSurface):
        return Geom_CylindricalSurface(axis, surface.radius)
    if isinstance(surface, ConeSurface):
        return Geom_ConicalSurface(
            axis, math.atan2(surface.sin_half_angle, surface.cos_half_angle), surface.radius
        )
    if isinstance(surface, SphereSurface):
        return Geom_SphericalSurface(axis, surface.radius)
    if isinstance(surface, TorusSurface):
        return Geom_ToroidalSurface(axis, surface.major_radius, surface.minor_radius)
    raise ValueError("unsupported source surface in release oracle")


def xyz(point):
    return [point.X(), point.Y(), point.Z()]


def edge_distance(point, edge):
    from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeVertex
    from OCP.BRepExtrema import BRepExtrema_DistShapeShape
    from OCP.gp import gp_Pnt

    result = BRepExtrema_DistShapeShape(BRepBuilderAPI_MakeVertex(gp_Pnt(*point)).Vertex(), edge)
    if not result.IsDone():
        raise ValueError("STEP edge distance did not complete")
    return result.Value()


def surface_distance(point, surface):
    from OCP.Extrema import Extrema_ExtAlgo_Tree
    from OCP.GeomAPI import GeomAPI_ProjectPointOnSurf
    from OCP.gp import gp_Pnt

    # A local gradient search can select the opposite lobe of a closed spline.
    projection = GeomAPI_ProjectPointOnSurf(gp_Pnt(*point), surface, 1e-12, Extrema_ExtAlgo_Tree)
    if projection.NbPoints() < 1:
        raise ValueError("surface projection did not complete")
    return projection.LowerDistance()
