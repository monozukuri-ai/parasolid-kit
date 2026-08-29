"""OCCT inventory, validity, and metric validation with delayed runtime imports."""

from __future__ import annotations

from dataclasses import dataclass, field
from math import isclose

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


def validate_and_measure(shape: object) -> tuple[bool, OcctMetrics]:
    from OCP.Bnd import Bnd_Box
    from OCP.BRepBndLib import BRepBndLib
    from OCP.BRepCheck import BRepCheck_Analyzer
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps

    valid = bool(BRepCheck_Analyzer(shape).IsValid())
    bounding_box = Bnd_Box()
    BRepBndLib.AddOptimal_s(shape, bounding_box, False, True)
    bounds = None
    if not bounding_box.IsVoid():
        bounds = tuple(float(value) for value in bounding_box.Get())

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
            if not isclose(actual, expected, rel_tol=0.0, abs_tol=tolerance):
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
