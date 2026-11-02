"""Immutable Python views of Rust-framed neutral X_B documents."""

from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from typing import Literal, TypeAlias

from ..diagnostics import Diagnostic
from ..schema.model import (
    FieldDefinition,
    FieldType,
    SchemaCoverageReport,
    SchemaKey,
    SchemaProviderResolution,
    SchemaResolution,
    TypeDefinition,
)
from ..text.header import XtHeader, XtTermination
from .header import ByteRange, XbHeader

CompositeValue: TypeAlias = tuple[float | None, ...]
ScalarValue: TypeAlias = int | float | bool | CompositeValue | None


@dataclass(frozen=True, slots=True)
class FieldValue:
    """One scalar or fixed composite decoded by a Rust field codec."""

    field_type: FieldType
    value: ScalarValue

    def __post_init__(self) -> None:
        if not isinstance(self.field_type, FieldType):
            object.__setattr__(self, "field_type", FieldType(self.field_type))
        if self.field_type in {
            FieldType.INTERVAL,
            FieldType.VECTOR,
            FieldType.BOX3,
            FieldType.INTERSECTION_POINT,
        }:
            if isinstance(self.value, list):
                object.__setattr__(self, "value", tuple(self.value))
            expected = {
                FieldType.INTERVAL: 2,
                FieldType.VECTOR: 3,
                FieldType.BOX3: 6,
                FieldType.INTERSECTION_POINT: 3,
            }[self.field_type]
            if (
                not isinstance(self.value, tuple)
                or len(self.value) != expected
                or any(
                    item is not None and (isinstance(item, bool) or not isinstance(item, float))
                    for item in self.value
                )
            ):
                raise ValueError("composite field value has an invalid shape or component")
            return
        if isinstance(self.value, tuple):
            raise ValueError("only composite field types may contain tuple values")
        if self.field_type is FieldType.OPAQUE_POINTER:
            raise ValueError("opaque pointer fields have no public neutral value")
        if self.field_type is FieldType.LOGICAL:
            if not isinstance(self.value, bool):
                raise ValueError("logical field value must be a boolean")
            return
        if (
            self.field_type
            in {FieldType.SHORT_INTEGER, FieldType.INTEGER, FieldType.TAG, FieldType.DOUBLE}
            and self.value is None
        ):
            return
        if self.field_type is FieldType.DOUBLE:
            if isinstance(self.value, bool) or not isinstance(self.value, float):
                raise ValueError("double field value must be a float or None")
            return
        if isinstance(self.value, bool) or not isinstance(self.value, int):
            raise ValueError("integral field value must be an integer")
        bounds = {
            FieldType.UNSIGNED_BYTE: (0, 255),
            FieldType.CHARACTER: (0, 255),
            FieldType.SHORT_INTEGER: (-32_768, 32_767),
            FieldType.UNICODE_CHARACTER: (0, 65_535),
            FieldType.INTEGER: (-2_147_483_648, 2_147_483_647),
            FieldType.POINTER_INDEX: (0, 1_073_709_055),
            FieldType.TAG: (-2_147_483_648, 2_147_483_647),
        }
        minimum, maximum = bounds[self.field_type]
        if not minimum <= self.value <= maximum:
            raise ValueError("integral field value lies outside its physical range")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        value: object = list(self.value) if isinstance(self.value, tuple) else self.value
        return {"field_type": self.field_type.value, "value": value}


@dataclass(frozen=True, slots=True)
class RawField:
    """One decoded effective field and its exact transmitted byte range."""

    definition: FieldDefinition
    values: tuple[FieldValue, ...]
    byte_range: ByteRange

    def __post_init__(self) -> None:
        if not isinstance(self.definition, FieldDefinition):
            raise ValueError("definition must be a FieldDefinition")
        if not isinstance(self.values, tuple):
            object.__setattr__(self, "values", tuple(self.values))
        if not all(isinstance(value, FieldValue) for value in self.values):
            raise ValueError("values must contain FieldValue values")
        if any(value.field_type is not self.definition.field_type for value in self.values):
            raise ValueError("field values must match the definition field type")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        return {
            "definition": self.definition.to_dict(),
            "values": [value.to_dict() for value in self.values],
            "byte_range": self.byte_range.to_dict(),
        }


@dataclass(frozen=True, slots=True)
class RawNode:
    """One non-termination node in physical source order."""

    node_type: int
    index: int
    variable_length: int | None
    definition: TypeDefinition
    first_schema: SchemaResolution | None
    fields: tuple[RawField, ...]
    byte_range: ByteRange
    user_fields: tuple[int | None, ...] = ()

    def __post_init__(self) -> None:
        if not 2 <= self.node_type <= 32_767:
            raise ValueError("node_type must fit the positive non-termination short range")
        if self.index <= 0:
            raise ValueError("node index must be greater than zero")
        if self.variable_length is not None and self.variable_length < 0:
            raise ValueError("variable_length must be non-negative when present")
        if self.definition.node_type != self.node_type:
            raise ValueError("definition node type must match the node")
        if self.definition.variable != (self.variable_length is not None):
            raise ValueError("variable_length presence must match the definition")
        if self.first_schema is not None and self.first_schema.definition != self.definition:
            raise ValueError("first_schema definition must match the node definition")
        if not isinstance(self.fields, tuple):
            object.__setattr__(self, "fields", tuple(self.fields))
        if not all(isinstance(item, RawField) for item in self.fields):
            raise ValueError("fields must contain RawField values")
        if tuple(item.definition for item in self.fields) != self.definition.fields:
            raise ValueError("raw fields must follow the complete effective definition")
        if not isinstance(self.user_fields, tuple):
            object.__setattr__(self, "user_fields", tuple(self.user_fields))
        if len(self.user_fields) > 16 or any(
            word is not None and (type(word) is not int or not -(2**31) <= word < 2**31)
            for word in self.user_fields
        ):
            raise ValueError("user_fields must contain at most 16 signed integer words or None")

    @property
    def type_name(self) -> str:
        """Return the effective schema name."""

        return self.definition.name

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        return {
            "node_type": self.node_type,
            "type_name": self.type_name,
            "user_fields": list(self.user_fields),
            "index": self.index,
            "variable_length": self.variable_length,
            "definition": self.definition.to_dict(),
            "first_schema": (None if self.first_schema is None else self.first_schema.to_dict()),
            "fields": [item.to_dict() for item in self.fields],
            "byte_range": self.byte_range.to_dict(),
        }


@dataclass(frozen=True, slots=True)
class XbTermination:
    """Validated node-type-1 marker followed by compact index zero."""

    index: int
    byte_range: ByteRange

    def __post_init__(self) -> None:
        if self.index != 0:
            raise ValueError("termination index must be zero")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        return {"index": self.index, "byte_range": self.byte_range.to_dict()}


@dataclass(frozen=True, slots=True)
class ParasolidDocument:
    """Complete raw X_T/X_B document backed by an immutable native parse snapshot.

    ``nodes`` are materialized from the native snapshot on first access and then
    kept, so a document whose nodes are never read builds no Python node tree;
    ``node_count`` is known without materializing them.
    """

    format: Literal["binary", "text"]
    header: XbHeader | XtHeader
    schema_key: SchemaKey
    schemas: tuple[SchemaResolution, ...]
    terminator: XbTermination | XtTermination
    schema_coverage: SchemaCoverageReport
    raw_bytes: bytes
    node_count: int
    _native_document: object = field(repr=False, compare=False)
    diagnostics: tuple[Diagnostic, ...] = ()
    _node_loader: Callable[[], Sequence[RawNode]] | None = field(
        default=None, repr=False, compare=False
    )
    _nodes: tuple[RawNode, ...] | None = field(default=None, repr=False, compare=False)
    schema_resolution: SchemaProviderResolution | None = None

    def __post_init__(self) -> None:
        if self.schema_resolution is not None:
            if not isinstance(self.schema_resolution, SchemaProviderResolution):
                raise TypeError("schema_resolution must be a SchemaProviderResolution or None")
            if (
                self.schema_resolution.kind == "builtin"
                and self.schema_resolution.schema_key != self.schema_key.raw
            ):
                raise ValueError("built-in provenance key must match the document")
        if self.format not in {"binary", "text"}:
            raise ValueError("document format must be 'binary' or 'text'")
        expected_header = XbHeader if self.format == "binary" else XtHeader
        expected_termination = XbTermination if self.format == "binary" else XtTermination
        if not isinstance(self.header, expected_header):
            raise ValueError("document header does not match its source format")
        if not isinstance(self.terminator, expected_termination):
            raise ValueError("document terminator does not match its source format")
        if self.header.schema_key != self.schema_key.raw:
            raise ValueError("header and parsed schema key must match")
        if not isinstance(self.schemas, tuple):
            object.__setattr__(self, "schemas", tuple(self.schemas))
        if not all(isinstance(item, SchemaResolution) for item in self.schemas):
            raise ValueError("schemas must contain SchemaResolution values")
        schema_types = tuple(item.definition.node_type for item in self.schemas)
        if schema_types != self.schema_coverage.node_types:
            raise ValueError("schemas must match schema_coverage node types")
        if (
            isinstance(self.node_count, bool)
            or not isinstance(self.node_count, int)
            or self.node_count < 0
        ):
            raise ValueError("node_count must be a non-negative integer")
        if self._nodes is None:
            if self._node_loader is None:
                raise ValueError("nodes or a node loader must be present")
        else:
            object.__setattr__(self, "_nodes", self._validated_nodes(self._nodes))
        if not isinstance(self.raw_bytes, bytes):
            object.__setattr__(self, "raw_bytes", bytes(self.raw_bytes))
        if len(self.raw_bytes) != self.header.file_size:
            raise ValueError("raw_bytes length must match the inspected file size")
        if self.terminator.byte_range.end != len(self.raw_bytes):
            raise ValueError("terminator must end at the final source byte")
        if self._native_document is None:
            raise ValueError("native document handle must be present")
        if not isinstance(self.diagnostics, tuple):
            object.__setattr__(self, "diagnostics", tuple(self.diagnostics))
        if not all(isinstance(item, Diagnostic) for item in self.diagnostics):
            raise ValueError("diagnostics must contain Diagnostic values")

    @property
    def nodes(self) -> tuple[RawNode, ...]:
        """Return the nodes in physical source order, materializing them once."""

        nodes = self._nodes
        if nodes is None:
            loader = self._node_loader
            if loader is None:
                raise ValueError("nodes or a node loader must be present")
            nodes = self._validated_nodes(loader())
            object.__setattr__(self, "_nodes", nodes)
        return nodes

    def _validated_nodes(self, nodes: Sequence[RawNode]) -> tuple[RawNode, ...]:
        nodes = tuple(nodes)
        if not all(isinstance(item, RawNode) for item in nodes):
            raise ValueError("nodes must contain RawNode values")
        if len(nodes) != self.node_count:
            raise ValueError("nodes must match the parsed node count")
        indices = tuple(item.index for item in nodes)
        if len(indices) != len(set(indices)):
            raise ValueError("nodes must have unique non-zero indices")
        return nodes

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible report without duplicating the full source bytes."""

        return {
            "format": self.format,
            "header": self.header.to_dict(),
            "schema_key": self.schema_key.to_dict(),
            "schemas": [schema.to_dict() for schema in self.schemas],
            "nodes": [node.to_dict() for node in self.nodes],
            "terminator": self.terminator.to_dict(),
            "schema_coverage": self.schema_coverage.to_dict(),
            "schema_resolution": None
            if self.schema_resolution is None
            else self.schema_resolution.to_dict(),
            "raw_byte_count": len(self.raw_bytes),
            "diagnostics": [diagnostic.to_dict() for diagnostic in self.diagnostics],
        }
