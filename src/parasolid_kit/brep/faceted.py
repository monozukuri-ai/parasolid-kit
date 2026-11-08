"""Opt-in access to stored polylines and embedded PSM mesh pools.

Names are interpreted only in explicit caller-supplied catalogs. Built-in
numeric profiles keep their existing, separately reviewed geometry contract.
"""

from __future__ import annotations

from dataclasses import dataclass
from math import isfinite

from ..binary.document import ParasolidDocument, RawField
from ..schema.model import FieldType
from .placement import Placement
from .topology import Sense, SourceNodeRef, Transform, Vector3


class FacetedGeometryError(ValueError):
    """Stored geometry has an unsupported schema, invalid data, or ambiguous indices."""


@dataclass(frozen=True, slots=True)
class Polyline:
    points: tuple[Vector3, ...]
    closed: bool
    sense: Sense
    base_parameter: float
    source: SourceNodeRef

    def display_points(self) -> tuple[Vector3, ...]:
        """Return connected points with source sense and explicit closure applied."""
        if self.sense is Sense.UNKNOWN or len(self.points) < 2:
            raise FacetedGeometryError(
                "polyline display needs at least two points and a known sense"
            )
        points = self.points[::-1] if self.sense is Sense.NEGATIVE else self.points
        if self.closed and points[-1] != points[0]:
            points = (*points, points[0])
        return points


@dataclass(frozen=True, slots=True)
class MeshPool:
    """A complete transmitted pool, including unused capacity and its encoding."""

    count: int
    capacity: int
    shift: int
    encoding: int
    values: tuple[int | float, ...]


@dataclass(frozen=True, slots=True)
class EmbeddedMesh:
    """Stored mesh data, without treating it as an editable or closed B-Rep."""

    source: SourceNodeRef
    sense: Sense
    positions: MeshPool | None
    position_indices: MeshPool | None
    normal_type: int | None
    normals: MeshPool | None
    normal_indices: MeshPool | None
    placement: Placement
    source_fields: tuple[RawField, ...]

    def inferred_index_origin(self) -> int:
        if self.positions is None or self.position_indices is None:
            raise FacetedGeometryError("mesh data is not embedded")
        indices = self.position_indices.values[: self.position_indices.count]
        candidates = [
            origin
            for origin in (0, 1)
            if all(origin <= i < origin + self.positions.count for i in indices)
        ]
        if len(candidates) != 1:
            raise FacetedGeometryError(
                "mesh index origin is ambiguous or out of range; select 0 or 1 explicitly"
            )
        return candidates[0]

    def triangles(self, *, index_origin: int | None = None) -> tuple[tuple[Vector3, ...], ...]:
        """Decode and place triangles. Reverse winding for source sense and reflection."""
        if self.positions is None or self.position_indices is None:
            raise FacetedGeometryError("mesh data is not embedded")
        if (
            self.positions.encoding != 1
            or self.positions.count <= 0
            or len(self.positions.values) < self.positions.count * 3
            or self.position_indices.count <= 0
            or self.position_indices.count % 3
            or len(self.position_indices.values) < self.position_indices.count
        ):
            raise FacetedGeometryError("mesh needs complete 3D positions and triangle indices")
        if self.sense is Sense.UNKNOWN:
            raise FacetedGeometryError("mesh surface sense is unknown")
        origin = self.inferred_index_origin() if index_origin is None else index_origin
        if type(origin) is not int or origin not in (0, 1):
            raise FacetedGeometryError("index_origin must be 0 or 1")
        indices = self.position_indices.values[: self.position_indices.count]
        if any(
            type(i) is not int or not origin <= i < origin + self.positions.count for i in indices
        ):
            raise FacetedGeometryError("mesh index is outside the selected position pool")
        result = []
        reverse = (self.sense is Sense.NEGATIVE) != (self.placement.determinant < 0)
        for start in range(0, len(indices), 3):
            points = []
            for index in indices[start : start + 3]:
                offset = (index - origin) * 3
                point = Vector3(*(float(v) for v in self.positions.values[offset : offset + 3]))
                points.append(self.placement.apply_point(point))
            if reverse:
                points[1], points[2] = points[2], points[1]
            result.append(tuple(points))
        return tuple(result)


@dataclass(frozen=True, slots=True)
class FacetedGeometry:
    """Selected stored records; this is not a completeness report for the document."""

    polylines: tuple[Polyline, ...]
    meshes: tuple[EmbeddedMesh, ...]
    schema_key: str
    source_format: str


def _source(node):
    return SourceNodeRef(node.index, node.node_type, node.type_name, None, node.byte_range)


class _Decoder:
    def __init__(self, document, max_pool_values):
        if not isinstance(document, ParasolidDocument):
            raise TypeError("document must be a parsed ParasolidDocument")
        if (
            document.schema_resolution is None
            or document.schema_resolution.kind != "caller_supplied"
        ):
            raise FacetedGeometryError(
                "stored geometry decoding requires an explicit named schema catalog"
            )
        if type(max_pool_values) is not int or not 1 <= max_pool_values <= 10_000_000:
            raise FacetedGeometryError("max_pool_values must be between 1 and 10000000")
        self.nodes = {node.index: node for node in document.nodes}
        self.maximum = max_pool_values
        self.used = 0

    def budget(self, count):
        self.used += count
        if self.used > self.maximum:
            raise FacetedGeometryError("stored geometry exceeds max_pool_values")

    def node(self, index, name):
        node = self.nodes.get(index)
        if node is None or node.type_name != name:
            raise FacetedGeometryError(f"node {index} does not resolve to {name}")
        return node

    def field(self, node, name, kind, *, scalar=False):
        found = [field for field in node.fields if field.definition.name == name]
        if (
            len(found) != 1
            or found[0].definition.field_type is not kind
            or not found[0].definition.transmitted
        ):
            raise FacetedGeometryError(
                f"node {node.index} has no unique transmitted {name} of type {kind.value}"
            )
        field = found[0]
        if scalar and (field.definition.element_count != 0 or len(field.values) != 1):
            raise FacetedGeometryError(f"node {node.index} field {name} is not scalar")
        values = tuple(value.value for value in field.values)
        return values[0] if scalar else values

    def integer(self, node, name, kind=FieldType.INTEGER):
        value = self.field(node, name, kind, scalar=True)
        if type(value) is not int or value < 0:
            raise FacetedGeometryError(
                f"node {node.index} field {name} must be a nonnegative integer"
            )
        return value

    def pointer(self, node, field, name, *, optional=False):
        index = self.integer(node, field, FieldType.POINTER_INDEX)
        if optional and index == 0:
            return None
        target = self.node(index, name)
        declaration = next(f.definition for f in node.fields if f.definition.name == field)
        if declaration.pointer_class < 1000 and declaration.pointer_class != target.node_type:
            raise FacetedGeometryError(
                f"node {node.index} field {field} pointer class does not match its target"
            )
        return target

    def sense(self, node):
        value = self.field(node, "sense", FieldType.CHARACTER, scalar=True)
        senses = {ord("+"): Sense.POSITIVE, ord("-"): Sense.NEGATIVE, ord("?"): Sense.UNKNOWN}
        if value not in senses:
            raise FacetedGeometryError("invalid stored geometry sense")
        return senses[value]

    def polyline(self, node):
        data = self.pointer(node, "data", "POLYLINE_DATA")
        points = self.pointer(data, "pvec", "POINT_VALUES")
        count = self.integer(data, "n_pvecs")
        values = self.field(points, "values", FieldType.VECTOR)
        self.budget(count * 3)
        if len(values) != count:
            raise FacetedGeometryError("polyline point count does not match its vector array")
        try:
            positions = tuple(Vector3(*value) for value in values)
        except (TypeError, ValueError) as error:
            raise FacetedGeometryError("polyline contains incomplete or invalid points") from error
        parameter = self.field(data, "base_parm", FieldType.DOUBLE, scalar=True)
        if parameter is None or not isfinite(parameter):
            raise FacetedGeometryError("polyline base parameter must be finite")
        closed = self.field(data, "closed", FieldType.LOGICAL, scalar=True)
        return Polyline(positions, closed, self.sense(node), parameter, _source(node))

    def pool(self, owner, link, *, vector):
        comb = self.pointer(owner, link, "VECTOR_COMB" if vector else "INTEGER_COMB")
        count_name = "vectors" if vector else "integers"
        count = self.integer(comb, f"n_{count_name}")
        capacity = self.integer(comb, f"n_max_{count_name}")
        shift = self.integer(comb, "shift")
        encoding = self.integer(comb, "encoding", FieldType.UNSIGNED_BYTE)
        if shift > 24 or count > capacity or encoding not in ((1, 2) if vector else (1,)):
            raise FacetedGeometryError("unsupported or invalid mesh pool encoding/count/shift")
        components = (3 if encoding == 1 else 2) if vector else 1
        if not vector and self.integer(comb, "n_bits_per_integer") != 32:
            raise FacetedGeometryError("only 32-bit mesh integer pools are supported")
        self.budget(capacity * components)
        tooth_size = 1 << shift
        indices = self.field(comb, "teeth", FieldType.POINTER_INDEX)
        if capacity % tooth_size or len(indices) != capacity // tooth_size:
            raise FacetedGeometryError("mesh pool capacity does not match its teeth")
        values = []
        kind = FieldType.DOUBLE if vector else FieldType.INTEGER
        for index in indices:
            tooth = self.node(index, "VECTOR_TOOTH" if vector else "INTEGER_TOOTH")
            data = self.field(tooth, "values", kind)
            if len(data) != tooth_size * components or any(
                v is None or not isfinite(v) for v in data
            ):
                raise FacetedGeometryError("mesh tooth length or values are invalid")
            values.extend(data)
        return MeshPool(count, capacity, shift, encoding, tuple(values))

    def placement(self, mesh):
        node = self.pointer(mesh, "transform", "TRANSFORM", optional=True)
        if node is None:
            return Placement()
        rotation = self.field(node, "rotation_matrix", FieldType.DOUBLE)
        translation = self.field(node, "translation_vector", FieldType.VECTOR, scalar=True)
        scale = self.field(node, "scale", FieldType.DOUBLE, scalar=True)
        perspective = self.field(node, "perspective_vector", FieldType.VECTOR, scalar=True)
        if len(rotation) != 9 or any(v is None for v in rotation) or scale is None:
            raise FacetedGeometryError("mesh transform has incomplete components")
        # Never discard a non-null higher-precision correction record.
        if (
            any(f.definition.name == "precision" for f in node.fields)
            and self.integer(node, "precision", FieldType.POINTER_INDEX) != 0
        ):
            raise FacetedGeometryError("mesh transform precision corrections are not yet supported")
        transform = Transform(
            tuple(tuple(rotation[i : i + 3]) for i in (0, 3, 6)),
            Vector3(*translation),
            scale,
            self.integer(node, "flag"),
            Vector3(*(0.0 if v is None else v for v in perspective)),
            _source(node),
        )
        return Placement.from_transform(transform)

    def mesh(self, node):
        sense, placement = self.sense(node), self.placement(node)
        psm = self.pointer(node, "psm_imesh", "PSM_MESH", optional=True)
        if psm is None:
            return EmbeddedMesh(
                _source(node), sense, None, None, None, None, None, placement, node.fields
            )
        if self.integer(psm, "owner", FieldType.POINTER_INDEX) != 0:
            raise FacetedGeometryError("reserved PSM_MESH owner must be null")
        if self.integer(psm, "precision", FieldType.UNSIGNED_BYTE) != 1:
            raise FacetedGeometryError("only double precision PSM_MESH is supported")
        positions = self.pool(psm, "position_pool", vector=True)
        indices = self.pool(psm, "position_indices", vector=False)
        if positions.encoding != 1 or not indices.count or indices.count % 3:
            raise FacetedGeometryError("mesh needs 3D positions and complete triangle indices")
        if any(i < 0 or i > positions.count for i in indices.values[: indices.count]):
            raise FacetedGeometryError("mesh position index is outside both possible origins")
        normal_type = self.integer(psm, "normal_type", FieldType.UNSIGNED_BYTE)
        normals = normal_indices = None
        if normal_type == 3:
            normals = self.pool(psm, "normal_pool", vector=True)
            normal_indices = self.pool(psm, "normal_indices", vector=False)
            if normal_indices.count != indices.count or any(
                i < 0 or i > normals.count for i in normal_indices.values[: normal_indices.count]
            ):
                raise FacetedGeometryError("mesh normal indices must parallel its triangle corners")
        elif normal_type != 1:
            raise FacetedGeometryError("unsupported mesh normal storage type")
        elif self.integer(psm, "normal_pool", FieldType.POINTER_INDEX) or self.integer(
            psm, "normal_indices", FieldType.POINTER_INDEX
        ):
            raise FacetedGeometryError("normal-none mesh has non-null normal links")
        return EmbeddedMesh(
            _source(node),
            sense,
            positions,
            indices,
            normal_type,
            normals,
            normal_indices,
            placement,
            node.fields,
        )


def decode_polyline(
    document: ParasolidDocument, node_index: int, *, max_pool_values=1_000_000
) -> Polyline:
    """Decode POLYLINE → POLYLINE_DATA → POINT_VALUES from an explicit catalog."""
    decoder = _Decoder(document, max_pool_values)
    return decoder.polyline(decoder.node(node_index, "POLYLINE"))


def decode_embedded_mesh(
    document: ParasolidDocument, node_index: int, *, max_pool_values=1_000_000
) -> EmbeddedMesh:
    """Retain MESH/PSM_MESH pools and placement; external associated files are not loaded."""
    decoder = _Decoder(document, max_pool_values)
    return decoder.mesh(decoder.node(node_index, "MESH"))


def faceted_geometry(document: ParasolidDocument, *, max_pool_values=1_000_000) -> FacetedGeometry:
    """Extract named stored records independently of B-Rep mapping and OCCT."""
    decoder = _Decoder(document, max_pool_values)
    polylines, meshes = [], []
    for node in decoder.nodes.values():
        if node.type_name == "POLYLINE":
            polylines.append(decoder.polyline(node))
        elif node.type_name == "MESH":
            meshes.append(decoder.mesh(node))
    return FacetedGeometry(
        tuple(polylines), tuple(meshes), document.schema_key.raw, document.format
    )
