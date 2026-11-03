"""Placement of assembly instances: transformed copies of the converted bodies."""

from __future__ import annotations

from dataclasses import dataclass

from ...brep.model import BrepModel
from ...brep.topology import InstancePartKind, Transform
from .model import ShapeRelationKind, SourceEntityKind, SourceEntityRef
from .topology import PendingRelation

MAX_INSTANCE_DEPTH = 64


@dataclass(frozen=True, slots=True)
class Placement:
    """The placed root shape and the relations rewritten onto the copies."""

    shape: object
    relations: tuple[PendingRelation, ...]
    instances_placed: int
    instances_negative: int
    bodies_unreferenced: int


def place_instances(
    brep: BrepModel,
    body_shapes: dict[int, object],
    relations: tuple[PendingRelation, ...],
    scale: float,
) -> Placement:
    """Place every body instance of the root assemblies as a transformed copy.

    The chain of instances from a root assembly (one no instance references)
    down to a body composes its transforms, each ``(rotation . x +
    translation) . scale`` with the translation in the conversion's target
    unit; the copy keeps the source relations of the body, rewritten onto its
    own subshapes and noted with the instance chain, and gains a ``generated``
    relation from the placing instance. A body no instance references stays
    where it is. Negative instances are placed like positive ones and counted.
    """
    from OCP.BRep import BRep_Builder
    from OCP.BRepBuilderAPI import BRepBuilderAPI_Transform
    from OCP.gp import gp_Trsf
    from OCP.TopExp import TopExp
    from OCP.TopoDS import TopoDS_Compound
    from OCP.TopTools import TopTools_IndexedMapOfShape

    assemblies = {item.id: item for item in brep.assemblies}
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

    placed: list[tuple[tuple[int, ...], int, object, bool]] = []

    def visit(assembly_id: int, parent: object, chain: tuple[int, ...], visited: frozenset) -> None:
        if assembly_id in visited or len(chain) > MAX_INSTANCE_DEPTH:
            raise ValueError(f"instance chain {chain} is circular or too deep")
        assembly = assemblies.get(assembly_id)
        if assembly is None:
            raise ValueError(f"assembly {assembly_id} is missing")
        for instance_id in assembly.instances:
            instance = instances.get(instance_id)
            if instance is None:
                raise ValueError(f"instance {instance_id} is missing")
            trsf = parent.Multiplied(_trsf(instance.transform, scale, instance_id))
            if instance.part_kind is InstancePartKind.BODY:
                placed.append(((*chain, instance_id), instance.part, trsf, instance.negative))
            else:
                visit(instance.part, trsf, (*chain, instance_id), visited | {assembly_id})

    for assembly in brep.assemblies:
        if not assembly.referencing_instances:
            visit(assembly.id, gp_Trsf(), (), frozenset())

    shapes: list[object] = []
    new_relations: list[PendingRelation] = []
    referenced: set[int] = set()
    negative = 0
    for chain, body_id, trsf, is_negative in placed:
        referenced.add(body_id)
        negative += int(is_negative)
        source = body_shapes.get(body_id)
        if source is None:
            # The body was not converted (a partial conversion dropped it).
            continue
        transformer = BRepBuilderAPI_Transform(source, trsf, True)
        copy = transformer.Shape()
        note = "instance " + "/".join(str(item) for item in chain)
        if is_negative:
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
    unreferenced = 0
    for body_id, shape in body_shapes.items():
        if body_id in referenced:
            continue
        unreferenced += 1
        shapes.append(shape)
        new_relations.extend(body_relations.get(body_id, ()))
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
    return Placement(root, tuple(new_relations), len(placed), negative, unreferenced)


def _trsf(transform: Transform | None, scale: float, instance_id: int) -> object:
    from OCP.gp import gp_Trsf

    trsf = gp_Trsf()
    if transform is None:
        return trsf
    s = transform.scale
    rows = transform.rotation
    t = transform.translation
    # OCCT accepts any matrix; only a rotation (or reflection) scaled uniformly
    # keeps the copied geometry exact, so the rows must be orthogonal and of
    # equal length.
    lengths = [sum(value * value for value in row) for row in rows]
    reference = max(lengths)
    if not reference > 0 or not all(
        abs(length - reference) <= 1e-9 * reference for length in lengths
    ):
        raise ValueError(
            f"transform of instance {instance_id} is not a rotation with a uniform scale"
        )
    for first in range(3):
        for second in range(first + 1, 3):
            dot = sum(a * b for a, b in zip(rows[first], rows[second], strict=True))
            if abs(dot) > 1e-9 * reference:
                raise ValueError(
                    f"transform of instance {instance_id} is not a rotation with a uniform scale"
                )
    try:
        trsf.SetValues(
            rows[0][0] * s,
            rows[0][1] * s,
            rows[0][2] * s,
            t.x * s * scale,
            rows[1][0] * s,
            rows[1][1] * s,
            rows[1][2] * s,
            t.y * s * scale,
            rows[2][0] * s,
            rows[2][1] * s,
            rows[2][2] * s,
            t.z * s * scale,
        )
    except Exception as error:  # OCCT raises Standard_ConstructionError without text
        raise ValueError(
            f"transform of instance {instance_id} is not a rotation with a uniform scale"
        ) from error
    return trsf
