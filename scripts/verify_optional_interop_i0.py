#!/usr/bin/env python3
"""Exercise the optional OCCT/CadQuery dependency profiles for the I0 spike.

This script deliberately constructs synthetic OCCT shapes without importing
``parasolid_kit``.  It proves the third-party API surface needed by the future
adapter without making that prototype part of the public parser API.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from importlib.metadata import PackageNotFoundError, metadata, version
from typing import Any


def _distribution(name: str) -> dict[str, str] | None:
    try:
        package_metadata = metadata(name)
    except PackageNotFoundError:
        return None
    return {
        "name": name,
        "version": version(name),
        "requires_python": package_metadata.get("Requires-Python", ""),
    }


def _installed_distributions() -> dict[str, dict[str, str] | None]:
    return {
        name: _distribution(name)
        for name in (
            "cadquery-ocp-novtk",
            "cadquery-ocp",
            "cadquery-ocp-proxy",
            "cadquery",
        )
    }


def _validate_profile(
    expected_profile: str,
    distributions: dict[str, dict[str, str] | None],
) -> None:
    novtk = distributions["cadquery-ocp-novtk"]
    full = distributions["cadquery-ocp"]
    cadquery = distributions["cadquery"]
    if novtk is not None and full is not None:
        raise RuntimeError("cadquery-ocp-novtk and cadquery-ocp provide the same OCP namespace")
    if expected_profile == "occt" and (novtk is None or full is not None or cadquery is not None):
        raise RuntimeError("occt profile must contain only cadquery-ocp-novtk")
    if expected_profile == "cadquery" and (novtk is not None or full is None or cadquery is None):
        raise RuntimeError("cadquery profile must contain cadquery and cadquery-ocp")


def _shape_count(shape: object, shape_type: object) -> int:
    from OCP.TopExp import TopExp
    from OCP.TopTools import TopTools_IndexedMapOfShape

    indexed = TopTools_IndexedMapOfShape()
    TopExp.MapShapes_s(shape, shape_type, indexed)
    return indexed.Extent()


def _shape_metrics(shape: object) -> dict[str, Any]:
    from OCP.Bnd import Bnd_Box
    from OCP.BRepBndLib import BRepBndLib
    from OCP.BRepCheck import BRepCheck_Analyzer
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps
    from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_SOLID, TopAbs_VERTEX

    bounding_box = Bnd_Box()
    BRepBndLib.Add_s(shape, bounding_box)
    x_min, y_min, z_min, x_max, y_max, z_max = bounding_box.Get()

    surface_properties = GProp_GProps()
    BRepGProp.SurfaceProperties_s(shape, surface_properties)
    volume_properties = GProp_GProps()
    BRepGProp.VolumeProperties_s(shape, volume_properties)

    return {
        "valid": BRepCheck_Analyzer(shape).IsValid(),
        "solid_count": _shape_count(shape, TopAbs_SOLID),
        "face_count": _shape_count(shape, TopAbs_FACE),
        "edge_count": _shape_count(shape, TopAbs_EDGE),
        "vertex_count": _shape_count(shape, TopAbs_VERTEX),
        "bbox": [x_min, y_min, z_min, x_max, y_max, z_max],
        "surface_area": surface_properties.Mass(),
        "volume": volume_properties.Mass(),
    }


def _assert_close(
    actual: float,
    expected: float,
    name: str,
    *,
    absolute_tolerance: float = 1.0e-9,
) -> None:
    if not math.isclose(
        actual,
        expected,
        rel_tol=1.0e-10,
        abs_tol=absolute_tolerance,
    ):
        raise RuntimeError(f"{name}: expected {expected!r}, observed {actual!r}")


def _assert_bbox(actual: list[float], expected: list[float], name: str) -> None:
    if len(actual) != len(expected):
        raise RuntimeError(f"{name}: unexpected bounding-box length")
    for index, (actual_value, expected_value) in enumerate(zip(actual, expected, strict=True)):
        _assert_close(
            actual_value,
            expected_value,
            f"{name}[{index}]",
            absolute_tolerance=1.0e-6,
        )


def _build_box() -> tuple[object, dict[str, Any]]:
    from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox

    length = 40.0
    width = 30.0
    height = 20.0
    shape = BRepPrimAPI_MakeBox(length, width, height).Shape()
    metrics = _shape_metrics(shape)
    if not metrics["valid"] or metrics["solid_count"] != 1:
        raise RuntimeError("synthetic box is not one valid solid")
    if metrics["face_count"] != 6:
        raise RuntimeError("synthetic box does not contain six faces")
    _assert_bbox(metrics["bbox"], [0.0, 0.0, 0.0, length, width, height], "box bbox")
    expected_area = 2.0 * (length * width + width * height + height * length)
    _assert_close(metrics["surface_area"], expected_area, "box area")
    _assert_close(metrics["volume"], length * width * height, "box volume")
    return shape, metrics


def _build_cylinder_hole() -> tuple[object, dict[str, Any]]:
    from OCP.BRepAlgoAPI import BRepAlgoAPI_Cut
    from OCP.BRepPrimAPI import BRepPrimAPI_MakeCylinder
    from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt

    outer_radius = 20.0
    inner_radius = 8.0
    height = 30.0
    margin = 1.0

    outer = BRepPrimAPI_MakeCylinder(outer_radius, height).Shape()
    inner_axis = gp_Ax2(gp_Pnt(0.0, 0.0, -margin), gp_Dir(0.0, 0.0, 1.0))
    inner = BRepPrimAPI_MakeCylinder(inner_axis, inner_radius, height + 2.0 * margin).Shape()
    operation = BRepAlgoAPI_Cut(outer, inner)
    operation.Build()
    if not operation.IsDone():
        raise RuntimeError("OCCT boolean cut did not complete")

    shape = operation.Shape()
    metrics = _shape_metrics(shape)
    if not metrics["valid"] or metrics["solid_count"] != 1:
        raise RuntimeError("synthetic cylinder-hole is not one valid solid")
    if metrics["face_count"] != 4:
        raise RuntimeError("synthetic cylinder-hole does not contain four faces")

    expected_area = (
        2.0 * math.pi * outer_radius * height
        + 2.0 * math.pi * inner_radius * height
        + 2.0 * math.pi * (outer_radius**2 - inner_radius**2)
    )
    expected_volume = math.pi * (outer_radius**2 - inner_radius**2) * height
    _assert_bbox(
        metrics["bbox"],
        [-outer_radius, -outer_radius, 0.0, outer_radius, outer_radius, height],
        "cylinder-hole bbox",
    )
    _assert_close(metrics["surface_area"], expected_area, "cylinder-hole area")
    _assert_close(metrics["volume"], expected_volume, "cylinder-hole volume")
    return shape, metrics


def _build_periodic_ring_edge() -> tuple[object, dict[str, Any]]:
    from OCP.BRep import BRep_Tool
    from OCP.BRepAdaptor import BRepAdaptor_Curve
    from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge
    from OCP.GeomAbs import GeomAbs_Circle
    from OCP.gp import gp_Ax2, gp_Circ, gp_Dir, gp_Pnt
    from OCP.TopAbs import TopAbs_VERTEX

    circle = gp_Circ(
        gp_Ax2(gp_Pnt(0.0, 0.0, 12.5), gp_Dir(0.0, 0.0, 1.0)),
        7.5,
    )
    edge_builder = BRepBuilderAPI_MakeEdge(circle)
    if not edge_builder.IsDone():
        raise RuntimeError("OCCT periodic circle edge construction failed")
    edge = edge_builder.Edge()
    curve = BRepAdaptor_Curve(edge)
    result = {
        "geometrically_closed": BRep_Tool.IsClosed_s(edge),
        "curve_type_is_circle": curve.GetType() == GeomAbs_Circle,
        "first_parameter": curve.FirstParameter(),
        "last_parameter": curve.LastParameter(),
        "vertex_count": _shape_count(edge, TopAbs_VERTEX),
    }
    if not result["geometrically_closed"] or not result["curve_type_is_circle"]:
        raise RuntimeError("periodic ring edge did not remain a closed circle")
    _assert_close(result["first_parameter"], 0.0, "ring first parameter")
    _assert_close(result["last_parameter"], 2.0 * math.pi, "ring last parameter")
    return edge, result


def _probe_low_level_geometry_builders() -> dict[str, Any]:
    from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Surface
    from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge, BRepBuilderAPI_MakeFace
    from OCP.BRepCheck import BRepCheck_Analyzer
    from OCP.GeomAbs import GeomAbs_Cylinder, GeomAbs_Line, GeomAbs_Plane
    from OCP.gp import gp_Ax3, gp_Cylinder, gp_Dir, gp_Pln, gp_Pnt

    line_builder = BRepBuilderAPI_MakeEdge(
        gp_Pnt(1.0, 2.0, 3.0),
        gp_Pnt(11.0, 2.0, 3.0),
    )
    if not line_builder.IsDone():
        raise RuntimeError("OCCT line edge construction failed")
    line_edge = line_builder.Edge()
    line_curve = BRepAdaptor_Curve(line_edge)
    if line_curve.GetType() != GeomAbs_Line:
        raise RuntimeError("OCCT line edge did not retain line geometry")

    plane = gp_Pln(gp_Pnt(0.0, 0.0, 0.0), gp_Dir(0.0, 0.0, 1.0))
    plane_builder = BRepBuilderAPI_MakeFace(plane, -5.0, 5.0, -4.0, 4.0)
    if not plane_builder.IsDone():
        raise RuntimeError("OCCT bounded plane face construction failed")
    plane_face = plane_builder.Face()
    plane_surface = BRepAdaptor_Surface(plane_face)
    if plane_surface.GetType() != GeomAbs_Plane:
        raise RuntimeError("OCCT plane face did not retain plane geometry")

    cylinder = gp_Cylinder(
        gp_Ax3(gp_Pnt(0.0, 0.0, 0.0), gp_Dir(0.0, 0.0, 1.0)),
        6.0,
    )
    cylinder_builder = BRepBuilderAPI_MakeFace(
        cylinder,
        0.0,
        2.0 * math.pi,
        0.0,
        15.0,
    )
    if not cylinder_builder.IsDone():
        raise RuntimeError("OCCT bounded cylinder face construction failed")
    cylinder_face = cylinder_builder.Face()
    cylinder_surface = BRepAdaptor_Surface(cylinder_face)
    if cylinder_surface.GetType() != GeomAbs_Cylinder:
        raise RuntimeError("OCCT cylinder face did not retain cylinder geometry")

    result = {
        "line": {
            "valid": BRepCheck_Analyzer(line_edge).IsValid(),
            "first_parameter": line_curve.FirstParameter(),
            "last_parameter": line_curve.LastParameter(),
        },
        "plane": {
            "valid": BRepCheck_Analyzer(plane_face).IsValid(),
            "u_bounds": [plane_surface.FirstUParameter(), plane_surface.LastUParameter()],
            "v_bounds": [plane_surface.FirstVParameter(), plane_surface.LastVParameter()],
        },
        "cylinder": {
            "valid": BRepCheck_Analyzer(cylinder_face).IsValid(),
            "u_bounds": [
                cylinder_surface.FirstUParameter(),
                cylinder_surface.LastUParameter(),
            ],
            "v_bounds": [
                cylinder_surface.FirstVParameter(),
                cylinder_surface.LastVParameter(),
            ],
        },
    }
    if not all(section["valid"] for section in result.values()):
        raise RuntimeError("one or more low-level OCCT geometry builders produced invalid output")
    _assert_close(result["line"]["first_parameter"], 0.0, "line first parameter")
    _assert_close(result["line"]["last_parameter"], 10.0, "line last parameter")
    return result


def _cadquery_metrics(box: object, cylinder_hole: object) -> dict[str, Any] | None:
    try:
        import cadquery as cq
    except ModuleNotFoundError:
        return None

    box_shape = cq.Shape.cast(box)
    cylinder_hole_root = cq.Shape.cast(cylinder_hole)
    cylinder_hole_solids = cylinder_hole_root.Solids()
    if len(cylinder_hole_solids) != 1:
        raise RuntimeError("CadQuery cylinder-hole root does not contain one solid")
    cylinder_hole_shape = cylinder_hole_solids[0]
    compound = cq.Compound.makeCompound([box_shape, cylinder_hole_shape])

    _assert_close(box_shape.Volume(), 24_000.0, "CadQuery box volume")
    _assert_close(
        cylinder_hole_shape.Volume(),
        math.pi * (20.0**2 - 8.0**2) * 30.0,
        "CadQuery cylinder-hole volume",
    )
    if len(compound.Solids()) != 2:
        raise RuntimeError("CadQuery compound does not contain two solids")

    return {
        "box_type": type(box_shape).__name__,
        "box_volume": box_shape.Volume(),
        "cylinder_hole_root_type": type(cylinder_hole_root).__name__,
        "cylinder_hole_type": type(cylinder_hole_shape).__name__,
        "cylinder_hole_volume": cylinder_hole_shape.Volume(),
        "compound_type": type(compound).__name__,
        "compound_solid_count": len(compound.Solids()),
    }


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--expect-profile",
        choices=("occt", "cadquery"),
        required=True,
        help="dependency profile that must be present in the current environment",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(sys.argv[1:] if argv is None else argv)
    distributions = _installed_distributions()
    _validate_profile(args.expect_profile, distributions)

    box, box_metrics = _build_box()
    cylinder_hole, cylinder_hole_metrics = _build_cylinder_hole()
    _, periodic_edge = _build_periodic_ring_edge()
    low_level_geometry = _probe_low_level_geometry_builders()
    cadquery_metrics = _cadquery_metrics(box, cylinder_hole)
    if args.expect_profile == "cadquery" and cadquery_metrics is None:
        raise RuntimeError("cadquery profile did not provide the cadquery import")
    if args.expect_profile == "occt" and cadquery_metrics is not None:
        raise RuntimeError("occt profile unexpectedly provided the cadquery import")

    result = {
        "schema_version": 1,
        "profile": args.expect_profile,
        "python_version": ".".join(str(item) for item in sys.version_info[:3]),
        "distributions": distributions,
        "explicit_healing_used": False,
        "box": box_metrics,
        "cylinder_hole": cylinder_hole_metrics,
        "periodic_ring_edge": periodic_edge,
        "low_level_geometry": low_level_geometry,
        "cadquery": cadquery_metrics,
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
