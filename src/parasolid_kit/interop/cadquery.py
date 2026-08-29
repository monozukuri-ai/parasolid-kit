"""CadQuery wrappers for strict documented OCCT conversion results.

The adapter deliberately returns CadQuery ``Shape`` objects, not ``Workplane``
or ``Assembly`` values. It does not reconstruct feature history or infer
product structure that is absent from the Parasolid B-Rep source model.
"""

from __future__ import annotations

from math import isclose
from types import ModuleType
from typing import TYPE_CHECKING, Any

from ..brep.model import BrepModel
from ..diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity
from .dependency import require_cadquery
from .errors import CadQueryConversionError
from .limits import DEFAULT_INTEROP_LIMITS, InteropLimits
from .occt.conversion import to_occt
from .occt.model import OcctConversionResult, ShapeRelationKind, SourceEntityKind
from .occt.options import LengthUnit, ValidationTolerances

if TYPE_CHECKING:
    from cadquery import Shape as CadQueryShape
else:
    CadQueryShape = Any


def load_runtime() -> ModuleType:
    """Validate the installed profile before importing and returning CadQuery."""

    return require_cadquery()


def to_cadquery(
    brep: BrepModel,
    *,
    source_unit: LengthUnit,
    target_unit: LengthUnit = "mm",
    require_complete: bool = True,
    heal: bool = False,
    validation: ValidationTolerances | None = None,
    limits: InteropLimits = DEFAULT_INTEROP_LIMITS,
    source_identity: str | None = None,
) -> CadQueryShape:
    """Convert one B-Rep to a CadQuery Shape or explicit multi-body Compound.

    A single source body is returned as the most specific CadQuery ``Shape``
    subclass available for its OCCT topology. Multiple source bodies are
    returned as one ``cadquery.Compound`` in source-body order.
    """

    runtime, converted, shapes = _convert_body_shapes(
        brep,
        source_unit=source_unit,
        target_unit=target_unit,
        require_complete=require_complete,
        heal=heal,
        validation=validation,
        limits=limits,
        source_identity=source_identity,
    )
    if len(shapes) == 1:
        result = shapes[0]
    else:
        try:
            result = runtime.Compound.makeCompound(list(shapes))
        except (KeyboardInterrupt, SystemExit):
            raise
        except Exception as error:
            raise _adapter_error(
                converted,
                code="cadquery.wrap_failed",
                message=f"CadQuery multi-body Compound construction failed: {error}",
                details={"exception_type": type(error).__name__},
            ) from error
    _validate_wrapper(result, converted)
    return result


def to_cadquery_shapes(
    brep: BrepModel,
    *,
    source_unit: LengthUnit,
    target_unit: LengthUnit = "mm",
    require_complete: bool = True,
    heal: bool = False,
    validation: ValidationTolerances | None = None,
    limits: InteropLimits = DEFAULT_INTEROP_LIMITS,
    source_identity: str | None = None,
) -> tuple[CadQueryShape, ...]:
    """Return one CadQuery Shape per source body, preserving source order."""

    runtime, converted, shapes = _convert_body_shapes(
        brep,
        source_unit=source_unit,
        target_unit=target_unit,
        require_complete=require_complete,
        heal=heal,
        validation=validation,
        limits=limits,
        source_identity=source_identity,
    )
    try:
        combined = shapes[0] if len(shapes) == 1 else runtime.Compound.makeCompound(list(shapes))
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception as error:
        raise _adapter_error(
            converted,
            code="cadquery.wrap_failed",
            message=f"CadQuery body validation Compound construction failed: {error}",
            details={"exception_type": type(error).__name__},
        ) from error
    _validate_wrapper(combined, converted)
    return shapes


def _convert_body_shapes(
    brep: BrepModel,
    *,
    source_unit: LengthUnit,
    target_unit: LengthUnit,
    require_complete: bool,
    heal: bool,
    validation: ValidationTolerances | None,
    limits: InteropLimits,
    source_identity: str | None,
) -> tuple[ModuleType, OcctConversionResult, tuple[CadQueryShape, ...]]:
    if not isinstance(brep, BrepModel):
        raise TypeError("brep must be BrepModel")
    if not isinstance(limits, InteropLimits):
        raise TypeError("limits must be InteropLimits")

    # Require the full profile before to_occt(), otherwise a base installation
    # would misleadingly recommend the narrower [occt] extra.
    runtime = require_cadquery()
    converted = to_occt(
        brep,
        source_unit=source_unit,
        target_unit=target_unit,
        require_complete=require_complete,
        heal=heal,
        validation=validation,
        limits=limits,
        source_identity=source_identity,
    )
    subshapes = {item.key: item.shape for item in converted.subshapes}
    wrapped: list[CadQueryShape] = []
    for body in brep.bodies:
        target_keys = tuple(
            dict.fromkeys(
                relation.target_key
                for relation in converted.source_map.relations
                if relation.source.kind is SourceEntityKind.BODY
                and relation.source.entity_id == body.id
                and relation.relation is not ShapeRelationKind.GENERATED
            )
        )
        if len(target_keys) != 1:
            raise _adapter_error(
                converted,
                code="cadquery.body_mapping_invalid",
                message="each source body must map to exactly one OCCT body shape",
                details={
                    "source_body_id": body.id,
                    "target_count": len(target_keys),
                },
            )
        target = subshapes.get(target_keys[0])
        if target is None:
            raise _adapter_error(
                converted,
                code="cadquery.body_mapping_invalid",
                message="source body mapping references an unavailable OCCT subshape",
                details={
                    "source_body_id": body.id,
                    "target_key": target_keys[0],
                },
            )
        try:
            shape = runtime.Shape.cast(target)
        except (KeyboardInterrupt, SystemExit):
            raise
        except Exception as error:
            raise _adapter_error(
                converted,
                code="cadquery.wrap_failed",
                message=f"CadQuery could not wrap source body {body.id}: {error}",
                details={
                    "source_body_id": body.id,
                    "target_key": target_keys[0],
                    "exception_type": type(error).__name__,
                },
            ) from error
        if not isinstance(shape, runtime.Shape):
            raise _adapter_error(
                converted,
                code="cadquery.wrap_failed",
                message="CadQuery Shape.cast returned a non-Shape value",
                details={
                    "source_body_id": body.id,
                    "actual_type": type(shape).__name__,
                },
            )
        wrapped.append(shape)

    if len(wrapped) != len(brep.bodies) or not wrapped:
        raise _adapter_error(
            converted,
            code="cadquery.body_mapping_invalid",
            message="CadQuery body wrapping did not preserve the source body count",
            details={
                "source_body_count": len(brep.bodies),
                "wrapped_body_count": len(wrapped),
            },
        )
    return runtime, converted, tuple(wrapped)


def _validate_wrapper(shape: CadQueryShape, converted: OcctConversionResult) -> None:
    expected = converted.report.metrics
    tolerances = converted.report.options.validation
    try:
        valid = bool(shape.isValid())
        bounding_box = shape.BoundingBox()
        actual_bounds = tuple(
            float(getattr(bounding_box, name))
            for name in ("xmin", "ymin", "zmin", "xmax", "ymax", "zmax")
        )
        actual_area = float(shape.Area())
        # CadQuery's Shape.Volume() dispatches by the wrapped shape's
        # dimensionality, so it returns area for a Face. Sum only Solid
        # children to obtain a metric comparable with OCCT volume properties.
        actual_volume = sum(float(solid.Volume()) for solid in shape.Solids())
        actual_faces = len(shape.Faces())
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception as error:
        raise _adapter_error(
            converted,
            code="cadquery.validation_failed",
            message=f"CadQuery wrapper validation failed: {error}",
            details={"exception_type": type(error).__name__},
        ) from error

    if not valid:
        raise _adapter_error(
            converted,
            code="cadquery.invalid_shape",
            message="CadQuery rejected the wrapped OCCT topology",
        )
    expected_faces = converted.report.output_topology.to_dict().get("faces", 0)
    if actual_faces != expected_faces:
        raise _adapter_error(
            converted,
            code="cadquery.topology_mismatch",
            message="CadQuery face count differs from the OCCT conversion result",
            details={"expected": expected_faces, "actual": actual_faces},
        )
    if expected.bounding_box is not None:
        for index, (reference, actual) in enumerate(
            zip(expected.bounding_box, actual_bounds, strict=True)
        ):
            _require_close(
                converted,
                metric=f"bounding_box[{index}]",
                expected=reference,
                actual=actual,
                tolerance=tolerances.linear_threshold(reference),
            )
    if expected.surface_area is not None:
        _require_close(
            converted,
            metric="surface_area",
            expected=expected.surface_area,
            actual=actual_area,
            tolerance=tolerances.area_threshold(expected.surface_area),
        )
    if expected.volume is not None:
        _require_close(
            converted,
            metric="volume",
            expected=expected.volume,
            actual=actual_volume,
            tolerance=tolerances.volume_threshold(expected.volume),
        )


def _require_close(
    converted: OcctConversionResult,
    *,
    metric: str,
    expected: float,
    actual: float,
    tolerance: float,
) -> None:
    if isclose(actual, expected, rel_tol=0.0, abs_tol=tolerance):
        return
    raise _adapter_error(
        converted,
        code="cadquery.metric_mismatch",
        message=f"CadQuery {metric} differs from the OCCT conversion result",
        details={
            "metric": metric,
            "expected": expected,
            "actual": actual,
            "absolute_difference": abs(actual - expected),
            "tolerance": tolerance,
        },
    )


def _adapter_error(
    converted: OcctConversionResult,
    *,
    code: str,
    message: str,
    details: dict[str, Any] | None = None,
) -> CadQueryConversionError:
    return CadQueryConversionError(
        Diagnostic(
            code=code,
            severity=DiagnosticSeverity.ERROR,
            kind=DiagnosticKind.INVALID,
            message=message,
            schema_key=converted.report.schema_key,
            fatal=True,
            details=details or {},
        ),
        result=converted,
    )


__all__ = ["load_runtime", "to_cadquery", "to_cadquery_shapes"]
