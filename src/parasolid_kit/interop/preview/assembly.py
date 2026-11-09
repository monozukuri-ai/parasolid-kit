"""Attach explicit placements while retaining a single tessellation per source body."""

from ...brep.placement import AssemblyPlacementError, assembly_occurrences
from ...diagnostics import DiagnosticKind


def add_assembly_manifest(manifest, brep, converted, limits, root_assembly=None):
    if converted.report.placement_requested:
        raise AssemblyPlacementError(
            "preview assembly placement requires an unplaced OCCT conversion",
            code="assembly.already_placed",
            field="placement_requested",
        )
    occurrences = assembly_occurrences(brep, root_assembly, max_instances=limits.max_instances)
    primitives = manifest["primitives"]
    if any(len(p["body_ids"]) != 1 for p in primitives):
        raise AssemblyPlacementError(
            "assembly display requires unambiguous primitive body membership",
            code="assembly.ambiguous_body",
            field="body_ids",
        )
    rendered = {p["body_ids"][0] for p in primitives}
    scale = converted.report.options.applied_scale
    result = []
    instances = {instance.id: instance for instance in brep.instances}
    for occurrence in occurrences:
        if occurrence.negative:
            negative = next(
                instances[key] for key in occurrence.instance_path if instances[key].negative
            )
            raise AssemblyPlacementError(
                "negative assembly instances need Boolean interpretation before display",
                code="assembly.negative_instance",
                kind=DiagnosticKind.UNSUPPORTED,
                source=negative.source,
                field="negative",
                details={
                    "instance_id": negative.id,
                    "instance_path": "/".join(map(str, occurrence.instance_path)),
                },
            )
        if occurrence.body_id not in rendered:
            raise AssemblyPlacementError(
                "an assembly body has no display geometry",
                code="assembly.missing_geometry",
                field="body_id",
                details={"body_id": occurrence.body_id},
            )
        matrix = [[*row[:3], row[3] * scale] for row in occurrence.placement.matrix]
        result.append(
            {
                "body_id": occurrence.body_id,
                "instance_path": list(occurrence.instance_path),
                "assembly_path": list(occurrence.assembly_path),
                "matrix": matrix,
            }
        )
    if not result:
        raise AssemblyPlacementError(
            "assembly has no renderable body occurrences", code="assembly.missing_geometry"
        )
    return {**manifest, "occurrences": result}
