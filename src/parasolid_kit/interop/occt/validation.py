"""OCCT inventory, validity, and metric validation with delayed runtime imports."""

from __future__ import annotations

from dataclasses import dataclass, field
from math import isclose

from ...brep.geometry import LineCurve, PlaneSurface
from ...brep.model import BrepModel
from ...diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity
from .model import NamedCounts, OcctMetrics, OcctShapeKind, OcctSubshape
from .options import OcctConversionOptions


@dataclass(frozen=True, slots=True)
class ShapeRegistry:
    """Deterministic, conversion-local lookup for one owned OCCT shape tree."""

    subshapes: tuple[OcctSubshape, ...]
    counts: NamedCounts
    _by_hash: dict[int, tuple[OcctSubshape, ...]] = field(
        init=False,
        repr=False,
        compare=False,
    )

    def __post_init__(self) -> None:
        values: dict[int, list[OcctSubshape]] = {}
        for item in self.subshapes:
            values.setdefault(hash(item.shape), []).append(item)
        object.__setattr__(
            self,
            "_by_hash",
            {key: tuple(items) for key, items in values.items()},
        )

    def key_for(self, shape: object) -> str:
        for item in self._by_hash.get(hash(shape), ()):
            if item.shape.IsSame(shape):
                return item.key
        raise KeyError("OCCT shape is not owned by the conversion result")


def input_topology_counts(model: BrepModel) -> NamedCounts:
    return NamedCounts.from_dict(
        {
            "bodies": len(model.bodies),
            "edges": len(model.edges),
            "faces": len(model.faces),
            "half_edges": len(model.half_edges),
            "loops": len(model.loops),
            "points": len(model.points),
            "regions": len(model.regions),
            "shells": len(model.shells),
            "vertices": len(model.vertices),
        }
    )


def input_curve_counts(model: BrepModel) -> NamedCounts:
    values: dict[str, int] = {}
    for curve in model.curves:
        values[curve.kind.value] = values.get(curve.kind.value, 0) + 1
    return NamedCounts.from_dict(values)


def input_surface_counts(model: BrepModel) -> NamedCounts:
    values: dict[str, int] = {}
    for surface in model.surfaces:
        values[surface.kind.value] = values.get(surface.kind.value, 0) + 1
    return NamedCounts.from_dict(values)


def collect_shape_registry(shape: object) -> ShapeRegistry:
    """Assign stable local keys by OCCT kind and indexed traversal order."""

    from OCP.TopAbs import (
        TopAbs_COMPOUND,
        TopAbs_COMPSOLID,
        TopAbs_EDGE,
        TopAbs_FACE,
        TopAbs_SHELL,
        TopAbs_SOLID,
        TopAbs_VERTEX,
        TopAbs_WIRE,
    )
    from OCP.TopExp import TopExp
    from OCP.TopTools import TopTools_IndexedMapOfShape

    kinds = (
        (OcctShapeKind.COMPOUND, "compounds", TopAbs_COMPOUND),
        (OcctShapeKind.COMPSOLID, "compsolids", TopAbs_COMPSOLID),
        (OcctShapeKind.SOLID, "solids", TopAbs_SOLID),
        (OcctShapeKind.SHELL, "shells", TopAbs_SHELL),
        (OcctShapeKind.FACE, "faces", TopAbs_FACE),
        (OcctShapeKind.WIRE, "wires", TopAbs_WIRE),
        (OcctShapeKind.EDGE, "edges", TopAbs_EDGE),
        (OcctShapeKind.VERTEX, "vertices", TopAbs_VERTEX),
    )
    result: list[OcctSubshape] = []
    counts: dict[str, int] = {}
    for kind, count_name, shape_type in kinds:
        indexed = TopTools_IndexedMapOfShape()
        TopExp.MapShapes_s(shape, shape_type, indexed)
        counts[count_name] = indexed.Extent()
        for index in range(1, indexed.Extent() + 1):
            result.append(
                OcctSubshape(
                    key=f"occt:{kind.value}:{index:06d}",
                    kind=kind,
                    shape=indexed.FindKey(index),
                )
            )
    return ShapeRegistry(tuple(result), NamedCounts.from_dict(counts))


# A face with more wires than this is checked in groups of wires (face_is_valid).
GROUPED_CHECK_WIRES = 256
# Inner wires checked together with the outer wire in one temporary face.
_WIRES_PER_GROUP = 128
# A wire box spanning more grid cells than this is compared with every wire;
# more such wires than _MAX_LARGE_WIRES make the face be checked whole.
_MAX_CELLS_PER_WIRE = 1024
_MAX_LARGE_WIRES = 64


def face_is_valid(face: object) -> bool:
    """``BRepCheck_Analyzer(face).IsValid()``, checked in groups for many wires.

    OCCT classifies every wire of a face against every other one, so a plate
    with thousands of holes takes minutes. Two inner wires whose parameter
    boxes do not overlap can neither cross nor nest, so such a face is checked
    as temporary faces of its outer wire and groups of inner wires, with any
    two overlapping wires in the same group. Every wire, with its edges and
    vertices, is checked against the same surface in one group. Faces on
    periodic surfaces, and faces whose inner wires the outer wire does not box,
    are checked whole.
    """
    from OCP.BRepCheck import BRepCheck_Analyzer

    parts = _wire_groups(face)
    if parts is None:
        return bool(BRepCheck_Analyzer(face).IsValid())
    return all(bool(BRepCheck_Analyzer(part).IsValid()) for part in parts)


def shape_is_valid(shape: object) -> bool:
    """``BRepCheck_Analyzer(shape).IsValid()``, with faces of many wires in groups.

    Without such a face, or a facet face, the whole shape is checked at once.
    Otherwise the other faces, the free edges and the free vertices are checked
    together, each many-wire face by :func:`face_is_valid`, and the shells and
    solids by the closure and orientation checks the analyzer applies to them.
    A facet face (stored triangles, no surface) is not a kernel B-Rep face:
    it is left out, with the shells and solids around it.
    """
    from OCP.BRep import BRep_Builder, BRep_Tool
    from OCP.BRepCheck import BRepCheck_Analyzer, BRepCheck_Shell, BRepCheck_Solid
    from OCP.BRepCheck import BRepCheck_Status as Status
    from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_SHELL, TopAbs_SOLID, TopAbs_VERTEX
    from OCP.TopExp import TopExp
    from OCP.TopoDS import TopoDS, TopoDS_Compound
    from OCP.TopTools import (
        TopTools_IndexedDataMapOfShapeListOfShape,
        TopTools_IndexedMapOfShape,
        TopTools_MapOfShape,
    )

    def indexed(root: object, kind: object) -> list:
        found = TopTools_IndexedMapOfShape()
        TopExp.MapShapes_s(root, kind, found)
        return [found.FindKey(index) for index in range(1, found.Extent() + 1)]

    def shape_set(shapes: list) -> object:
        result = TopTools_MapOfShape()
        for item in shapes:
            result.Add(item)
        return result

    faces = indexed(shape, TopAbs_FACE)
    facets = [face for face in faces if BRep_Tool.Surface_s(TopoDS.Face_s(face)) is None]
    heavy = [
        face
        for face in faces
        if _wire_count(face) > GROUPED_CHECK_WIRES
        and BRep_Tool.Surface_s(TopoDS.Face_s(face)) is not None
    ]
    if not heavy and not facets:
        return bool(BRepCheck_Analyzer(shape).IsValid())
    special, facet_set = shape_set([*heavy, *facets]), shape_set(facets)
    builder = BRep_Builder()
    rest = TopoDS_Compound()
    builder.MakeCompound(rest)
    for face in faces:
        if not special.Contains(face):
            builder.Add(rest, face)
    for kind, owner in ((TopAbs_EDGE, TopAbs_FACE), (TopAbs_VERTEX, TopAbs_EDGE)):
        owners = TopTools_IndexedDataMapOfShapeListOfShape()
        TopExp.MapShapesAndAncestors_s(shape, kind, owner, owners)
        for index in range(1, owners.Extent() + 1):
            if owners.FindFromIndex(index).IsEmpty():
                builder.Add(rest, owners.FindKey(index))
    if not BRepCheck_Analyzer(rest).IsValid():
        return False
    if not all(face_is_valid(face) for face in heavy):
        return False

    def has_facets(owner: object) -> bool:
        return bool(facets) and any(
            facet_set.Contains(face) for face in indexed(owner, TopAbs_FACE)
        )

    in_solids = TopTools_MapOfShape()
    for solid in indexed(shape, TopAbs_SOLID):
        if has_facets(solid):
            continue
        if set(BRepCheck_Solid(TopoDS.Solid_s(solid)).Status()) != {Status.BRepCheck_NoError}:
            return False
        for shell in indexed(solid, TopAbs_SHELL):
            in_solids.Add(shell)
    for shell in indexed(shape, TopAbs_SHELL):
        if has_facets(shell):
            continue
        check = BRepCheck_Shell(TopoDS.Shell_s(shell))
        if set(check.Status()) != {Status.BRepCheck_NoError}:
            return False
        if in_solids.Contains(shell) and (
            check.Closed() != Status.BRepCheck_NoError
            or check.Orientation() != Status.BRepCheck_NoError
        ):
            return False
    return True


def _wire_count(face: object) -> int:
    from OCP.TopoDS import TopoDS_Iterator

    count, children = 0, TopoDS_Iterator(face, False, False)
    while children.More():
        count += 1
        children.Next()
    return count


def _wire_groups(face: object) -> list | None:
    """Temporary faces covering the wires of ``face``, or ``None`` to check it whole."""
    from collections import defaultdict
    from math import floor

    from OCP.Bnd import Bnd_Box2d
    from OCP.BRep import BRep_Builder, BRep_Tool
    from OCP.BRepTools import BRepTools
    from OCP.Precision import Precision
    from OCP.TopAbs import TopAbs_WIRE
    from OCP.TopExp import TopExp_Explorer
    from OCP.TopoDS import TopoDS, TopoDS_Iterator

    stored = []
    children = TopoDS_Iterator(face, False, False)
    while children.More():
        stored.append(children.Value())
        children.Next()
    if len(stored) <= GROUPED_CHECK_WIRES or any(
        child.ShapeType() != TopAbs_WIRE for child in stored
    ):
        return None
    # The same wires as the face places them (with its location and orientation).
    placed = []
    explorer = TopExp_Explorer(face, TopAbs_WIRE)
    while explorer.More():
        placed.append(TopoDS.Wire_s(explorer.Current()))
        explorer.Next()
    surface = BRep_Tool.Surface_s(TopoDS.Face_s(face))
    if len(placed) != len(stored) or surface is None:
        return None
    if surface.IsUPeriodic() or surface.IsVPeriodic():
        return None
    outer = BRepTools.OuterWire_s(TopoDS.Face_s(face))
    outer_index = next(
        (index for index, wire in enumerate(placed) if not outer.IsNull() and wire.IsSame(outer)),
        None,
    )
    if outer_index is None:
        return None

    def bounds(wire: object) -> tuple[float, ...]:
        box = Bnd_Box2d()
        BRepTools.AddUVBounds_s(TopoDS.Face_s(face), wire, box)
        return tuple(float(value) for value in box.Get())

    u_low, v_low, u_high, v_high = bounds(placed[outer_index])
    margin = max(Precision.PConfusion_s(), 1e-9 * max(u_high - u_low, v_high - v_low))
    inner = [index for index in range(len(placed)) if index != outer_index]
    boxes = {}
    for index in inner:
        a, b, c, d = bounds(placed[index])
        if not (u_low - margin <= a <= c <= u_high + margin) or not (
            v_low - margin <= b <= d <= v_high + margin
        ):
            return None
        boxes[index] = (a - margin, b - margin, c + margin, d + margin)

    # Join the wires whose boxes overlap, finding candidates through a grid of
    # the median box size.
    sizes = sorted(max(c - a, d - b) for a, b, c, d in boxes.values())
    cell = max(sizes[len(sizes) // 2], margin)
    parent = {index: index for index in inner}

    def root(index: int) -> int:
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    def overlap(index: int, other: int) -> bool:
        a, b, c, d = boxes[index]
        p, q, r, s = boxes[other]
        return a <= r and p <= c and b <= s and q <= d

    grid: dict[tuple[int, int], list[int]] = defaultdict(list)
    large = []
    for index in inner:
        a, b, c, d = boxes[index]
        columns = range(floor((a - u_low) / cell), floor((c - u_low) / cell) + 1)
        rows = range(floor((b - v_low) / cell), floor((d - v_low) / cell) + 1)
        if len(columns) * len(rows) > _MAX_CELLS_PER_WIRE:
            large.append(index)
            continue
        for column in columns:
            for row in rows:
                for other in grid[column, row]:
                    if overlap(index, other):
                        parent[root(index)] = root(other)
                grid[column, row].append(index)
    if len(large) > _MAX_LARGE_WIRES:
        return None
    for index in large:
        for other in inner:
            if other != index and overlap(index, other):
                parent[root(index)] = root(other)
    components: dict[int, list[int]] = defaultdict(list)
    for index in inner:
        components[root(index)].append(index)

    builder = BRep_Builder()
    parts: list = []
    group: list[int] = []

    def flush() -> None:
        # An empty copy keeps the surface, location and orientation; Add takes
        # the wires as the face places them and stores them relative to it.
        part = TopoDS.Face_s(face.EmptyCopied())
        builder.Add(part, placed[outer_index])
        for index in group:
            builder.Add(part, placed[index])
        parts.append(part)
        group.clear()

    for members in components.values():
        if group and len(group) + len(members) > _WIRES_PER_GROUP:
            flush()
        group.extend(members)
    if group:
        flush()
    return parts


def validate_and_measure(shape: object) -> tuple[bool, OcctMetrics]:
    from OCP.Bnd import Bnd_Box
    from OCP.BRepBndLib import BRepBndLib
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps

    valid = shape_is_valid(shape)
    bounding_box = Bnd_Box()
    # Only facet faces carry a triangulation at this point; it is their geometry.
    BRepBndLib.AddOptimal_s(shape, bounding_box, True, True)
    bounds = None
    if not bounding_box.IsVoid():
        bounds = tuple(float(value) for value in bounding_box.Get())

    # The fixed Gauss rule, exact for the analytic and polynomial faces the
    # source metrics are compared with; it integrates a multi-span rational
    # B-spline face to about a tenth of a percent, where OCCT's adaptive rule
    # stops short on extruded surfaces instead.
    surface_properties = GProp_GProps()
    BRepGProp.SurfaceProperties_s(shape, surface_properties)
    volume_properties = GProp_GProps()
    BRepGProp.VolumeProperties_s(shape, volume_properties)
    return valid, OcctMetrics(
        bounding_box=bounds,
        surface_area=float(surface_properties.Mass()),
        volume=float(volume_properties.Mass()),
    )


def metric_diagnostics(
    model: BrepModel,
    metrics: OcctMetrics,
    options: OcctConversionOptions,
) -> tuple[Diagnostic, ...]:
    """Compare kernel metrics with source-model metrics in target units."""

    scale = options.applied_scale
    diagnostics: list[Diagnostic] = []
    source_bounds = model.metrics.bounding_box
    # Core bounds enclose source vertices, not extrema in the interior of curves
    # or surfaces. Equality is meaningful only for a polygonal planar model.
    polygonal = all(isinstance(c.definition, LineCurve) for c in model.curves) and all(
        isinstance(s.definition, PlaneSurface) for s in model.surfaces
    )
    if source_bounds is not None and metrics.bounding_box is not None:
        expected_bounds = tuple(
            value * scale
            for value in (
                *source_bounds.minimum.to_tuple(),
                *source_bounds.maximum.to_tuple(),
            )
        )
        for index, (expected, actual) in enumerate(
            zip(expected_bounds, metrics.bounding_box, strict=True)
        ):
            tolerance = options.validation.linear_threshold(expected)
            if polygonal:
                mismatch = not isclose(actual, expected, rel_tol=0.0, abs_tol=tolerance)
            else:
                tolerance = max(
                    tolerance,
                    max(((v.tolerance or 0) * scale for v in model.vertices), default=0.0),
                )
                mismatch = (
                    actual > expected + tolerance if index < 3 else actual < expected - tolerance
                )
            if mismatch:
                diagnostics.append(
                    _metric_diagnostic(
                        metric=f"bounding_box[{index}]",
                        expected=expected,
                        actual=actual,
                        tolerance=tolerance,
                        schema_key=model.schema_key,
                    )
                )

    if model.metrics.surface_area is not None and metrics.surface_area is not None:
        expected_area = model.metrics.surface_area * scale**2
        area_tolerance = options.validation.area_threshold(expected_area)
        if not isclose(
            metrics.surface_area,
            expected_area,
            rel_tol=0.0,
            abs_tol=area_tolerance,
        ):
            diagnostics.append(
                _metric_diagnostic(
                    metric="surface_area",
                    expected=expected_area,
                    actual=metrics.surface_area,
                    tolerance=area_tolerance,
                    schema_key=model.schema_key,
                )
            )

    if model.metrics.volume is not None and metrics.volume is not None:
        expected_volume = model.metrics.volume * scale**3
        volume_tolerance = options.validation.volume_threshold(expected_volume)
        if not isclose(
            metrics.volume,
            expected_volume,
            rel_tol=0.0,
            abs_tol=volume_tolerance,
        ):
            diagnostics.append(
                _metric_diagnostic(
                    metric="volume",
                    expected=expected_volume,
                    actual=metrics.volume,
                    tolerance=volume_tolerance,
                    schema_key=model.schema_key,
                )
            )
    return tuple(diagnostics)


def _metric_diagnostic(
    *,
    metric: str,
    expected: float,
    actual: float,
    tolerance: float,
    schema_key: str,
) -> Diagnostic:
    return Diagnostic(
        code="occt.metric_mismatch",
        severity=DiagnosticSeverity.ERROR,
        kind=DiagnosticKind.INVALID,
        message=f"OCCT {metric} differs from the scaled source metric",
        schema_key=schema_key,
        fatal=True,
        details={
            "metric": metric,
            "expected": expected,
            "actual": actual,
            "absolute_difference": abs(actual - expected),
            "tolerance": tolerance,
        },
    )


__all__ = [
    "ShapeRegistry",
    "collect_shape_registry",
    "input_curve_counts",
    "input_surface_counts",
    "input_topology_counts",
    "metric_diagnostics",
    "validate_and_measure",
]
