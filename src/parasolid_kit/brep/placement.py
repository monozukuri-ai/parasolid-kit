"""Bounded assembly traversal and affine placement of existing source models."""

from __future__ import annotations

from dataclasses import dataclass
from math import isfinite

from ..diagnostics import DiagnosticKind
from ._errors import _OperationError
from .evaluation import _cross, _unit, _vector
from .model import BrepModel
from .topology import InstancePartKind, Transform, Vector3


class AssemblyPlacementError(_OperationError):
    """An assembly graph or stored transform cannot be placed unambiguously."""

    default_code = "assembly.invalid_graph"


class _AssemblyExpansionLimitError(AssemblyPlacementError):
    """Distinguish expansion ceilings from malformed assembly graphs."""


@dataclass(frozen=True, slots=True)
class Placement:
    """Row-major 3x4 affine matrix, with translation in source units."""

    matrix: tuple[tuple[float, float, float, float], ...] = (
        (1.0, 0.0, 0.0, 0.0),
        (0.0, 1.0, 0.0, 0.0),
        (0.0, 0.0, 1.0, 0.0),
    )

    def __post_init__(self):
        matrix = tuple(tuple(float(x) for x in row) for row in self.matrix)
        if len(matrix) != 3 or any(len(row) != 4 for row in matrix):
            raise AssemblyPlacementError(
                "placement must contain three rows of four values",
                code="assembly.invalid_matrix",
                field="matrix",
            )
        if not all(isfinite(x) for row in matrix for x in row):
            raise AssemblyPlacementError(
                "placement must be finite", code="assembly.invalid_matrix", field="matrix"
            )
        object.__setattr__(self, "matrix", matrix)
        if not isfinite(self.determinant) or self.determinant == 0:
            raise AssemblyPlacementError(
                "placement matrix is singular", code="assembly.singular_matrix", field="matrix"
            )

    @property
    def determinant(self) -> float:
        a, b, c = (row[:3] for row in self.matrix)
        return sum(x * y for x, y in zip(a, _cross(b, c), strict=True))

    def apply_point(self, point: Vector3) -> Vector3:
        return _vector(
            sum(a * b for a, b in zip(row[:3], point, strict=True)) + row[3] for row in self.matrix
        )

    def apply_normal(self, normal: Vector3) -> Vector3:
        """Apply inverse transpose, including nonuniform scale and reflection."""
        a, b, c = (row[:3] for row in self.matrix)
        cofactors = (_cross(b, c), _cross(c, a), _cross(a, b))
        result = tuple(
            sum(x * y for x, y in zip(row, normal, strict=True)) / self.determinant
            for row in cofactors
        )
        return _vector(_unit(result))

    def compose(self, local: Placement) -> Placement:
        """Return self(local(point)); outer assembly is applied last."""
        rows = []
        for outer in self.matrix:
            row = tuple(sum(outer[k] * local.matrix[k][j] for k in range(3)) for j in range(4))
            rows.append((*row[:3], row[3] + outer[3]))
        return Placement(tuple(rows))

    @classmethod
    def from_transform(cls, transform: Transform | None) -> Placement:
        if transform is None:
            return cls()
        if transform.flags < 0 or transform.flags & ~31 or any(transform.perspective):
            raise AssemblyPlacementError(
                "unsupported transform flags or perspective",
                code="assembly.unsupported_transform",
                kind=DiagnosticKind.UNSUPPORTED,
                source=transform.source,
                field="flags" if transform.flags < 0 or transform.flags & ~31 else "perspective",
            )
        if not isfinite(transform.scale) or transform.scale == 0:
            raise AssemblyPlacementError(
                "transform scale must be finite and nonzero",
                code="assembly.invalid_scale",
                source=transform.source,
                field="scale",
                details={"value": transform.scale},
            )
        try:
            return cls(
                tuple(
                    (*[v * transform.scale for v in row], t * transform.scale)
                    for row, t in zip(transform.rotation, transform.translation, strict=True)
                )
            )
        except AssemblyPlacementError as error:
            error.add_context(source=transform.source)
            raise


@dataclass(frozen=True, slots=True)
class BodyOccurrence:
    """One BODY occurrence; paths run from root assembly to leaf instance."""

    body_id: int
    instance_path: tuple[int, ...]
    assembly_path: tuple[int, ...]
    placement: Placement
    negative: bool = False


def _index(items, name):
    result = {}
    for item in items:
        if item.id in result:
            raise AssemblyPlacementError(
                f"duplicate {name} IDs",
                code="assembly.duplicate_id",
                source=item.source,
                field="id",
                details={"entity_kind": name, "entity_id": item.id},
            )
        result[item.id] = item
    return result


def assembly_occurrences(
    model: BrepModel, root_assembly: int | None = None, *, max_instances: int = 100_000
) -> tuple[BodyOccurrence, ...]:
    """Resolve bounded source placements; failures carry a structured diagnostic."""
    try:
        return _assembly_occurrences(model, root_assembly, max_instances=max_instances)
    except AssemblyPlacementError as error:
        error.add_context(schema_key=model.schema_key)
        raise


def _assembly_occurrences(
    model: BrepModel, root_assembly: int | None = None, *, max_instances: int = 100_000
) -> tuple[BodyOccurrence, ...]:
    """Resolve repeated/nested BODY placements without modifying source geometry.

    With no root selected, traverse every root and include uninstanced bodies
    once at identity. Negative instances are retained explicitly; displaying
    their Boolean meaning requires an operation beyond affine placement. Both
    expanded instance links and output body occurrences are bounded by
    max_instances. At most 64 assemblies may occur along one path.
    """
    if not isinstance(model, BrepModel):
        raise TypeError("model must be a BrepModel")
    if isinstance(max_instances, bool) or not isinstance(max_instances, int) or max_instances <= 0:
        raise AssemblyPlacementError(
            "max_instances must be a positive integer",
            code="assembly.invalid_parameter",
            field="max_instances",
            details={"value": max_instances},
        )
    bodies = _index(model.bodies, "body")
    assemblies = _index(model.assemblies, "assembly")
    instances = _index(model.instances, "instance")
    referenced = set()
    children = {}
    incoming = {assembly_id: set() for assembly_id in assemblies}
    for assembly in assemblies.values():
        children[assembly.id] = set(assembly.instances)
        if len(children[assembly.id]) != len(assembly.instances):
            raise AssemblyPlacementError(
                "assembly contains duplicate instance links",
                code="assembly.invalid_reference",
                source=assembly.source,
                field="instances",
                details={"entity_id": assembly.id},
            )
        for instance_id in assembly.instances:
            instance = instances.get(instance_id)
            if instance is None or instance.assembly != assembly.id:
                raise AssemblyPlacementError(
                    "assembly child link is missing or has a different owner",
                    code="assembly.invalid_reference",
                    source=assembly.source,
                    field="instances",
                    details={"entity_id": assembly.id, "reference_id": instance_id},
                )
    for instance in instances.values():
        owner = assemblies.get(instance.assembly)
        if owner is None or instance.id not in children[owner.id]:
            raise AssemblyPlacementError(
                "instance has no reciprocal owning assembly link",
                code="assembly.invalid_reference",
                source=instance.source,
                field="assembly",
                details={"entity_id": instance.id, "reference_id": instance.assembly},
            )
        if instance.part_kind is InstancePartKind.ASSEMBLY:
            parts = assemblies
        elif instance.part_kind is InstancePartKind.BODY:
            parts = bodies
        else:
            raise AssemblyPlacementError(
                "instance part kind is unsupported",
                code="assembly.unsupported_part",
                kind=DiagnosticKind.UNSUPPORTED,
                source=instance.source,
                field="part_kind",
            )
        if instance.part not in parts:
            raise AssemblyPlacementError(
                "instance part is missing",
                code="assembly.invalid_reference",
                source=instance.source,
                field="part",
                details={"entity_id": instance.id, "reference_id": instance.part},
            )
        if instance.part_kind is InstancePartKind.ASSEMBLY:
            referenced.add(instance.part)
            incoming[instance.part].add(instance.id)
    for assembly in assemblies.values():
        links = assembly.referencing_instances
        if len(set(links)) != len(links) or set(links) != incoming[assembly.id]:
            raise AssemblyPlacementError(
                "assembly has nonreciprocal referencing instance links",
                code="assembly.invalid_reference",
                source=assembly.source,
                field="referencing_instances",
                details={"entity_id": assembly.id},
            )
    if root_assembly is not None:
        if type(root_assembly) is not int or root_assembly not in assemblies:
            raise AssemblyPlacementError(
                "root assembly does not exist",
                code="assembly.invalid_root",
                field="root_assembly",
                details={"value": root_assembly},
            )
        roots = [root_assembly]
    else:
        roots = [key for key in assemblies if key not in referenced]
        if assemblies and not roots:
            raise AssemblyPlacementError(
                "assembly graph has no root (cycle)",
                code="assembly.cycle",
                field="instances",
                source=next(iter(assemblies.values())).source,
            )
    result = []
    reached = set()
    visits = 0
    stack = [(root, (), (), Placement(), False) for root in reversed(roots)]
    while stack:
        entry = stack.pop()
        if isinstance(entry, BodyOccurrence):
            result.append(entry)
            continue
        assembly_id, path, ancestors, placement, negative = entry
        if assembly_id in ancestors:
            raise AssemblyPlacementError(
                "assembly graph contains a cycle",
                code="assembly.cycle",
                source=assemblies[assembly_id].source,
                field="instances",
                details={"entity_id": assembly_id},
            )
        if len(ancestors) >= 64:
            raise AssemblyPlacementError(
                "assembly depth exceeds 64",
                code="assembly.depth_exceeded",
                kind=DiagnosticKind.LIMIT,
                source=assemblies[assembly_id].source,
                field="instances",
                details={"limit": 64, "observed": len(ancestors) + 1},
            )
        reached.add(assembly_id)
        chain = (*ancestors, assembly_id)
        # Reverse pushing preserves serialized child order, including repeated parts.
        children = []
        for instance_id in assemblies[assembly_id].instances:
            visits += 1
            if visits > max_instances:
                raise _AssemblyExpansionLimitError(
                    "assembly expansion exceeds max_instances",
                    code="assembly.limit_exceeded",
                    kind=DiagnosticKind.LIMIT,
                    source=assemblies[assembly_id].source,
                    field="instances",
                    details={
                        "resource": "max_instances",
                        "observed": visits,
                        "limit": max_instances,
                    },
                )
            instance = instances[instance_id]
            instance_path = (*path, instance_id)
            try:
                located = placement.compose(Placement.from_transform(instance.transform))
            except AssemblyPlacementError as error:
                error.add_context(
                    entity=instance,
                    instance_id=instance.id,
                    instance_path="/".join(map(str, instance_path)),
                )
                raise
            # Placement does not evaluate CSG: nested negative flags must never
            # be cancelled into an apparently ordinary positive solid.
            sign = negative or instance.negative
            if instance.part_kind is InstancePartKind.BODY:
                children.append(BodyOccurrence(instance.part, instance_path, chain, located, sign))
            else:
                children.append((instance.part, instance_path, chain, located, sign))
        # Emit leaf occurrences using the same DFS ordering as nested assemblies.
        for child in reversed(children):
            stack.append(child)
    if root_assembly is None:
        if reached != set(assemblies):
            raise AssemblyPlacementError(
                "assembly graph has an unreachable cycle",
                code="assembly.cycle",
                field="instances",
                source=next(
                    value for key, value in assemblies.items() if key not in reached
                ).source,
            )
        used_bodies = {
            instance.part
            for instance in instances.values()
            if instance.part_kind is InstancePartKind.BODY
        }
        for body_id in bodies:
            if body_id not in used_bodies:
                if len(result) >= max_instances:
                    raise _AssemblyExpansionLimitError(
                        "body occurrence count exceeds max_instances",
                        code="assembly.limit_exceeded",
                        kind=DiagnosticKind.LIMIT,
                        source=bodies[body_id].source,
                        details={
                            "resource": "max_instances",
                            "observed": len(result) + 1,
                            "limit": max_instances,
                        },
                    )
                result.append(BodyOccurrence(body_id, (), (), Placement()))
    return tuple(result)
