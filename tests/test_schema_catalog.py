from __future__ import annotations

import hashlib
from pathlib import Path

import pytest

from parasolid_kit import DirectorySchemaProvider, SchemaError, load_schema_catalog
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


def test_directory_schema_provider_loads_only_the_exact_catalog_and_caches_it(
    tmp_path: Path,
) -> None:
    path = tmp_path / "sch_30000.sch_txt"
    path.write_bytes(_catalog_bytes())
    provider = DirectorySchemaProvider(tmp_path)

    first = provider.get_schema("30000")
    assert first is not None
    assert first.source_path == str(path.resolve())
    assert provider.catalog_path("30000") == path.resolve()
    assert provider.get_schema("30001") is None

    path.write_bytes(b"the cached catalog must not be silently replaced")
    assert provider.get_schema("30000") is first


def test_directory_schema_provider_rejects_invalid_paths_ids_and_catalog_identity(
    tmp_path: Path,
) -> None:
    not_a_directory = tmp_path / "file"
    not_a_directory.write_bytes(b"not a directory")
    with pytest.raises(NotADirectoryError):
        DirectorySchemaProvider(not_a_directory)

    provider = DirectorySchemaProvider(tmp_path)
    for invalid in ("", "../30000", "3000é", "SCH_30000"):
        with pytest.raises(ValueError, match="ASCII digits"):
            provider.get_schema(invalid)

    mismatched = tmp_path / "sch_13006.sch_txt"
    mismatched.write_bytes(_catalog_bytes())
    with pytest.raises(SchemaError) as captured:
        provider.get_schema("13006")
    assert captured.value.diagnostic.code == "schema.catalog_id_mismatch"


def test_directory_schema_provider_rejects_directory_and_catalog_symlinks(
    tmp_path: Path,
) -> None:
    real_directory = tmp_path / "real"
    real_directory.mkdir()
    directory_link = tmp_path / "schemas"
    try:
        directory_link.symlink_to(real_directory, target_is_directory=True)
    except OSError as error:
        pytest.skip(f"symbolic links are unavailable: {error}")

    with pytest.raises(ValueError, match="symbolic link"):
        DirectorySchemaProvider(directory_link)

    target = real_directory / "target.sch_txt"
    target.write_bytes(_catalog_bytes())
    link = real_directory / "sch_30000.sch_txt"
    link.symlink_to(target)

    provider = DirectorySchemaProvider(real_directory)
    with pytest.raises(ValueError, match="symbolic link"):
        provider.get_schema("30000")
