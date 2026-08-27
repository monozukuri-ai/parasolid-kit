from __future__ import annotations

import hashlib
from pathlib import Path

import pytest

from parasolid_kit import SchemaError, load_schema_catalog
from parasolid_kit.schema import FieldType, SchemaSource


def _catalog_bytes() -> bytes:
    return b"""**PARASOLID schema fixture
**END_OF_HEADER***************************************************
T
1
: SCHEMA FILE created by modeller version 3000119/30000;
3 2 3 17
1 NULLP; Null; 1 0 0
2 VALUES; Values; 1 3 1
hidden; d; 0 0 0
tag; t; 1 0 0
raw; q; 0 0 1
**************** end of schema SCH_3000119_30000 ****************
"""


def test_load_schema_catalog_returns_validated_metadata_and_effective_fields() -> None:
    source = _catalog_bytes()

    catalog = load_schema_catalog(source, expected_schema_id="30000")

    assert catalog.schema_id == "30000"
    assert catalog.modeller_version == "3000119"
    assert catalog.declared_max_node_type == 2
    assert catalog.declared_node_count == 2
    assert catalog.declared_field_count == 3
    assert catalog.declared_auxiliary_count == 17
    assert catalog.source_path is None
    assert catalog.source_sha256 == hashlib.sha256(source).hexdigest()
    assert [definition.node_type for definition in catalog.definitions] == [1, 2]
    assert [field.name for field in catalog.definitions[1].fields] == ["tag", "raw"]
    assert [field.field_type for field in catalog.definitions[1].fields] == [
        FieldType.TAG,
        FieldType.OPAQUE_POINTER,
    ]
    assert catalog.definitions[1].fields[1].transmitted is False
    assert catalog.definitions[1].source is SchemaSource.BASE
    assert catalog.to_dict()["source_sha256"] == catalog.source_sha256


def test_load_schema_catalog_accepts_path_and_records_absolute_provenance(
    tmp_path: Path,
) -> None:
    path = tmp_path / "sch_30000.sch_txt"
    path.write_bytes(_catalog_bytes())

    catalog = load_schema_catalog(path)

    assert catalog.source_path == str(path.resolve())


def test_load_schema_catalog_rejects_wrong_expected_id_and_invalid_counts() -> None:
    with pytest.raises(SchemaError) as mismatched:
        load_schema_catalog(_catalog_bytes(), expected_schema_id="13006")
    assert mismatched.value.diagnostic.code == "schema.catalog_id_mismatch"
    assert mismatched.value.diagnostic.details == {"expected": "13006", "actual": "30000"}

    malformed = _catalog_bytes().replace(b"3 2 3 17", b"3 2 4 17")
    with pytest.raises(SchemaError) as invalid:
        load_schema_catalog(malformed)
    assert invalid.value.diagnostic.code == "schema.field_count_mismatch"
    assert invalid.value.diagnostic.location is not None
    assert invalid.value.diagnostic.location.byte_offset > 0
