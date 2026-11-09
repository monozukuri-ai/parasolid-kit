"""Placement of assembly instances: transformed copies of the converted bodies."""

from __future__ import annotations

from dataclasses import dataclass
from math import isfinite

from ...brep.model import BrepModel
from ...brep.placement import AssemblyPlacementError, BodyOccurrence, assembly_occurrences
from ...brep.placement import Placement as AffinePlacement
from ...diagnostics import DiagnosticKind
from .model import ShapeRelationKind, SourceEntityKind, SourceEntityRef
from .topology import PendingRelation


@dataclass(frozen=True, slots=True)
class Placement:
    """The placed root shape and the relations rewritten onto the copies."""

    shape: object
    relations: tuple[PendingRelation, ...]
    instances_placed: int
    instances_negative: int
    bodies_unreferenced: int


def prepare_instances(brep: BrepModel, max_instances: int) -> tuple[BodyOccurrence, ...]:
    """Validate and expand once, before importing the optional OCCT runtime."""
    occurrences = assembly_occurrences(brep, max_instances=max_instances)
    for instance in brep.instances:
        try:
            _validate_similarity(AffinePlacement.from_transform(instance.transform), instance.id)
        except AssemblyPlacementError as error:
            error.add_context(
                source=instance.transform.source if instance.transform else instance.source,
                schema_key=brep.schema_key,
                instance_id=instance.id,
            )
            raise
    return occurrences


def place_instances(
    brep: BrepModel,
    body_shapes: dict[int, object],
    relations: tuple[PendingRelation, ...],
    scale: float,
    occurrences: tuple[BodyOccurrence, ...],
) -> Placement:
    """Place every body instance of the root assemblies as a transformed copy.

    The chain of instances from a root assembly (one no instance references)
    down to a body composes its transforms, each ``(rotation . x +
    translation) . scale`` with the translation in the conversion's target
    unit; the copy keeps the source relations of the body, rewritten onto its
    own subshapes and noted with the instance chain, and gains a ``generated``
    relation from the placing instance. A body no instance references stays
    where it is. A negative flag anywhere along a chain marks the resulting
    occurrence as negative; it is copied and counted, without a Boolean cut.
    """
    from OCP.BRep import BRep_Builder
    from OCP.BRepBuilderAPI import BRepBuilderAPI_Transform
    from OCP.TopExp import TopExp
    from OCP.TopoDS import TopoDS_Compound
    from OCP.TopTools import TopTools_IndexedMapOfShape

    instances = {item.id: item for item in brep.instances}
    indexed_bodies = {}
    for body_id, shape in body_shapes.items():
        indexed = TopTools_IndexedMapOfShape()
        TopExp.MapShapes_s(shape, indexed)
        indexed_bodies[body_id] = indexed
    body_relations: dict[int, list[PendingRelation]] = {}
    for relation in relations:
        for body_id, indexed in indexed_bodies.items():
            if indexed.Contains(relation.target):
                body_relations.setdefault(body_id, []).append(relation)
                break

    shapes: list[object] = []
    new_relations: list[PendingRelation] = []
    placed = negative = unreferenced = 0
    for occurrence in occurrences:
        body_id, chain = occurrence.body_id, occurrence.instance_path
        if chain:
            placed += 1
            negative += int(occurrence.negative)
        source = body_shapes.get(body_id)
        if source is None:
            # The body was not converted (a partial conversion dropped it).
            continue
        if not chain:
            unreferenced += 1
            shapes.append(source)
            new_relations.extend(body_relations.get(body_id, ()))
            continue
        try:
            trsf = _trsf(occurrence.placement, scale, chain[-1])
        except AssemblyPlacementError as error:
            error.add_context(
                entity=instances[chain[-1]],
                instance_path="/".join(map(str, chain)),
                schema_key=brep.schema_key,
            )
            raise
        transformer = BRepBuilderAPI_Transform(source, trsf, True)
        copy = transformer.Shape()
        note = "instance " + "/".join(str(item) for item in chain)
        if occurrence.negative:
            note += " (negative)"
        for relation in body_relations.get(body_id, ()):
            target = (
                copy
                if relation.target.IsSame(source)
                else transformer.ModifiedShape(relation.target)
            )
            new_relations.append(
                PendingRelation(
                    relation.source,
                    target,
                    relation.relation,
                    note if relation.note is None else f"{relation.note}; {note}",
                )
            )
        instance = instances[chain[-1]]
        new_relations.append(
            PendingRelation(
                SourceEntityRef(SourceEntityKind.INSTANCE, instance.id, instance.source),
                copy,
                ShapeRelationKind.GENERATED,
                note,
            )
        )
        shapes.append(copy)
    if not shapes:
        raise ValueError("no body is placed by an instance or stands on its own")
    if len(shapes) == 1:
        root = shapes[0]
    else:
        builder = BRep_Builder()
        compound = TopoDS_Compound()
        builder.MakeCompound(compound)
        for shape in shapes:
            builder.Add(compound, shape)
        root = compound
    return Placement(root, tuple(new_relations), placed, negative, unreferenced)


def _validate_similarity(placement: AffinePlacement, instance_id: int) -> None:
    rows = [row[:3] for row in placement.matrix]
    # OCCT accepts any matrix; only a rotation (or reflection) scaled uniformly
    # keeps the copied geometry exact, so the rows must be orthogonal and of
    # equal length.
    lengths = [sum(value * value for value in row) for row in rows]
    reference = max(lengths)
    if (
        not isfinite(reference)
        or not reference > 0
        or not all(abs(length - reference) <= 1e-9 * reference for length in lengths)
    ):
        raise AssemblyPlacementError(
            f"transform of instance {instance_id} is not a rotation with a uniform scale",
            code="assembly.unsupported_transform",
            kind=DiagnosticKind.UNSUPPORTED,
            field="matrix",
            details={"instance_id": instance_id},
        )
    for first in range(3):
        for second in range(first + 1, 3):
            dot = sum(a * b for a, b in zip(rows[first], rows[second], strict=True))
            if abs(dot) > 1e-9 * reference:
                raise AssemblyPlacementError(
                    f"transform of instance {instance_id} is not a rotation with a uniform scale",
                    code="assembly.unsupported_transform",
                    kind=DiagnosticKind.UNSUPPORTED,
                    field="matrix",
                    details={"instance_id": instance_id},
                )


def _trsf(placement: AffinePlacement, scale: float, instance_id: int) -> object:
    from OCP.gp import gp_Trsf

    _validate_similarity(placement, instance_id)
    trsf = gp_Trsf()
    try:
        trsf.SetValues(
            *(
                value * scale if column == 3 else value
                for row in placement.matrix
                for column, value in enumerate(row)
            )
        )
    except Exception as error:  # OCCT raises Standard_ConstructionError without text
        raise AssemblyPlacementError(
            f"transform of instance {instance_id} is not a rotation with a uniform scale",
            code="assembly.unsupported_transform",
            kind=DiagnosticKind.UNSUPPORTED,
            field="matrix",
            details={"instance_id": instance_id},
        ) from error
    return trsf
