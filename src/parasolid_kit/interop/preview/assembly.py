"""Attach explicit placements while retaining a single tessellation per source body."""

from ...brep.placement import AssemblyPlacementError, assembly_occurrences


def add_assembly_manifest(manifest, brep, converted, limits, root_assembly=None):
    if converted.report.placement_requested:
        raise AssemblyPlacementError(
            "preview assembly placement requires an unplaced OCCT conversion"
        )
    occurrences = assembly_occurrences(brep, root_assembly, max_instances=limits.max_entities)
    primitives = manifest["primitives"]
    if any(len(p["body_ids"]) != 1 for p in primitives):
        raise AssemblyPlacementError(
            "assembly display requires unambiguous primitive body membership"
        )
    rendered = {p["body_ids"][0] for p in primitives}
    scale = converted.report.options.applied_scale
    result = []
    for occurrence in occurrences:
        if occurrence.negative:
            raise AssemblyPlacementError(
                "negative assembly instances need Boolean interpretation before display"
            )
        if occurrence.body_id not in rendered:
            raise AssemblyPlacementError("an assembly body has no display geometry")
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
        raise AssemblyPlacementError("assembly has no renderable body occurrences")
    return {**manifest, "occurrences": result}
