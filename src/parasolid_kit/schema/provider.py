"""Explicit base-schema catalog boundary for complete document parsing."""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass
from typing import Protocol, runtime_checkable

from .model import SchemaSource, TypeDefinition


@dataclass(frozen=True, slots=True)
class SchemaCatalog:
    """One caller-supplied, complete standard/base schema catalog."""

    schema_id: str
    definitions: tuple[TypeDefinition, ...]
    modeller_version: str | None = None
    declared_max_node_type: int | None = None
    declared_node_count: int | None = None
    declared_field_count: int | None = None
    declared_auxiliary_count: int | None = None
    source_path: str | None = None
    source_sha256: str | None = None

    def __post_init__(self) -> None:
        if not self.schema_id or not self.schema_id.isascii() or not self.schema_id.isdigit():
            raise ValueError("schema_id must contain only ASCII digits")
        if not isinstance(self.definitions, tuple):
            object.__setattr__(self, "definitions", tuple(self.definitions))
        if not all(isinstance(item, TypeDefinition) for item in self.definitions):
            raise ValueError("definitions must contain TypeDefinition values")
        node_types = [item.node_type for item in self.definitions]
        if len(set(node_types)) != len(node_types):
            raise ValueError("definitions must contain each node type at most once")
        if any(item.source is not SchemaSource.BASE for item in self.definitions):
            raise ValueError("catalog definitions must use SchemaSource.BASE")
        if self.modeller_version is not None and (
            not self.modeller_version
            or not self.modeller_version.isascii()
            or not self.modeller_version.isdigit()
        ):
            raise ValueError("modeller_version must contain only ASCII digits")
        for name in (
            "declared_max_node_type",
            "declared_node_count",
            "declared_field_count",
            "declared_auxiliary_count",
        ):
            value = getattr(self, name)
            if value is not None and (
                isinstance(value, bool) or not isinstance(value, int) or value < 0
            ):
                raise ValueError(f"{name} must be a non-negative integer or None")
        if self.declared_max_node_type is not None and self.declared_max_node_type > 65_535:
            raise ValueError("declared_max_node_type must fit an unsigned 16-bit field")
        if self.declared_node_count is not None and self.declared_node_count != len(
            self.definitions
        ):
            raise ValueError("declared_node_count must match definitions")
        observed_maximum = max(node_types, default=0)
        if (
            self.declared_max_node_type is not None
            and self.declared_max_node_type < observed_maximum
        ):
            raise ValueError("declared_max_node_type must cover every definition")
        if self.source_path is not None and not self.source_path:
            raise ValueError("source_path must not be empty")
        if self.source_sha256 is not None and (
            len(self.source_sha256) != 64
            or any(character not in "0123456789abcdef" for character in self.source_sha256)
        ):
            raise ValueError("source_sha256 must be a lowercase hexadecimal SHA-256 digest")
        object.__setattr__(
            self,
            "definitions",
            tuple(sorted(self.definitions, key=lambda item: item.node_type)),
        )

    def to_dict(self) -> dict[str, object]:
        """Return catalog metadata and effective definitions as JSON-compatible values."""

        return {
            "schema_id": self.schema_id,
            "modeller_version": self.modeller_version,
            "declared_max_node_type": self.declared_max_node_type,
            "declared_node_count": self.declared_node_count,
            "declared_field_count": self.declared_field_count,
            "declared_auxiliary_count": self.declared_auxiliary_count,
            "source_path": self.source_path,
            "source_sha256": self.source_sha256,
            "definitions": [definition.to_dict() for definition in self.definitions],
        }


@runtime_checkable
class SchemaProvider(Protocol):
    """Supply a complete catalog by the numeric schema identifier in an X_B key."""

    def get_schema(self, schema_id: str) -> SchemaCatalog | None:
        """Return the complete named catalog, or ``None`` when it is not loaded."""


class InMemorySchemaProvider:
    """Deterministic provider for tests and caller-managed local catalogs."""

    def __init__(self, catalogs: Iterable[SchemaCatalog] = ()) -> None:
        self._catalogs: dict[str, SchemaCatalog] = {}
        for catalog in catalogs:
            self.add(catalog)

    def add(self, catalog: SchemaCatalog) -> None:
        """Insert a catalog, rejecting accidental replacement."""

        if not isinstance(catalog, SchemaCatalog):
            raise TypeError("catalog must be a SchemaCatalog")
        if catalog.schema_id in self._catalogs:
            raise ValueError(f"schema catalog {catalog.schema_id!r} is already loaded")
        self._catalogs[catalog.schema_id] = catalog

    def get_schema(self, schema_id: str) -> SchemaCatalog | None:
        """Return one loaded catalog without guessing compatible versions."""

        return self._catalogs.get(schema_id)
