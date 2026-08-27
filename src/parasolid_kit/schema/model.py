"""Typed Python views of Rust-resolved Parasolid schemas."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from ..binary.header import ByteRange


@dataclass(frozen=True, slots=True)
class SchemaKey:
    """Parsed components of a standard or embedded-base schema key."""

    raw: str
    modeller: str
    effective: str
    base: str | None

    def __post_init__(self) -> None:
        expected = [self.modeller, self.effective]
        if self.base is not None:
            expected.append(self.base)
        if not all(
            component and component.isascii() and component.isdigit() for component in expected
        ) or self.raw != "SCH_" + "_".join(expected):
            raise ValueError("schema key components do not match a valid SCH_... key")

    @classmethod
    def parse(cls, raw: str) -> SchemaKey:
        """Parse a header key already validated by the Rust header reader."""

        if not isinstance(raw, str) or not raw.startswith("SCH_"):
            raise ValueError("schema key must start with 'SCH_'")
        components = raw[4:].split("_")
        if len(components) not in (2, 3):
            raise ValueError("schema key must contain two or three numeric components")
        return cls(
            raw=raw,
            modeller=components[0],
            effective=components[1],
            base=None if len(components) == 2 else components[2],
        )

    @property
    def provider_schema(self) -> str:
        """Return the catalog identifier required to parse node bodies."""

        return self.base if self.base is not None else self.effective

    def to_dict(self) -> dict[str, str | None]:
        """Return a JSON-compatible representation."""

        return {
            "raw": self.raw,
            "modeller": self.modeller,
            "effective": self.effective,
            "base": self.base,
            "provider_schema": self.provider_schema,
        }


class FieldType(str, Enum):
    """Scalar codec code stored in an effective schema field."""

    UNSIGNED_BYTE = "u"
    CHARACTER = "c"
    LOGICAL = "l"
    SHORT_INTEGER = "n"
    UNICODE_CHARACTER = "w"
    INTEGER = "d"
    POINTER_INDEX = "p"
    OPAQUE_POINTER = "q"
    TAG = "t"
    DOUBLE = "f"
    INTERVAL = "i"
    VECTOR = "v"
    BOX3 = "b"
    INTERSECTION_POINT = "h"


class SchemaSource(str, Enum):
    """Provenance of one effective type definition."""

    BASE = "base"
    EMBEDDED_UNCHANGED = "embedded_unchanged"
    EMBEDDED_DELTA = "embedded_delta"
    EMBEDDED_FULL = "embedded_full"


class SchemaEditKind(str, Enum):
    """Opcode in an embedded delta schema."""

    COPY = "C"
    DELETE = "D"
    INSERT = "I"
    APPEND = "A"
    END = "Z"


@dataclass(frozen=True, slots=True)
class FieldDefinition:
    """One effective transmitted field."""

    name: str
    field_type: FieldType
    pointer_class: int
    element_count: int
    transmitted: bool

    def __post_init__(self) -> None:
        if not self.name:
            raise ValueError("field name must not be empty")
        if not isinstance(self.field_type, FieldType):
            object.__setattr__(self, "field_type", FieldType(self.field_type))
        if not 0 <= self.pointer_class <= 65_535:
            raise ValueError("pointer_class must fit an unsigned 16-bit field")
        if not 0 <= self.element_count <= 0xFFFF_FFFF:
            raise ValueError("element_count must fit an unsigned 32-bit field")
        if not isinstance(self.transmitted, bool):
            raise ValueError("transmitted must be a boolean")
        if self.pointer_class and self.field_type is not FieldType.POINTER_INDEX:
            raise ValueError("non-zero pointer_class requires POINTER_INDEX field type")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        return {
            "name": self.name,
            "field_type": self.field_type.value,
            "pointer_class": self.pointer_class,
            "element_count": self.element_count,
            "transmitted": self.transmitted,
        }


@dataclass(frozen=True, slots=True)
class TypeDefinition:
    """Effective field sequence for one node type."""

    node_type: int
    name: str
    description: str
    variable: bool
    fields: tuple[FieldDefinition, ...]
    source: SchemaSource

    def __post_init__(self) -> None:
        if not 0 <= self.node_type <= 65_535:
            raise ValueError("node_type must fit an unsigned 16-bit field")
        if not self.name:
            raise ValueError("type name must not be empty")
        if not isinstance(self.variable, bool):
            raise ValueError("variable must be a boolean")
        if not isinstance(self.fields, tuple):
            object.__setattr__(self, "fields", tuple(self.fields))
        if not all(isinstance(field, FieldDefinition) for field in self.fields):
            raise ValueError("fields must contain FieldDefinition values")
        expected_variable = bool(self.fields and self.fields[-1].element_count == 1)
        if self.variable is not expected_variable:
            raise ValueError("variable must match the final field element count")
        if not isinstance(self.source, SchemaSource):
            object.__setattr__(self, "source", SchemaSource(self.source))

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        return {
            "node_type": self.node_type,
            "name": self.name,
            "description": self.description,
            "variable": self.variable,
            "fields": [field.to_dict() for field in self.fields],
            "source": self.source.value,
        }


@dataclass(frozen=True, slots=True)
class SchemaEdit:
    """One decoded delta instruction at an absolute blob offset."""

    kind: SchemaEditKind
    byte_offset: int
    field: FieldDefinition | None = None

    def __post_init__(self) -> None:
        if not isinstance(self.kind, SchemaEditKind):
            object.__setattr__(self, "kind", SchemaEditKind(self.kind))
        if self.byte_offset < 0:
            raise ValueError("byte_offset must be non-negative")
        carries_field = self.kind in (SchemaEditKind.INSERT, SchemaEditKind.APPEND)
        if carries_field != isinstance(self.field, FieldDefinition):
            raise ValueError("only insert and append edits must carry a field")

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        result: dict[str, object] = {
            "kind": self.kind.value,
            "byte_offset": self.byte_offset,
        }
        if self.field is not None:
            result["field"] = self.field.to_dict()
        return result


@dataclass(frozen=True, slots=True)
class SchemaResolution:
    """Exact embedded bytes and the effective definition they produce."""

    definition: TypeDefinition
    raw_schema: bytes
    byte_range: ByteRange
    edits: tuple[SchemaEdit, ...]

    def __post_init__(self) -> None:
        if not isinstance(self.definition, TypeDefinition):
            raise ValueError("definition must be a TypeDefinition")
        if not isinstance(self.raw_schema, bytes):
            object.__setattr__(self, "raw_schema", bytes(self.raw_schema))
        if self.byte_range.length != len(self.raw_schema):
            raise ValueError("byte_range length must match raw_schema")
        if not isinstance(self.edits, tuple):
            object.__setattr__(self, "edits", tuple(self.edits))
        if not all(isinstance(edit, SchemaEdit) for edit in self.edits):
            raise ValueError("edits must contain SchemaEdit values")

    @property
    def consumed(self) -> int:
        """Return the number of consumed embedded-schema bytes."""

        return self.byte_range.length

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible report without embedding binary bytes."""

        return {
            "definition": self.definition.to_dict(),
            "raw_schema_hex": self.raw_schema.hex(),
            "byte_range": self.byte_range.to_dict(),
            "edits": [edit.to_dict() for edit in self.edits],
        }


@dataclass(frozen=True, slots=True)
class SchemaCoverageReport:
    """Resolved type and field counts grouped by provenance."""

    node_types: tuple[int, ...]
    field_count: int
    base_count: int
    unchanged_count: int
    delta_count: int
    full_count: int

    def __post_init__(self) -> None:
        if not isinstance(self.node_types, tuple):
            object.__setattr__(self, "node_types", tuple(self.node_types))
        if tuple(sorted(set(self.node_types))) != self.node_types:
            raise ValueError("node_types must be sorted and unique")
        for name in (
            "field_count",
            "base_count",
            "unchanged_count",
            "delta_count",
            "full_count",
        ):
            value = getattr(self, name)
            if value < 0:
                raise ValueError(f"{name} must be non-negative")

    @property
    def resolved_type_count(self) -> int:
        """Return the number of unique resolved node types."""

        return len(self.node_types)

    def to_dict(self) -> dict[str, object]:
        """Return a JSON-compatible representation."""

        return {
            "node_types": list(self.node_types),
            "resolved_type_count": self.resolved_type_count,
            "field_count": self.field_count,
            "base_count": self.base_count,
            "unchanged_count": self.unchanged_count,
            "delta_count": self.delta_count,
            "full_count": self.full_count,
        }
