"""Schema-free base/embedded selection and preservation of copied B-Rep roles."""

import json
import struct

import pytest

from parasolid_kit import (
    InMemorySchemaProvider,
    ParseError,
    SchemaError,
    compare_documents,
    parse_xb,
    parse_xt,
    read_brep,
)
from parasolid_kit.cli import main
from parasolid_kit.diagnostics import DiagnosticKind
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header

BASE = "SCH_1300000_13006"
EMBEDDED = "SCH_3000310_30000_13006"
PROFILES = {
    BASE: (
        "onshape-sch13006-r6",
        "2748e9f9c28fa32b59edb9e0b16fea7a976ad081f698dd3d098d9cbd17e08fbb",
    ),
    EMBEDDED: (
        "icad-sch30000-13006-r5",
        "1f090c87aef63e99af8dcb3aef9cc077a613af6749f177392989cca70ca3bfa5",
    ),
}


def builder(key, user_fields=0):
    return SyntheticXbBuilder(
        schema_name=key,
        schema_max_type=205 if key == EMBEDDED else None,
        user_field_size=user_fields,
    )


def header(key, user_fields=0):
    return text_header(
        key,
        schema_max_type=205 if key == EMBEDDED else None,
        user_field_size=user_fields,
    )


def array(encoding, key, user_fields=0):
    if encoding == "x_t":
        marker = b"255 " if key == EMBEDDED else b""
        return header(key, user_fields) + b"82 " + marker + b"2 1 -7 ?1 0 "
    marker = b"\xff" if key == EMBEDDED else b""
    record = marker + struct.pack(">i", 2) + positive_integer(1) + struct.pack(">ii", -7, -32764)
    return builder(key, user_fields).add_raw_node(82, record).build()


def general_body(encoding, key, *, replace_region=False):
    """Synthetic general BODY; insert a field to move all base wire ordinals."""
    values = [0] * 23
    values[7], values[8], values[14] = 1e-6, 1e-8, 6
    text_edits, binary_edits = b"", b""
    if key == EMBEDDED:
        text_edits = b"24 I4 note0 0 1 d"
        binary_edits = b"\x18I\x04note\x00\x00" + positive_integer(0) + b"\x01d"
        for ordinal in range(23):
            if replace_region and ordinal == 20:
                text_edits += b"DI11 region_head19 0 "
                binary_edits += b"DI\x0bregion_head" + struct.pack(">H", 19) + positive_integer(0)
            else:
                text_edits += b"C"
                binary_edits += b"C"
        text_edits += b"Z"
        binary_edits += b"Z"
    if encoding == "x_t":
        note = b"73 " if key == EMBEDDED else b""
        return (
            header(key)
            + b"12 "
            + text_edits
            + b"1 "
            + note
            + "".join(f"{value} " for value in values).encode()
            + b"1 0 "
        )
    record = binary_edits + positive_integer(1)
    if key == EMBEDDED:
        record += struct.pack(">i", 73)
    for ordinal, value in enumerate(values):
        if ordinal == 0:
            record += struct.pack(">i", value)
        elif ordinal in (7, 8):
            record += struct.pack(">d", value)
        elif ordinal in (12, 14, 15):
            record += bytes([value])
        else:
            record += positive_integer(value)
    return builder(key).add_raw_node(12, record).build()


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", [BASE, EMBEDDED])
def test_defaults_select_base_and_embedded_profiles(encoding, key):
    parser = parse_xt if encoding == "x_t" else parse_xb
    data = array(encoding, key)
    document = parser(data)
    assert document == parser(data, schema_provider=None)
    profile_id, digest = PROFILES[key]
    assert document.schema_resolution.to_dict() == {
        "kind": "builtin",
        "profile_id": profile_id,
        "profile_revision": 6 if key == BASE else 5,
        "schema_key": key,
        "coverage": "verified_subset",
        "profile_sha256": digest,
    }
    assert document.schemas[0].definition.source is (
        SchemaSource.EMBEDDED_UNCHANGED if key == EMBEDDED else SchemaSource.BASE
    )
    assert [v.value for v in document.nodes[0].fields[0].values] == [-7, None]
    with pytest.raises(SchemaError, match=r"schema\.missing_base_schema"):
        parser(data, schema_provider=InMemorySchemaProvider())


@pytest.mark.parametrize("key", [BASE, EMBEDDED])
def test_cross_encoding_and_brep_with_shifted_fields(key):
    a, b = general_body("x_t", key), general_body("x_b", key)
    assert compare_documents(parse_xt(a), parse_xb(b)).equivalent
    for data in (a, b):
        result = read_brep(data)
        assert result.brep.complete and result.brep.topology.valid
        assert len(result.brep.bodies) == 1
        assert result.summary.schema_resolution == result.document.schema_resolution


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_identical_inserted_name_cannot_replace_a_copied_role(encoding):
    data = general_body(encoding, EMBEDDED, replace_region=True)
    parser = parse_xt if encoding == "x_t" else parse_xb
    assert len(parser(data).nodes) == 1  # A valid raw definition with unverified meaning.
    with pytest.raises(ParseError, match=r"brep\.invalid_field"):
        read_brep(data)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_unknown_membership_is_rejected_before_full_declaration(encoding):
    if encoding == "x_t":
        data = header(EMBEDDED) + b"110 1 3 NEW0 5 value0 0 1 d1 91 1 0 "
    else:
        record = b"\x01\x03NEW\x00\x05value\x00\x00" + positive_integer(0) + b"\x01d"
        record += positive_integer(1) + struct.pack(">i", 91)
        data = builder(EMBEDDED).add_raw_node(110, record).build()
    parser = parse_xt if encoding == "x_t" else parse_xb
    with pytest.raises(SchemaError) as captured:
        parser(data)
    diagnostic = captured.value.diagnostic
    assert diagnostic.code == "schema.unknown_base_type"
    assert diagnostic.kind is DiagnosticKind.INCOMPLETE
    assert diagnostic.node_type == 110
    assert diagnostic.details["schema"] == "13006"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", [BASE, EMBEDDED])
def test_user_fields_are_rejected_at_the_header(encoding, key):
    parser = parse_xt if encoding == "x_t" else parse_xb
    with pytest.raises(ParseError, match=r"node\.unsupported_user_fields"):
        parser(array(encoding, key, user_fields=1))


@pytest.mark.parametrize("key", [BASE, EMBEDDED])
def test_cli_parses_brep_without_a_catalog(key, tmp_path, capsys):
    path = tmp_path / "synthetic.x_b"
    path.write_bytes(general_body("x_b", key))
    assert main(["check", str(path), "--json"]) == 0
    output = json.loads(capsys.readouterr().out)
    assert output["summary"]["schema_resolution"]["profile_id"] == PROFILES[key][0]
