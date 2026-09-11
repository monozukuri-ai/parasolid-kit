"""Independent Onshape/STEP analytic oracle, executed outside the parser-only worker.

The comparison follows the existing M8 base campaign; unsupported geometry fails.
All tolerances and requested core metrics come from the frozen case sidecar.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

POSITION_TOL = 1e-9
DIRECTION_TOL = 1e-10


def require(condition: object, message: str) -> None:
    if not condition:
        raise ValueError(message)


def vec(p):
    return [p[k] for k in "xyz"] if isinstance(p, dict) else list(p)


def minus(a, b):
    return [x - y for x, y in zip(vec(a), vec(b), strict=True)]


def dot(a, b):
    return sum(x * y for x, y in zip(vec(a), vec(b), strict=True))


def parallel(a, b):
    return min(math.dist(vec(a), vec(b)), math.dist(vec(a), [-x for x in vec(b)])) <= DIRECTION_TOL


def axial(a, b, axis):
    d = minus(a, b)
    return math.sqrt(max(0, dot(d, d) - dot(d, axis) ** 2)) <= POSITION_TOL


def matches(a, b):
    if a["type"] not in {"PLANE", "LINE", "CYLINDER", "CIRCLE", "SPHERE", "ELLIPSE"}:
        raise ValueError("geometry outside the analytic oracle scope")
    if a["type"] != b["type"]:
        return False
    kind = a["type"]
    d = minus(a["origin"], b["origin"])
    if kind == "PLANE":
        return parallel(a["normal"], b["normal"]) and abs(dot(d, a["normal"])) <= POSITION_TOL
    if kind in ("LINE", "CYLINDER"):
        key = "direction" if kind == "LINE" else "axis"
        if not parallel(a[key], b[key]) or not axial(a["origin"], b["origin"], a[key]):
            return False
    elif math.dist(vec(a["origin"]), vec(b["origin"])) > POSITION_TOL:
        return False
    if kind in ("SPHERE", "CIRCLE", "CYLINDER") and abs(a["radius"] - b["radius"]) > POSITION_TOL:
        return False
    if kind in ("CIRCLE", "ELLIPSE") and not parallel(a["normal"], b["normal"]):
        return False
    if kind == "ELLIPSE":
        return (
            parallel(a["majorAxis"], b["majorAxis"])
            and abs(a["majorRadius"] - b["majorRadius"]) <= POSITION_TOL
            and abs(a["minorRadius"] - b["minorRadius"]) <= POSITION_TOL
        )
    return True


def step_geometry(path):
    from OCP.BRep import BRep_Tool
    from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Surface
    from OCP.BRepCheck import BRepCheck_Analyzer
    from OCP.BRepGProp import BRepGProp
    from OCP.GeomAbs import (
        GeomAbs_Circle,
        GeomAbs_Cylinder,
        GeomAbs_Ellipse,
        GeomAbs_Line,
        GeomAbs_Plane,
        GeomAbs_Sphere,
    )
    from OCP.GProp import GProp_GProps
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.Interface import Interface_Static
    from OCP.STEPControl import STEPControl_Reader
    from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_SOLID, TopAbs_VERTEX
    from OCP.TopExp import TopExp
    from OCP.TopoDS import TopoDS
    from OCP.TopTools import TopTools_IndexedMapOfShape

    reader = STEPControl_Reader()
    previous = Interface_Static.CVal_s("xstep.cascade.unit")
    try:
        require(Interface_Static.SetCVal_s("xstep.cascade.unit", "M"), "STEP units")
        require(reader.ReadFile(str(path)) == IFSelect_RetDone, "STEP read")
        require(reader.TransferRoots() > 0, "STEP roots")
        shape = reader.OneShape()
    finally:
        Interface_Static.SetCVal_s("xstep.cascade.unit", previous)
    require(not shape.IsNull() and BRepCheck_Analyzer(shape).IsValid(), "STEP validity")

    def shapes(kind):
        values = TopTools_IndexedMapOfShape()
        TopExp.MapShapes_s(shape, kind, values)
        return [values.FindKey(i) for i in range(1, values.Extent() + 1)]

    def xyz(p):
        return [p.X(), p.Y(), p.Z()]

    geometry = []
    other_curve_types = []
    for edge in shapes(TopAbs_EDGE):
        adaptor = BRepAdaptor_Curve(TopoDS.Edge_s(edge))
        kind = adaptor.GetType()
        if kind == GeomAbs_Line:
            e = adaptor.Line()
            g = dict(type="LINE", origin=xyz(e.Location()), direction=xyz(e.Direction()))
        elif kind == GeomAbs_Circle:
            e = adaptor.Circle()
            g = dict(
                type="CIRCLE",
                origin=xyz(e.Location()),
                normal=xyz(e.Axis().Direction()),
                radius=e.Radius(),
            )
        elif kind == GeomAbs_Ellipse:
            e = adaptor.Ellipse()
            g = dict(
                type="ELLIPSE",
                origin=xyz(e.Location()),
                normal=xyz(e.Axis().Direction()),
                majorAxis=xyz(e.XAxis().Direction()),
                majorRadius=e.MajorRadius(),
                minorRadius=e.MinorRadius(),
            )
        else:
            # STEP may introduce seam/degenerate edges. Every source primitive is
            # still required to match a native and STEP primitive below.
            other_curve_types.append(str(kind))
            continue
        geometry.append(g)
    for face in shapes(TopAbs_FACE):
        adaptor = BRepAdaptor_Surface(TopoDS.Face_s(face))
        kind = adaptor.GetType()
        if kind == GeomAbs_Plane:
            s = adaptor.Plane()
            g = dict(type="PLANE", origin=xyz(s.Location()), normal=xyz(s.Axis().Direction()))
        elif kind == GeomAbs_Cylinder:
            s = adaptor.Cylinder()
            g = dict(
                type="CYLINDER",
                origin=xyz(s.Location()),
                axis=xyz(s.Axis().Direction()),
                radius=s.Radius(),
            )
        elif kind == GeomAbs_Sphere:
            s = adaptor.Sphere()
            g = dict(type="SPHERE", origin=xyz(s.Location()), radius=s.Radius())
        else:
            raise ValueError("unexpected STEP surface " + str(kind))
        geometry.append(g)
    a, v = GProp_GProps(), GProp_GProps()
    BRepGProp.SurfaceProperties_s(shape, a, Eps=1e-10, SkipShared=False)
    BRepGProp.VolumeProperties_s(shape, v, Eps=1e-10, OnlyClosed=True, SkipShared=False)
    return dict(
        geometry=geometry,
        other_curve_types=other_curve_types,
        points=[xyz(BRep_Tool.Pnt_s(TopoDS.Vertex_s(p))) for p in shapes(TopAbs_VERTEX)],
        faces=len(shapes(TopAbs_FACE)),
        bodies=len(shapes(TopAbs_SOLID)),
        area=a.Mass(),
        volume=v.Mass(),
    )


def source_geometry(model):
    geometry = []
    for entity in (*model.curves, *model.surfaces):
        d = entity.definition
        kind = entity.kind
        if kind == "line":
            value = dict(type="LINE", origin=list(d.point), direction=list(d.direction))
        elif kind == "circle":
            value = dict(
                type="CIRCLE", origin=list(d.center), normal=list(d.normal), radius=d.radius
            )
        elif kind == "ellipse":
            value = dict(
                type="ELLIPSE",
                origin=list(d.center),
                normal=list(d.normal),
                majorAxis=list(d.x_axis),
                majorRadius=d.major_radius,
                minorRadius=d.minor_radius,
            )
        elif kind == "plane":
            value = dict(type="PLANE", origin=list(d.point), normal=list(d.normal))
        elif kind == "cylinder":
            value = dict(type="CYLINDER", origin=list(d.point), axis=list(d.axis), radius=d.radius)
        elif kind == "sphere":
            value = dict(type="SPHERE", origin=list(d.center), radius=d.radius)
        else:
            raise ValueError(f"source {kind} is outside the analytic oracle scope")
        geometry.append(value)
    return geometry


def verify(root, path, specification):
    global POSITION_TOL, DIRECTION_TOL
    from parasolid_kit import read_brep

    require(
        specification["kind"] == "onshape_analytic" and specification["unit"] == "m",
        "unsupported oracle or source units",
    )
    for name, value in specification.items():
        if name.endswith("_tolerance"):
            require(math.isfinite(value) and value > 0, "invalid oracle tolerance")
    POSITION_TOL = specification["linear_tolerance"]
    DIRECTION_TOL = specification["direction_tolerance"]
    model = read_brep(path).brep
    require(model.complete and model.topology.valid, "source B-Rep is incomplete")
    details = json.loads((root / specification["body_details"]["path"]).read_text())
    mass = json.loads((root / specification["mass_properties"]["path"]).read_text())
    version = specification["microversion"]
    require(
        details["microversionId"]["theId"] == mass["microversionId"] == version,
        "native oracle saved states differ",
    )
    require(len(details["bodies"]) == len(model.bodies), "native body count differs")
    producer = []
    vertices = []
    for body in details["bodies"]:
        producer.extend(e["curve"] for e in body.get("edges", []))
        producer.extend(f["surface"] for f in body["faces"])
        vertices.extend(v["point"] for v in body.get("vertices", []))
    for name in ("faces", "edges", "vertices"):
        require(
            sum(len(b.get(name, [])) for b in details["bodies"]) == len(getattr(model, name)),
            "native topology differs: " + name,
        )
    oracle = step_geometry(root / specification["step"]["path"])
    require(
        oracle["faces"] == len(model.faces) and oracle["bodies"] == len(model.bodies),
        "STEP body/face counts differ",
    )
    geometry = source_geometry(model)
    for decoded in geometry:
        for label, reference in [("native", producer), ("STEP", oracle["geometry"])]:
            require(any(matches(decoded, g) for g in reference), label + " geometry differs")
    for point in model.points:
        for label, reference in [("native", vertices), ("STEP", oracle["points"])]:
            require(
                any(math.dist(list(point.position), vec(p)) <= POSITION_TOL for p in reference),
                label + " vertex position differs",
            )
    values = mass["bodies"]["-all-"]
    for name, field, metric in [
        ("area", "periphery", "surface_area"),
        ("volume", "volume", "volume"),
    ]:
        tolerance = specification[name + "_tolerance"]
        require(
            values[field][1] - tolerance <= oracle[name] <= values[field][2] + tolerance,
            "STEP/native " + name + " differs",
        )
        if metric in specification["core_metrics"]:
            value = getattr(model.metrics, metric)
            require(value is not None, "required core metric was not measured: " + metric)
            require(
                math.isclose(
                    value,
                    oracle[name],
                    abs_tol=tolerance,
                    rel_tol=specification["relative_tolerance"],
                ),
                "core/STEP " + metric + " differs",
            )
    return {
        "status": "passed",
        "kind": "onshape_analytic",
        "unit": "m",
        "geometry_count": len(geometry),
        "point_count": len(model.points),
        "step_area": oracle["area"],
        "step_volume": oracle["volume"],
        "core_metrics_checked": specification["core_metrics"],
        "step_additional_curve_types": oracle["other_curve_types"],
    }


def main():
    try:
        root, path = map(Path, sys.argv[1:3])
        specification = json.loads(sys.argv[3])
        result = verify(root, path, specification)
    except Exception as error:
        print(json.dumps({"status": "failed", "error": str(error)}))
        return 1
    print(json.dumps(result, allow_nan=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
