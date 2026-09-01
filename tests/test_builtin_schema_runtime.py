"""Synthetic selection contracts; geometry evidence lives in the local M8 corpus."""

from __future__ import annotations

import json
import struct
from dataclasses import FrozenInstanceError, fields, replace

import pytest

from parasolid_kit import (
    DEFAULT_PARSE_LIMITS,
    InMemorySchemaProvider,
    ParasolidDocument,
    ParseError,
    SchemaCatalog,
    SchemaError,
    SchemaProviderResolution,
    _core,
    compare_documents,
    parse_xb,
    parse_xt,
    read_brep,
    write_xb,
)
from parasolid_kit.cli import main
from parasolid_kit.schema import FieldDefinition, FieldType, SchemaSource, TypeDefinition
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header

KEY = "SCH_3000000_30000"
PROVENANCE = {
    "kind": "builtin",
    "profile_id": "onshape-sch30000-r1",
    "profile_revision": 1,
    "schema_key": KEY,
    "coverage": "verified_subset",
    "profile_sha256": "e28a5e11a7713573a7134025bd8c3d83f194fc663662f079e839a95ea5981f80",
}


def payload(
    encoding: str, *, key: str = KEY, node_type: int | None = 82, user_fields: int = 0
) -> bytes:
    embedded = len(key.split("_")) == 4
    if encoding == "x_t":
        header = text_header(
            key,
            schema_max_type=205 if embedded else None,
            common_header=True,
            user_field_size=user_fields,
        )
        # Type 82: a variable array of two integers, including an unset value.
        return header + (b"" if node_type is None else f"{node_type} 2 1 7 ?".encode()) + b"1 0 "
    builder = SyntheticXbBuilder(
        schema_name=key,
        schema_max_type=205 if embedded else None,
        user_field_size=user_fields,
    )
    if node_type is not None:
        builder.add_raw_node(
            node_type, struct.pack(">i", 2) + positive_integer(1) + struct.pack(">ii", 7, -32764)
        )
    return builder.build()


def general_body_payload(encoding: str) -> bytes:
    """Synthetic empty general BODY for plumbing, not solid-geometry coverage."""
    values = [0] * 33
    values[9], values[10], values[16] = 1e-6, 1e-8, 6
    if encoding == "x_t":
        return text_header() + b"12 1 " + "".join(f"{v} " for v in values).encode() + b"1 0 "
    record = positive_integer(1)
    for ordinal, value in enumerate(values):
        if ordinal in (0, 27, 32):
            record += struct.pack(">i", value)
        elif ordinal in (9, 10):
            record += struct.pack(">d", value)
        elif ordinal in (14, 16, 17):
            record += bytes([value])
        else:
            record += positive_integer(value)
    return (
        SyntheticXbBuilder(schema_name=KEY, schema_max_type=None).add_raw_node(12, record).build()
    )


def parser(encoding: str):
    return parse_xt if encoding == "x_t" else parse_xb


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_default_and_none_select_exact_internal_key_with_provenance(encoding):
    data = payload(encoding)
    document = parser(encoding)(data)
    assert document == parser(encoding)(data, schema_provider=None)
    assert document.schema_resolution.to_dict() == PROVENANCE
    assert document.to_dict()["schema_resolution"] == PROVENANCE
    assert document.nodes[0].type_name == "onshape_type_82"
    assert [v.value for v in document.nodes[0].fields[0].values] == [7, None]
    assert document.schemas[0].definition.source is SchemaSource.BASE
    if encoding == "x_b":
        assert write_xb(document) == data
    assert fields(ParasolidDocument)[-1].name == "schema_resolution"
    assert replace(document, schema_resolution=None).to_dict()["schema_resolution"] is None
    with pytest.raises(FrozenInstanceError):
        document.schema_resolution.kind = "caller_supplied"


class EmptyFalseyProvider:
    def __bool__(self):
        return False

    def get_schema(self, schema_id):
        return None


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("provider", [InMemorySchemaProvider(), EmptyFalseyProvider()])
def test_explicit_empty_provider_never_falls_back(encoding, provider):
    with pytest.raises(SchemaError) as captured:
        parser(encoding)(payload(encoding), schema_provider=provider)
    assert captured.value.diagnostic.code == "schema.missing_base_schema"
    assert "no built-in" not in captured.value.diagnostic.message
    assert captured.value.diagnostic.node_type == 82


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_explicit_catalog_controls_layout_and_provenance(encoding):
    definition = TypeDefinition(
        82,
        "CALLER_VALUES",
        "caller",
        True,
        (FieldDefinition("caller_values", FieldType.INTEGER, 0, 1, True),),
        SchemaSource.BASE,
    )
    provider = InMemorySchemaProvider((SchemaCatalog("30000", (definition,)),))
    document = parser(encoding)(payload(encoding), schema_provider=provider)
    assert document.nodes[0].type_name == "CALLER_VALUES"
    assert document.schema_resolution == SchemaProviderResolution("caller_supplied")
    assert document.to_dict()["schema_resolution"] == {"kind": "caller_supplied"}
    with pytest.raises(SchemaError) as captured:
        parser(encoding)(
            payload(encoding), schema_provider=InMemorySchemaProvider((SchemaCatalog("30000", ()),))
        )
    assert captured.value.diagnostic.code == "schema.missing_type_definition"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    "key", ["SCH_3000001_30000", "SCH_3000000_30001", "SCH_3000000_30000_13006"]
)
@pytest.mark.parametrize("node_type", [82, None])
def test_unsupported_and_embedded_keys_do_not_fall_back(encoding, key, node_type):
    with pytest.raises(SchemaError) as captured:
        parser(encoding)(payload(encoding, key=key, node_type=node_type))
    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "schema.missing_base_schema"
    assert "no built-in profile supports this exact schema key" in diagnostic.message
    assert diagnostic.details["schema"] == key.split("_")[-1]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_uncovered_type_has_profile_diagnostic(encoding):
    with pytest.raises(SchemaError) as captured:
        parser(encoding)(payload(encoding, node_type=110))
    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "schema.builtin_profile_uncovered_type"
    assert diagnostic.node_type == 110
    assert diagnostic.details["profile_id"] == PROVENANCE["profile_id"]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_user_fields_rejected_before_record_payload(encoding):
    with pytest.raises(ParseError) as captured:
        parser(encoding)(payload(encoding, user_fields=1))
    assert captured.value.diagnostic.code == "node.unsupported_user_fields"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_private_native_default_retains_explicit_missing_provider_contract(encoding):
    limits = DEFAULT_PARSE_LIMITS
    args = [
        payload(encoding),
        None,
        None,
        limits.max_file_size,
        limits.max_nodes,
        limits.max_schema_types,
        limits.max_fields_per_type,
        limits.max_string_bytes,
        limits.max_variable_elements,
    ]
    native = _core._parse_xt if encoding == "x_t" else _core._parse_xb
    assert native(*args)["error"]["code"] == "schema.missing_base_schema"
    assert native(*args, allow_builtin=True)["value"]["schema_resolution"] == PROVENANCE


def test_builtin_pair_uses_existing_strict_comparison():
    left, right = parse_xt(payload("x_t")), parse_xb(payload("x_b"))
    assert compare_documents(left, right).equivalent
    changed = parse_xt(payload("x_t").replace(b"1 7 ?", b"1 8 ?"))
    assert not compare_documents(changed, right).equivalent


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_read_brep_and_cli_without_directory(encoding, tmp_path, capsys):
    # A synthetic general BODY tests plumbing; real solids are checked locally.
    path = tmp_path / f"empty.{encoding}"
    path.write_bytes(general_body_payload(encoding))
    result = read_brep(path)
    assert result.summary.schema_resolution == result.document.schema_resolution
    assert result.summary.to_dict()["schema_resolution"] == PROVENANCE
    assert main(["parse", str(path), "--brep"]) == 0
    assert json.loads(capsys.readouterr().out)["document"]["schema_resolution"] == PROVENANCE
    assert main(["check", str(path), "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["summary"]["schema_resolution"] == PROVENANCE
    assert main(["check", str(path)]) == 0
    assert PROVENANCE["profile_sha256"] in capsys.readouterr().out
    with pytest.raises(FileNotFoundError, match=r"sch_30000\.sch_txt"):
        read_brep(path, schema_dir=tmp_path)
    assert main(["parse", str(path), "--schema-dir", str(tmp_path)]) == 2
    assert json.loads(capsys.readouterr().err)["error_type"] == "FileNotFoundError"


def test_cli_compare_resolves_each_key_and_rejects_uncovered_type(tmp_path, capsys):
    left, right = tmp_path / "left.x_t", tmp_path / "right.x_b"
    left.write_bytes(payload("x_t"))
    right.write_bytes(payload("x_b"))
    assert main(["compare", str(left), str(right)]) == 0
    assert json.loads(capsys.readouterr().out)["comparison"]["equivalent"]
    right.write_bytes(payload("x_b", key="SCH_3000001_30000"))
    assert main(["compare", str(left), str(right)]) == 2
    assert "schema.missing_base_schema" in capsys.readouterr().err
    left.write_bytes(payload("x_t", node_type=110))
    assert main(["parse", str(left)]) == 2
    assert "schema.builtin_profile_uncovered_type" in capsys.readouterr().err


def test_profile_provenance_documentation_matches_runtime():
    import re
    from pathlib import Path

    root = Path(__file__).resolve().parents[1]
    document = (root / "docs/builtin-profiles.md").read_text()
    metadata = json.loads(re.search(r"```json\n(.*?)\n```", document, re.DOTALL).group(1))
    assert parse_xt(payload("x_t")).schema_resolution.to_dict() == metadata
    for name in ("README.md", "docs/api.md", "docs/format-support.md"):
        text = (root / name).read_text()
        assert KEY in text and "verified_subset" in text
