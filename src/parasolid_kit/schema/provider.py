"""Explicit base-schema catalog boundary for complete document parsing."""

from __future__ import annotations

import os
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol, runtime_checkable

from ..limits import DEFAULT_PARSE_LIMITS, ParseLimits
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


class DirectorySchemaProvider:
    """Load exact caller-owned ``sch_<id>.sch_txt`` catalogs on demand.

    The directory and catalog files must not be symbolic links. Missing exact
    filenames return ``None`` as required by :class:`SchemaProvider`; no nearby
    version, alternate spelling, or recursively discovered file is considered.
    Successfully loaded catalogs are cached for the lifetime of the provider.
    """

    def __init__(
        self,
        directory: str | os.PathLike[str],
        *,
        limits: ParseLimits = DEFAULT_PARSE_LIMITS,
    ) -> None:
        if not isinstance(directory, (str, os.PathLike)):
            raise TypeError("directory must be a filesystem path")
        if not isinstance(limits, ParseLimits):
            raise TypeError("limits must be a ParseLimits value")
        requested = Path(directory).expanduser()
        if requested.is_symlink():
            raise ValueError(f"schema directory must not be a symbolic link: {requested}")
        resolved = requested.resolve()
        if not resolved.is_dir():
            raise NotADirectoryError(f"schema directory does not exist: {resolved}")
        self._directory = resolved
        self._limits = limits
        self._catalogs: dict[str, SchemaCatalog] = {}

    @property
    def directory(self) -> Path:
        """Return the absolute caller-owned catalog directory."""

        return self._directory

    def catalog_path(self, schema_id: str) -> Path:
        """Return the one exact path eligible for ``schema_id``."""

        _validate_schema_id(schema_id)
        return self._directory / f"sch_{schema_id}.sch_txt"

    def get_schema(self, schema_id: str) -> SchemaCatalog | None:
        """Load and cache one exact catalog, or return ``None`` when absent."""

        path = self.catalog_path(schema_id)
        cached = self._catalogs.get(schema_id)
        if cached is not None:
            return cached
        if path.is_symlink():
            raise ValueError(f"schema catalog must not be a symbolic link: {path}")
        if not path.exists():
            return None
        if not path.is_file():
            raise FileNotFoundError(f"schema catalog is not a regular file: {path}")

        # Local import avoids a provider/api module cycle: schema.api owns the
        # native catalog parser while schema.provider owns this lookup policy.
        from .api import load_schema_catalog

        catalog = load_schema_catalog(
            path,
            expected_schema_id=schema_id,
            limits=self._limits,
        )
        self._catalogs[schema_id] = catalog
        return catalog


def _validate_schema_id(schema_id: str) -> None:
    if (
        not isinstance(schema_id, str)
        or not schema_id
        or not schema_id.isascii()
        or not schema_id.isdigit()
    ):
        raise ValueError("schema_id must contain only ASCII digits")
