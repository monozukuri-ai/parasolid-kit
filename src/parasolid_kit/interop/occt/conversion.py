"""Public strict ``BrepModel`` to OCCT conversion entry point."""

from __future__ import annotations

from collections import Counter
from collections.abc import Iterable
from itertools import pairwise
from math import isfinite

from ... import __version__
from ...brep.geometry import (
    CircleCurve,
    ConeSurface,
    CurveKind,
    CylinderSurface,
    EllipseCurve,
    HyperbolaCurve,
    LineCurve,
    NurbsCurve,
    NurbsSurface,
    OffsetSurface,
    ParabolaCurve,
    PlaneSurface,
    SphereSurface,
    SurfaceKind,
    SurfaceParametricCurve,
    TorusSurface,
    TrimmedCurve,
)
from ...brep.model import BrepModel
from ...brep.topology import BodyKind, Sense, SourceNodeRef
from ...diagnostics import (
    Diagnostic,
    DiagnosticKind,
    DiagnosticSeverity,
    SourceLocation,
)
from ..dependency import installed_interop_distributions, require_occt
from ..errors import OcctConversionError
from ..limits import DEFAULT_INTEROP_LIMITS, InteropLimits
from .geometry import GeometryFactory
from .model import (
    ConversionReport,
    InteropUsage,
    NamedCounts,
    OcctConversionResult,
    OcctMetrics,
    ShapeRelation,
    ShapeRelationKind,
    SourceShapeMap,
)
from .options import LengthUnit, OcctConversionOptions, ValidationTolerances
from .topology import PendingRelation, TopologyBuilder
from .validation import (
    ShapeRegistry,
    collect_shape_registry,
    input_curve_counts,
    input_surface_counts,
    input_topology_counts,
    metric_diagnostics,
    validate_and_measure,
)


def to_occt(
    brep: BrepModel,
    *,
    source_unit: LengthUnit,
    target_unit: LengthUnit = "mm",
    require_complete: bool = True,
    heal: bool = False,
    validation: ValidationTolerances | None = None,
    limits: InteropLimits = DEFAULT_INTEROP_LIMITS,
    source_identity: str | None = None,
) -> OcctConversionResult:
    """Convert the exact documented subset without inference, approximation, or healing."""

    if not isinstance(brep, BrepModel):
        raise TypeError("brep must be BrepModel")
    if not isinstance(limits, InteropLimits):
        raise TypeError("limits must be InteropLimits")
    options = OcctConversionOptions(
        source_unit=source_unit,
        target_unit=target_unit,
        require_complete=require_complete,
        heal=heal,
        validation=validation or ValidationTolerances(),
        source_identity=source_identity,
    )
    context = _ConversionContext(brep, options, limits)
    preflight = _preflight_diagnostics(context)
    if preflight:
        _raise_conversion(context, preflight[0], diagnostics=preflight)

    runtime = require_occt()
    context.ocp_version = _optional_text(getattr(runtime, "__version__", None))
    context.ocp_distribution = _ocp_distribution()
    try:
        built = TopologyBuilder(brep, GeometryFactory(options)).build()
        registry = collect_shape_registry(built.shape)
        context.registry = registry
        context.operations = built.operations
        if len(registry.subshapes) > limits.max_occt_subshapes:
            diagnostic = _diagnostic(
                context,
                code="occt.limit_exceeded",
                kind=DiagnosticKind.LIMIT,
                message="OCCT output subshape count exceeds the configured interop limit",
                details={
                    "resource": "max_occt_subshapes",
                    "observed": len(registry.subshapes),
                    "limit": limits.max_occt_subshapes,
                },
            )
            _raise_conversion(context, diagnostic)
        output_vertices = registry.counts.to_dict().get("vertices", 0)
        if output_vertices > limits.max_vertices:
            diagnostic = _diagnostic(
                context,
                code="occt.limit_exceeded",
                kind=DiagnosticKind.LIMIT,
                message="OCCT output vertex count exceeds the configured interop limit",
                details={
                    "resource": "max_vertices",
                    "observed": output_vertices,
                    "limit": limits.max_vertices,
                },
            )
            _raise_conversion(context, diagnostic)
        source_map = _resolve_relations(registry, built.relations)
        context.source_map = source_map
        valid, metrics = validate_and_measure(built.shape)
        context.metrics = metrics
        context.occt_valid = valid
        validation_diagnostics: list[Diagnostic] = []
        if not valid:
            validation_diagnostics.append(
                _diagnostic(
                    context,
                    code="occt.invalid_shape",
                    kind=DiagnosticKind.INVALID,
                    message="OCCT BRepCheck_Analyzer rejected the converted shape",
                )
            )
        validation_diagnostics.extend(metric_diagnostics(brep, metrics, options))
        if validation_diagnostics:
            _raise_conversion(
                context,
                validation_diagnostics[0],
                diagnostics=validation_diagnostics,
            )
    except OcctConversionError:
        raise
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception as error:
        diagnostic = _diagnostic(
            context,
            code="occt.construction_failed",
            kind=DiagnosticKind.INVALID,
            message=f"OCCT topology construction failed: {error}",
            details={"exception_type": type(error).__name__},
        )
        conversion_error = _conversion_error(context, diagnostic)
        raise conversion_error from error

    context.conversion_complete = True
    report = context.report()
    return OcctConversionResult(
        shape=built.shape,
        subshapes=registry.subshapes,
        source_map=source_map,
        report=report,
    )


class _ConversionContext:
    def __init__(
        self,
        brep: BrepModel,
        options: OcctConversionOptions,
        limits: InteropLimits,
    ) -> None:
        self.brep = brep
        self.options = options
        self.limits = limits
        self.ocp_distribution: str | None = None
        self.ocp_version: str | None = None
        self.registry: ShapeRegistry | None = None
        self.source_map: SourceShapeMap | None = None
        self.metrics = OcctMetrics(None, None, None)
        self.occt_valid = False
        self.conversion_complete = False
        self.diagnostics: list[Diagnostic] = list(brep.diagnostics)
        self.operations: tuple[str, ...] = ()

    @property
    def entity_count(self) -> int:
        return sum(
            len(values)
            for values in (
                self.brep.bodies,
                self.brep.regions,
                self.brep.shells,
                self.brep.faces,
                self.brep.loops,
                self.brep.half_edges,
                self.brep.edges,
                self.brep.vertices,
                self.brep.points,
                self.brep.curves,
                self.brep.surfaces,
            )
        )

    def report(self) -> ConversionReport:
        registry = self.registry
        source_map = self.source_map
        generated = 0
        mapping_count = 0
        if source_map is not None:
            mapping_count = len(source_map.relations)
            generated = len(
                {
                    relation.target_key
                    for relation in source_map.relations
                    if relation.relation is ShapeRelationKind.GENERATED
                }
            )
        subshape_count = 0 if registry is None else len(registry.subshapes)
        vertex_count = 0
        output_topology = _empty_output_counts()
        if registry is not None:
            output_topology = registry.counts
            vertex_count = registry.counts.to_dict().get("vertices", 0)
        return ConversionReport(
            schema_version=1,
            producer="parasolid-kit.interop.occt",
            parser_version=__version__,
            ocp_distribution=self.ocp_distribution,
            ocp_version=self.ocp_version,
            source_identity=self.options.source_identity,
            source_format=self.brep.source_format,
            schema_key=self.brep.schema_key,
            options=self.options,
            source_complete=self.brep.complete,
            conversion_complete=self.conversion_complete,
            occt_valid=self.occt_valid,
            input_topology=input_topology_counts(self.brep),
            input_curve_kinds=input_curve_counts(self.brep),
            input_surface_kinds=input_surface_counts(self.brep),
            output_topology=output_topology,
            metrics=self.metrics,
            mapping_relation_count=mapping_count,
            generated_topology_count=generated,
            diagnostics=tuple(self.diagnostics),
            limits=self.limits,
            usage=InteropUsage(
                entities=self.entity_count,
                occt_subshapes=subshape_count,
                vertices=vertex_count,
                diagnostics=len(self.diagnostics),
            ),
            topology_operations=self.operations,
            healing_requested=self.options.heal,
            healing_performed=False,
        )


def _preflight_diagnostics(context: _ConversionContext) -> list[Diagnostic]:
    model = context.brep
    result: list[Diagnostic] = []
    if context.options.heal:
        result.append(
            _diagnostic(
                context,
                code="occt.healing_unavailable",
                kind=DiagnosticKind.UNSUPPORTED,
                message="the exact OCCT adapter does not perform shape healing; pass heal=False",
            )
        )
    if context.options.require_complete and not model.complete:
        result.append(
            _diagnostic(
                context,
                code="occt.source_incomplete",
                kind=DiagnosticKind.INCOMPLETE,
                message="strict OCCT conversion requires BrepModel.complete=True",
            )
        )
    if not model.topology.valid:
        result.append(
            _diagnostic(
                context,
                code="occt.invalid_topology",
                kind=DiagnosticKind.INVALID,
                message="OCCT conversion requires a valid linked B-Rep topology graph",
            )
        )
    if not model.bodies:
        result.append(
            _diagnostic(
                context,
                code="occt.invalid_topology",
                kind=DiagnosticKind.INVALID,
                message="OCCT conversion requires at least one source body",
            )
        )
    if context.entity_count > context.limits.max_entities:
        result.append(
            _diagnostic(
                context,
                code="occt.limit_exceeded",
                kind=DiagnosticKind.LIMIT,
                message="source entity count exceeds the configured interop limit",
                details={
                    "resource": "max_entities",
                    "observed": context.entity_count,
                    "limit": context.limits.max_entities,
                },
            )
        )
    surface_kinds = {surface.id: surface.kind for surface in model.surfaces}
    ring_face_count = sum(
        surface_kinds.get(face.surface) in {SurfaceKind.CYLINDER, SurfaceKind.CONE}
        for face in model.faces
    )
    generated_face_count = sum(
        surface_kinds.get(face.surface) in {SurfaceKind.SPHERE, SurfaceKind.TORUS}
        or (
            not face.loops
            and surface_kinds.get(face.surface) in {SurfaceKind.NURBS, SurfaceKind.OFFSET}
        )
        for face in model.faces
    )
    control_point_count = sum(
        len(curve.definition.control_vertices)
        for curve in model.curves
        if isinstance(curve.definition, NurbsCurve)
    ) + sum(
        len(surface.definition.control_vertices)
        for surface in model.surfaces
        if isinstance(surface.definition, NurbsSurface)
    )
    estimated_vertices = (
        len(model.vertices) + 2 * ring_face_count + 4 * generated_face_count + control_point_count
    )
    body_topology_count = sum(2 if body.kind is BodyKind.SOLID else 1 for body in model.bodies)
    estimated_wires = len(model.loops) - ring_face_count + generated_face_count
    estimated_subshapes = (
        (1 if len(model.bodies) > 1 else 0)
        + body_topology_count
        + len(model.faces)
        + estimated_wires
        + len(model.edges)
        + len(model.vertices)
        + 3 * ring_face_count
        + 8 * generated_face_count
    )
    if estimated_subshapes > context.limits.max_occt_subshapes:
        result.append(
            _diagnostic(
                context,
                code="occt.limit_exceeded",
                kind=DiagnosticKind.LIMIT,
                message="estimated OCCT topology exceeds the configured subshape limit",
                details={
                    "resource": "max_occt_subshapes",
                    "observed": estimated_subshapes,
                    "limit": context.limits.max_occt_subshapes,
                },
            )
        )
    if estimated_vertices > context.limits.max_vertices:
        result.append(
            _diagnostic(
                context,
                code="occt.limit_exceeded",
                kind=DiagnosticKind.LIMIT,
                message="estimated OCCT vertices exceed the configured vertex limit",
                details={
                    "resource": "max_vertices",
                    "observed": estimated_vertices,
                    "limit": context.limits.max_vertices,
                },
            )
        )

    result.extend(_unique_id_diagnostics(context))
    result.extend(_conditional_geometry_diagnostics(context))
    result.extend(_unsupported_diagnostics(context))
    result.extend(_reference_diagnostics(context))
    result.extend(_geometry_cycle_diagnostics(context))
    if len(context.diagnostics) + len(result) > context.limits.max_diagnostics:
        result = result[: max(0, context.limits.max_diagnostics - len(context.diagnostics))]
        result.append(
            _diagnostic(
                context,
                code="occt.limit_exceeded",
                kind=DiagnosticKind.LIMIT,
                message="conversion diagnostics exceed the configured interop limit",
                details={
                    "resource": "max_diagnostics",
                    "observed": len(context.diagnostics) + len(result),
                    "limit": context.limits.max_diagnostics,
                },
            )
        )
    return result


def _unsupported_diagnostics(context: _ConversionContext) -> list[Diagnostic]:
    model = context.brep
    result: list[Diagnostic] = []
    for body in model.bodies:
        if body.kind not in {BodyKind.SOLID, BodyKind.SHEET}:
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_body",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(
                        f"body {body.id} kind {body.kind.value} is not supported by the "
                        "exact OCCT coverage contract"
                    ),
                    source=body.source,
                    details={"entity_kind": "body", "entity_id": body.id},
                )
            )
    for curve in model.curves:
        supported = _supported_curve_definition(curve.kind, curve.definition)
        if not supported:
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_curve",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(
                        f"curve {curve.id} kind {curve.kind.value} is not supported by the "
                        "exact OCCT coverage contract"
                    ),
                    source=curve.source,
                    details={
                        "entity_kind": "curve",
                        "entity_id": curve.id,
                        "geometry_kind": curve.kind.value,
                    },
                )
            )
    for surface in model.surfaces:
        supported = _supported_surface_definition(surface.kind, surface.definition)
        if not supported:
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_surface",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(
                        f"surface {surface.id} kind {surface.kind.value} is not supported by "
                        "the exact OCCT coverage contract"
                    ),
                    source=surface.source,
                    details={
                        "entity_kind": "surface",
                        "entity_id": surface.id,
                        "geometry_kind": surface.kind.value,
                    },
                )
            )
    for face in model.faces:
        if face.sense is Sense.UNKNOWN:
            result.append(_unknown_sense(context, "face", face.id, face.source))
    for surface in model.surfaces:
        if surface.sense is Sense.UNKNOWN:
            result.append(_unknown_sense(context, "surface", surface.id, surface.source))
    for curve in model.curves:
        if curve.sense is Sense.UNKNOWN:
            result.append(_unknown_sense(context, "curve", curve.id, curve.source))
    curves = {curve.id: curve for curve in model.curves}
    for edge in model.edges:
        curve_ids = {edge.curve} if edge.curve is not None else set()
        curve_ids.update(
            half_edge.curve
            for half_edge in model.half_edges
            if half_edge.id in edge.half_edges and half_edge.curve is not None
        )
        if len(curve_ids) == 1:
            curve = curves.get(next(iter(curve_ids)))
            if curve is None:
                continue
            has_start = edge.start_vertex is not None
            has_end = edge.end_vertex is not None
            if curve.kind in {CurveKind.CIRCLE, CurveKind.ELLIPSE} and (has_start or has_end):
                result.append(
                    _diagnostic(
                        context,
                        code="occt.unsupported_curve",
                        kind=DiagnosticKind.UNSUPPORTED,
                        message=(
                            f"{curve.kind.value} edge {edge.id} is vertex-trimmed; use an "
                            "explicit trimmed curve with source parameters"
                        ),
                        source=edge.source,
                        details={
                            "entity_kind": "edge",
                            "entity_id": edge.id,
                            "geometry_kind": f"trimmed_{curve.kind.value}",
                        },
                    )
                )
            if has_start != has_end:
                result.append(
                    _diagnostic(
                        context,
                        code="occt.unsupported_topology",
                        kind=DiagnosticKind.UNSUPPORTED,
                        message=f"edge {edge.id} has only one endpoint vertex",
                        source=edge.source,
                        details={"entity_kind": "edge", "entity_id": edge.id},
                    )
                )
            if curve.kind in {
                CurveKind.LINE,
                CurveKind.PARABOLA,
                CurveKind.HYPERBOLA,
                CurveKind.TRIMMED,
            } and not (has_start and has_end):
                result.append(
                    _diagnostic(
                        context,
                        code="occt.unsupported_curve",
                        kind=DiagnosticKind.UNSUPPORTED,
                        message=(
                            f"{curve.kind.value} edge {edge.id} requires two endpoint vertices"
                        ),
                        source=edge.source,
                        details={
                            "entity_kind": "edge",
                            "entity_id": edge.id,
                            "geometry_kind": curve.kind.value,
                        },
                    )
                )
            if curve.kind is CurveKind.NURBS and isinstance(curve.definition, NurbsCurve):
                definition = curve.definition
                closed = definition.closed or definition.periodic
                if closed == (has_start and has_end):
                    result.append(
                        _diagnostic(
                            context,
                            code="occt.unsupported_curve",
                            kind=DiagnosticKind.UNSUPPORTED,
                            message=(
                                f"NURBS edge {edge.id} endpoint topology does not match its "
                                "closed/periodic flags"
                            ),
                            source=edge.source,
                            details={
                                "entity_kind": "edge",
                                "entity_id": edge.id,
                                "geometry_kind": curve.kind.value,
                            },
                        )
                    )
    for face in model.faces:
        if face.surface is None:
            continue
        surface = next((item for item in model.surfaces if item.id == face.surface), None)
        if surface is None:
            continue
        if surface.kind in {SurfaceKind.SPHERE, SurfaceKind.TORUS} and face.loops:
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_topology",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(
                        f"{surface.kind.value} face {face.id} must be an untrimmed closed face"
                    ),
                    source=face.source,
                    details={
                        "entity_kind": "face",
                        "entity_id": face.id,
                        "geometry_kind": surface.kind.value,
                    },
                )
            )
        if surface.kind in {SurfaceKind.NURBS, SurfaceKind.OFFSET} and len(face.loops) > 1:
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_topology",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(f"{surface.kind.value} face {face.id} supports at most one trim loop"),
                    source=face.source,
                    details={
                        "entity_kind": "face",
                        "entity_id": face.id,
                        "geometry_kind": surface.kind.value,
                    },
                )
            )
    loop_half_edges = {half_edge_id for loop in model.loops for half_edge_id in loop.half_edges}
    for half_edge in model.half_edges:
        if half_edge.id in loop_half_edges and half_edge.dummy:
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_topology",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=f"loop uses dummy half-edge {half_edge.id}",
                    source=half_edge.source,
                    details={"entity_kind": "half_edge", "entity_id": half_edge.id},
                )
            )
        if half_edge.id in loop_half_edges and half_edge.sense is Sense.UNKNOWN:
            result.append(_unknown_sense(context, "half_edge", half_edge.id, half_edge.source))
    return result


def _supported_curve_definition(kind: CurveKind, definition: object) -> bool:
    return any(
        (
            kind is expected_kind and isinstance(definition, expected_type)
            for expected_kind, expected_type in (
                (CurveKind.LINE, LineCurve),
                (CurveKind.CIRCLE, CircleCurve),
                (CurveKind.ELLIPSE, EllipseCurve),
                (CurveKind.PARABOLA, ParabolaCurve),
                (CurveKind.HYPERBOLA, HyperbolaCurve),
                (CurveKind.TRIMMED, TrimmedCurve),
                (CurveKind.NURBS, NurbsCurve),
            )
        )
    )


def _supported_surface_definition(kind: SurfaceKind, definition: object) -> bool:
    return any(
        (
            kind is expected_kind and isinstance(definition, expected_type)
            for expected_kind, expected_type in (
                (SurfaceKind.PLANE, PlaneSurface),
                (SurfaceKind.CYLINDER, CylinderSurface),
                (SurfaceKind.CONE, ConeSurface),
                (SurfaceKind.SPHERE, SphereSurface),
                (SurfaceKind.TORUS, TorusSurface),
                (SurfaceKind.NURBS, NurbsSurface),
                (SurfaceKind.OFFSET, OffsetSurface),
            )
        )
    )


def _conditional_geometry_diagnostics(context: _ConversionContext) -> list[Diagnostic]:
    result: list[Diagnostic] = []
    for curve in context.brep.curves:
        definition = curve.definition
        if isinstance(definition, NurbsCurve) and (
            definition.rational or definition.vertex_dimension != 3
        ):
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_curve",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(
                        f"NURBS curve {curve.id} requires a rational or non-3D control-vertex "
                        "interpretation that is not established by I7"
                    ),
                    source=curve.source,
                    details={
                        "entity_kind": "curve",
                        "entity_id": curve.id,
                        "geometry_kind": CurveKind.NURBS.value,
                        "rational": definition.rational,
                        "vertex_dimension": definition.vertex_dimension,
                    },
                )
            )
            continue
        if isinstance(definition, NurbsCurve) and (definition.periodic or definition.closed):
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_curve",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(
                        f"NURBS curve {curve.id} is closed or periodic; I7 has not established "
                        "the exact source-to-OCCT pole and knot relationship"
                    ),
                    source=curve.source,
                    details={
                        "entity_kind": "curve",
                        "entity_id": curve.id,
                        "geometry_kind": CurveKind.NURBS.value,
                        "closed": definition.closed,
                        "periodic": definition.periodic,
                    },
                )
            )
            continue
        if isinstance(definition, NurbsCurve):
            issue = _nurbs_curve_issue(definition)
            if issue is not None:
                result.append(
                    _diagnostic(
                        context,
                        code="occt.invalid_geometry",
                        kind=DiagnosticKind.INVALID,
                        message=f"NURBS curve {curve.id} is invalid: {issue}",
                        source=curve.source,
                        details={
                            "entity_kind": "curve",
                            "entity_id": curve.id,
                            "geometry_kind": CurveKind.NURBS.value,
                            "reason": issue,
                        },
                    )
                )
    for surface in context.brep.surfaces:
        definition = surface.definition
        if isinstance(definition, NurbsSurface) and (
            definition.rational or definition.vertex_dimension != 3
        ):
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_surface",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(
                        f"NURBS surface {surface.id} requires a rational or non-3D control-grid "
                        "interpretation that is not established by I7"
                    ),
                    source=surface.source,
                    details={
                        "entity_kind": "surface",
                        "entity_id": surface.id,
                        "geometry_kind": SurfaceKind.NURBS.value,
                        "rational": definition.rational,
                        "vertex_dimension": definition.vertex_dimension,
                    },
                )
            )
            continue
        if isinstance(definition, NurbsSurface) and (
            definition.u_periodic
            or definition.v_periodic
            or definition.u_closed
            or definition.v_closed
        ):
            result.append(
                _diagnostic(
                    context,
                    code="occt.unsupported_surface",
                    kind=DiagnosticKind.UNSUPPORTED,
                    message=(
                        f"NURBS surface {surface.id} is closed or periodic; I7 has not "
                        "established the exact source-to-OCCT pole and knot relationship"
                    ),
                    source=surface.source,
                    details={
                        "entity_kind": "surface",
                        "entity_id": surface.id,
                        "geometry_kind": SurfaceKind.NURBS.value,
                        "u_closed": definition.u_closed,
                        "v_closed": definition.v_closed,
                        "u_periodic": definition.u_periodic,
                        "v_periodic": definition.v_periodic,
                    },
                )
            )
            continue
        if isinstance(definition, NurbsSurface):
            issue = _nurbs_surface_issue(definition)
            if issue is not None:
                result.append(
                    _diagnostic(
                        context,
                        code="occt.invalid_geometry",
                        kind=DiagnosticKind.INVALID,
                        message=f"NURBS surface {surface.id} is invalid: {issue}",
                        source=surface.source,
                        details={
                            "entity_kind": "surface",
                            "entity_id": surface.id,
                            "geometry_kind": SurfaceKind.NURBS.value,
                            "reason": issue,
                        },
                    )
                )
        if isinstance(definition, OffsetSurface) and not isfinite(definition.offset):
            result.append(
                _diagnostic(
                    context,
                    code="occt.invalid_geometry",
                    kind=DiagnosticKind.INVALID,
                    message=f"offset surface {surface.id} distance must be finite",
                    source=surface.source,
                    details={
                        "entity_kind": "surface",
                        "entity_id": surface.id,
                        "geometry_kind": SurfaceKind.OFFSET.value,
                    },
                )
            )
    return result


def _nurbs_curve_issue(definition: NurbsCurve) -> str | None:
    if definition.control_vertex_count != len(definition.control_vertices):
        return "control vertex count does not match its payload"
    if definition.degree < 1 or definition.degree >= definition.control_vertex_count:
        return "degree is outside its valid control-point range"
    if any(len(values) != 3 for values in definition.control_vertices):
        return "control vertices must contain exactly three coordinates"
    if any(not isfinite(value) for values in definition.control_vertices for value in values):
        return "control vertices must be finite"
    return _nurbs_knot_issue(
        definition.knots,
        definition.knot_multiplicities,
        definition.control_vertex_count + definition.degree + 1,
    )


def _nurbs_surface_issue(definition: NurbsSurface) -> str | None:
    expected = definition.u_control_vertex_count * definition.v_control_vertex_count
    if expected != len(definition.control_vertices):
        return "control grid dimensions do not match its payload"
    if (
        definition.u_degree < 1
        or definition.u_degree >= definition.u_control_vertex_count
        or definition.v_degree < 1
        or definition.v_degree >= definition.v_control_vertex_count
    ):
        return "degree is outside its valid control-point range"
    if any(len(values) != 3 for values in definition.control_vertices):
        return "control vertices must contain exactly three coordinates"
    if any(not isfinite(value) for values in definition.control_vertices for value in values):
        return "control vertices must be finite"
    u_issue = _nurbs_knot_issue(
        definition.u_knots,
        definition.u_knot_multiplicities,
        definition.u_control_vertex_count + definition.u_degree + 1,
    )
    if u_issue is not None:
        return f"U {u_issue}"
    v_issue = _nurbs_knot_issue(
        definition.v_knots,
        definition.v_knot_multiplicities,
        definition.v_control_vertex_count + definition.v_degree + 1,
    )
    return None if v_issue is None else f"V {v_issue}"


def _nurbs_knot_issue(
    knots: tuple[float, ...],
    multiplicities: tuple[int, ...],
    expected_expanded: int,
) -> str | None:
    if not knots or len(knots) != len(multiplicities):
        return "knots and multiplicities must have equal non-zero length"
    if any(not isfinite(value) for value in knots):
        return "knots must be finite"
    if any(left >= right for left, right in pairwise(knots)):
        return "distinct knots must be strictly increasing"
    if any(value <= 0 for value in multiplicities):
        return "knot multiplicities must be positive"
    if sum(multiplicities) != expected_expanded:
        return "expanded knot count does not equal control points plus degree plus one"
    return None


def _geometry_cycle_diagnostics(context: _ConversionContext) -> list[Diagnostic]:
    result: list[Diagnostic] = []
    curve_basis = {
        curve.id: curve.definition.basis_curve
        for curve in context.brep.curves
        if isinstance(curve.definition, TrimmedCurve)
    }
    surface_basis = {
        surface.id: surface.definition.basis_surface
        for surface in context.brep.surfaces
        if isinstance(surface.definition, OffsetSurface)
    }
    for entity_kind, graph, sources in (
        ("curve", curve_basis, {item.id: item.source for item in context.brep.curves}),
        ("surface", surface_basis, {item.id: item.source for item in context.brep.surfaces}),
    ):
        for start in graph:
            seen: set[int] = set()
            current = start
            while current in graph:
                if current in seen:
                    result.append(
                        _diagnostic(
                            context,
                            code="occt.invalid_reference",
                            kind=DiagnosticKind.INVALID,
                            message=f"{entity_kind} basis reference cycle includes {current}",
                            source=sources.get(start),
                            details={
                                "entity_kind": entity_kind,
                                "entity_id": start,
                                "target_id": current,
                            },
                        )
                    )
                    break
                seen.add(current)
                current = graph[current]
    return result


def _unique_id_diagnostics(context: _ConversionContext) -> list[Diagnostic]:
    groups: tuple[tuple[str, Iterable[object]], ...] = (
        ("body", context.brep.bodies),
        ("region", context.brep.regions),
        ("shell", context.brep.shells),
        ("face", context.brep.faces),
        ("loop", context.brep.loops),
        ("half_edge", context.brep.half_edges),
        ("edge", context.brep.edges),
        ("vertex", context.brep.vertices),
        ("point", context.brep.points),
        ("curve", context.brep.curves),
        ("surface", context.brep.surfaces),
    )
    result: list[Diagnostic] = []
    for kind, values in groups:
        counts = Counter(item.id for item in values)
        duplicates = sorted(entity_id for entity_id, count in counts.items() if count > 1)
        for entity_id in duplicates:
            result.append(
                _diagnostic(
                    context,
                    code="occt.invalid_topology",
                    kind=DiagnosticKind.INVALID,
                    message=f"duplicate {kind} ID {entity_id}",
                    details={"entity_kind": kind, "entity_id": entity_id},
                )
            )
    return result


def _reference_diagnostics(context: _ConversionContext) -> list[Diagnostic]:
    """Check every relation the exact builder dereferences before importing OCP."""

    model = context.brep
    ids = {
        "body": {item.id for item in model.bodies},
        "region": {item.id for item in model.regions},
        "shell": {item.id for item in model.shells},
        "face": {item.id for item in model.faces},
        "loop": {item.id for item in model.loops},
        "half_edge": {item.id for item in model.half_edges},
        "edge": {item.id for item in model.edges},
        "vertex": {item.id for item in model.vertices},
        "point": {item.id for item in model.points},
        "curve": {item.id for item in model.curves},
        "surface": {item.id for item in model.surfaces},
    }
    missing: list[tuple[str, int, str, int, SourceNodeRef]] = []

    def check(
        owner_kind: str,
        owner_id: int,
        target_kind: str,
        target_id: int | None,
        source: SourceNodeRef,
        *,
        optional: bool = False,
    ) -> None:
        if target_id is None:
            if not optional:
                missing.append((owner_kind, owner_id, target_kind, -1, source))
            return
        if target_id not in ids[target_kind]:
            missing.append((owner_kind, owner_id, target_kind, target_id, source))

    for body in model.bodies:
        for region_id in body.regions:
            check("body", body.id, "region", region_id, body.source)
        for edge_id in body.edges:
            check("body", body.id, "edge", edge_id, body.source)
        for vertex_id in body.vertices:
            check("body", body.id, "vertex", vertex_id, body.source)
    for region in model.regions:
        check("region", region.id, "body", region.body, region.source)
        for shell_id in region.shells:
            check("region", region.id, "shell", shell_id, region.source)
    for shell in model.shells:
        check("shell", shell.id, "region", shell.region, shell.source)
        for face_id in (*shell.back_faces, *shell.front_faces):
            check("shell", shell.id, "face", face_id, shell.source)
    for face in model.faces:
        for loop_id in face.loops:
            check("face", face.id, "loop", loop_id, face.source)
        check("face", face.id, "surface", face.surface, face.source)
    for loop in model.loops:
        check("loop", loop.id, "face", loop.face, loop.source)
        for half_edge_id in loop.half_edges:
            check("loop", loop.id, "half_edge", half_edge_id, loop.source)
    for half_edge in model.half_edges:
        check(
            "half_edge",
            half_edge.id,
            "loop",
            half_edge.loop,
            half_edge.source,
            optional=half_edge.dummy,
        )
        check(
            "half_edge",
            half_edge.id,
            "edge",
            half_edge.edge,
            half_edge.source,
            optional=half_edge.dummy,
        )
        check(
            "half_edge",
            half_edge.id,
            "vertex",
            half_edge.vertex,
            half_edge.source,
            optional=True,
        )
        check(
            "half_edge",
            half_edge.id,
            "curve",
            half_edge.curve,
            half_edge.source,
            optional=True,
        )
    for edge in model.edges:
        for half_edge_id in edge.half_edges:
            check("edge", edge.id, "half_edge", half_edge_id, edge.source)
        check("edge", edge.id, "vertex", edge.start_vertex, edge.source, optional=True)
        check("edge", edge.id, "vertex", edge.end_vertex, edge.source, optional=True)
        check("edge", edge.id, "curve", edge.curve, edge.source, optional=True)
    for vertex in model.vertices:
        check("vertex", vertex.id, "point", vertex.point, vertex.source)
    for curve in model.curves:
        definition = curve.definition
        if isinstance(definition, TrimmedCurve):
            check("curve", curve.id, "curve", definition.basis_curve, curve.source)
        elif isinstance(definition, SurfaceParametricCurve):
            check("curve", curve.id, "surface", definition.surface, curve.source)
            check("curve", curve.id, "curve", definition.parameter_curve, curve.source)
            check(
                "curve",
                curve.id,
                "curve",
                definition.original_curve,
                curve.source,
                optional=True,
            )
    for surface in model.surfaces:
        definition = surface.definition
        if isinstance(definition, OffsetSurface):
            check("surface", surface.id, "surface", definition.basis_surface, surface.source)

    return [
        _diagnostic(
            context,
            code="occt.invalid_reference",
            kind=DiagnosticKind.INVALID,
            message=(f"{owner_kind} {owner_id} references missing {target_kind} {target_id}"),
            source=source,
            details={
                "entity_kind": owner_kind,
                "entity_id": owner_id,
                "target_kind": target_kind,
                "target_id": target_id,
            },
        )
        for owner_kind, owner_id, target_kind, target_id, source in missing
    ]


def _resolve_relations(
    registry: ShapeRegistry,
    pending: tuple[PendingRelation, ...],
) -> SourceShapeMap:
    unique: dict[tuple[str, str, str, str | None], ShapeRelation] = {}
    for item in pending:
        target_key = registry.key_for(item.target)
        relation = ShapeRelation(item.source, target_key, item.relation, item.note)
        key = (item.source.key, target_key, item.relation.value, item.note)
        unique[key] = relation
    return SourceShapeMap(
        tuple(
            unique[key]
            for key in sorted(
                unique,
                key=lambda value: (value[0], value[1], value[2], value[3] or ""),
            )
        )
    )


def _empty_output_counts() -> NamedCounts:
    return NamedCounts.from_dict(
        {
            "compounds": 0,
            "compsolids": 0,
            "edges": 0,
            "faces": 0,
            "shells": 0,
            "solids": 0,
            "vertices": 0,
            "wires": 0,
        }
    )


def _ocp_distribution() -> str | None:
    installed = installed_interop_distributions()
    for name in ("cadquery-ocp-novtk", "cadquery-ocp"):
        if name in installed:
            return f"{name}=={installed[name]}"
    return None


def _optional_text(value: object) -> str | None:
    return value if isinstance(value, str) and value else None


def _unknown_sense(
    context: _ConversionContext,
    entity_kind: str,
    entity_id: int,
    source: SourceNodeRef,
) -> Diagnostic:
    return _diagnostic(
        context,
        code="occt.unknown_orientation",
        kind=DiagnosticKind.INCOMPLETE,
        message=f"{entity_kind} {entity_id} has unknown orientation sense",
        source=source,
        details={"entity_kind": entity_kind, "entity_id": entity_id},
    )


def _diagnostic(
    context: _ConversionContext,
    *,
    code: str,
    kind: DiagnosticKind,
    message: str,
    source: SourceNodeRef | None = None,
    details: dict[str, str | int | float | bool | None] | None = None,
) -> Diagnostic:
    return Diagnostic(
        code=code,
        severity=DiagnosticSeverity.ERROR,
        kind=kind,
        message=message,
        location=(None if source is None else SourceLocation(byte_offset=source.byte_range.start)),
        node_type=None if source is None else source.node_type,
        node_id=(
            None
            if source is None or source.node_id is None or source.node_id < 0
            else source.node_id
        ),
        schema_key=context.brep.schema_key,
        fatal=True,
        details={} if details is None else details,
    )


def _raise_conversion(
    context: _ConversionContext,
    diagnostic: Diagnostic,
    *,
    diagnostics: Iterable[Diagnostic] = (),
) -> None:
    raise _conversion_error(context, diagnostic, diagnostics=diagnostics)


def _conversion_error(
    context: _ConversionContext,
    diagnostic: Diagnostic,
    *,
    diagnostics: Iterable[Diagnostic] = (),
) -> OcctConversionError:
    additions = list(diagnostics)
    if not additions:
        additions = [diagnostic]
    context.diagnostics.extend(additions)
    return OcctConversionError(diagnostic, report=context.report())


__all__ = ["to_occt"]
