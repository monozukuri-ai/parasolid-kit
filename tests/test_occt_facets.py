"""Facet faces on the OCCT path, and faces with many holes checked in groups.

A facet face becomes an OCCT face that carries only its triangulation: OCCT
meshes, bounds, measures and writes it from the triangles, while the kernel's
validity checks, which need a surface, leave it out. A face with more wires
than ``GROUPED_CHECK_WIRES`` is checked as groups of wires that cannot touch.
"""

from __future__ import annotations

import struct

import pytest

from parasolid_kit import (
    Body,
    BodyKind,
    BrepMetrics,
    BrepModel,
    ByteRange,
    Face,
    MeshSurface,
    Region,
    RegionKind,
    Sense,
    Shell,
    SourceNodeRef,
    SurfaceGeometry,
    SurfaceKind,
    TopologyValidation,
)
from parasolid_kit.interop.occt import conversion, to_occt, validation

pytest.importorskip("OCP")

from OCP.BRep import BRep_Builder, BRep_Tool
from OCP.BRepBuilderAPI import (
    BRepBuilderAPI_MakeEdge,
    BRepBuilderAPI_MakeFace,
    BRepBuilderAPI_MakePolygon,
    BRepBuilderAPI_MakeWire,
)
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepGProp import BRepGProp
from OCP.BRepPrimAPI import BRepPrimAPI_MakePrism
from OCP.gp import gp_Ax2, gp_Circ, gp_Dir, gp_Pln, gp_Pnt, gp_Vec
from OCP.GProp import GProp_GProps
from OCP.TopAbs import TopAbs_FACE, TopAbs_REVERSED
from OCP.TopExp import TopExp
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import TopoDS, TopoDS_Compound, TopoDS_Face
from OCP.TopTools import TopTools_IndexedMapOfShape

# A tetrahedron with 10 mm edges, in metres, outward winding.
POSITIONS = (0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.01)
TRIANGLES = (0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 3, 2)


def source(index: int, name: str) -> SourceNodeRef:
    return SourceNodeRef(index, 10 + index, name, index, ByteRange(10 * index, 10 * index + 9))


def facet_model(sense: Sense = Sense.POSITIVE) -> BrepModel:
    mesh = MeshSurface(
        vertex_count=4,
        triangle_count=4,
        positions=struct.pack("<12d", *POSITIONS),
        triangles=struct.pack("<12I", *TRIANGLES),
        sources=(source(6, "PSM_MESH"),),
    )
    return BrepModel(
        source_format="binary",
        schema_key="synthetic-facets",
        complete=True,
        bodies=(Body(1, BodyKind.SOLID, 1e-6, 1e-8, (1,), (), (), source(1, "BODY")),),
        regions=(Region(1, RegionKind.SOLID, 1, (1,), source(2, "REGION")),),
        shells=(Shell(1, 1, (), (1,), (), None, source(3, "SHELL")),),
        faces=(Face(1, 1, 1, (), 1, sense, source(4, "FACE")),),
        loops=(),
        half_edges=(),
        edges=(),
        vertices=(),
        points=(),
        curves=(),
        surfaces=(
            SurfaceGeometry(1, Sense.POSITIVE, None, SurfaceKind.MESH, mesh, source(5, "MESH")),
        ),
        topology=TopologyValidation(True, 1, 0, 0),
        metrics=BrepMetrics(None, None, None),
        diagnostics=(),
    )


def faces(shape: object) -> list:
    found = TopTools_IndexedMapOfShape()
    TopExp.MapShapes_s(shape, TopAbs_FACE, found)
    return [TopoDS.Face_s(found.FindKey(index)) for index in range(1, found.Extent() + 1)]


@pytest.mark.parametrize("parametric", [False, True], ids=["analytic", "parametric"])
def test_a_facet_face_carries_only_its_triangulation(monkeypatch, parametric):
    monkeypatch.setattr(conversion, "_uses_parametric_topology", lambda model: parametric)
    result = to_occt(facet_model(), source_unit="m", target_unit="mm")
    assert result.report.conversion_complete and result.report.occt_valid
    (face,) = faces(result.shape)
    assert BRep_Tool.Surface_s(face) is None
    triangulation = BRep_Tool.Triangulation_s(face, TopLoc_Location())
    assert (triangulation.NbNodes(), triangulation.NbTriangles()) == (4, 4)
    assert triangulation.Node(2).X() == pytest.approx(10.0)
    assert triangulation.Triangle(1).Get() == (1, 3, 2)
    volume = GProp_GProps()
    BRepGProp.VolumeProperties_s(result.shape, volume)
    assert volume.Mass() == pytest.approx(1000.0 / 6.0)


def test_a_facet_face_against_its_surface_is_reversed():
    result = to_occt(facet_model(Sense.NEGATIVE), source_unit="m", target_unit="mm")
    (face,) = faces(result.shape)
    assert face.Orientation() == TopAbs_REVERSED
    assert result.report.occt_valid


def plate(holes: int = 300, *, variant: str | None = None) -> TopoDS_Face:
    """A 210 x 170 plate with a 20-column grid of 4 mm holes 10 mm apart."""
    outer = BRepBuilderAPI_MakePolygon(
        gp_Pnt(0, 0, 0), gp_Pnt(210, 0, 0), gp_Pnt(210, 170, 0), gp_Pnt(0, 170, 0), True
    ).Wire()
    builder = BRepBuilderAPI_MakeFace(gp_Pln(), outer, True)

    def hole(x: float, y: float, radius: float = 2.0, *, reverse: bool = True) -> None:
        circle = gp_Circ(gp_Ax2(gp_Pnt(x, y, 0), gp_Dir(0, 0, 1)), radius)
        wire = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(circle).Edge()).Wire()
        builder.Add(TopoDS.Wire_s(wire.Reversed()) if reverse else wire)

    for index in range(holes):
        hole(10 + 10 * (index % 20), 10 + 10 * (index // 20))
    if variant == "crossing":
        hole(13.0, 10.0)
    elif variant == "nested":
        hole(10.0, 10.0, 1.0)
    elif variant == "misoriented":
        hole(205.0, 165.0, 2.0, reverse=False)
    elif variant == "outside":
        hole(230.0, 75.0)
    return builder.Face()


def test_a_valid_face_with_many_holes_is_checked_in_groups():
    face = plate()
    parts = validation._wire_groups(face)
    assert parts is not None and len(parts) > 1
    assert validation.face_is_valid(face) is True
    assert bool(BRepCheck_Analyzer(face).IsValid()) is True


@pytest.mark.parametrize("variant", ["crossing", "nested", "misoriented", "outside"])
def test_a_face_with_many_holes_fails_as_the_kernel_does(variant):
    face = plate(variant=variant)
    assert bool(BRepCheck_Analyzer(face).IsValid()) is False
    assert validation.face_is_valid(face) is False


def test_a_solid_with_many_holes_is_checked_in_parts():
    solid = BRepPrimAPI_MakePrism(plate(), gp_Vec(0, 0, 5)).Shape()
    assert validation.shape_is_valid(solid) is True
    assert bool(BRepCheck_Analyzer(solid).IsValid()) is True
    # A face below the threshold keeps the kernel's whole-shape check.
    small = BRepPrimAPI_MakePrism(plate(holes=3), gp_Vec(0, 0, 5)).Shape()
    assert validation.shape_is_valid(small) is True


def test_a_shape_with_a_facet_face_skips_only_that_face():
    facets = to_occt(facet_model(), source_unit="m", target_unit="mm").shape
    solid = BRepPrimAPI_MakePrism(plate(holes=3), gp_Vec(0, 0, 5)).Shape()
    builder, both = BRep_Builder(), TopoDS_Compound()
    builder.MakeCompound(both)
    builder.Add(both, facets)
    builder.Add(both, solid)
    assert validation.shape_is_valid(both) is True
    # The kernel alone rejects the face without a surface.
    assert bool(BRepCheck_Analyzer(both).IsValid()) is False
