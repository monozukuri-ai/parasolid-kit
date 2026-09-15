"""Independent wire controls; native Onshape evidence remains in the local corpus."""

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
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header
from tests.test_builtin_embedded_runtime import EMBEDDED, general_body

KEY = "SCH_3701212_37102_13006"
PROFILE = {
    "kind": "builtin",
    "profile_id": "onshape-sch37102-13006-r2",
    "profile_revision": 2,
    "schema_key": KEY,
    "coverage": "verified_subset",
    "profile_sha256": "c65102214f88d51a41758fde42a3a691cf3532d0f9341533c5f7c86fa53fa93b",
}


def torus(encoding, *, edits="unchanged", major=0.029, minor=0.005, replace_region=False):
    body = general_body(encoding, EMBEDDED, replace_region=replace_region)[:-4].replace(
        EMBEDDED.encode(), KEY.encode()
    )
    center, axis, x_axis = (-0.011, 0.017, 0.023), (0, 0.6, 0.8), (1, 0, 0)
    numbers = (*center, *axis, major, minor, *x_axis)
    if encoding == "x_t":
        marker = {
            "unchanged": b"255 ",
            "copy": b"12 " + b"C" * 12 + b"Z",
            "insert": b"13 I4 note0 0 1 d" + b"C" * 12 + b"Z",
            "replace_center": b"12 " + b"C" * 7 + b"DI6 center0 0 1 v" + b"C" * 4 + b"Z",
        }[edits]
        note = b"987 " if edits == "insert" else b""
        return (
            body
            + b"54 "
            + marker
            + b"2 "
            + note
            + b"73 0 0 0 0 0 +"
            + " ".join(str(v) for v in numbers).encode()
            + b" 1 0 "
        )
    marker = {
        "unchanged": b"\xff",
        "copy": b"\x0c" + b"C" * 12 + b"Z",
        "insert": b"\x0dI\x04note\x00\x00" + positive_integer(0) + b"\x01d" + b"C" * 12 + b"Z",
        "replace_center": b"\x0c"
        + b"C" * 7
        + b"DI\x06center\x00\x00"
        + positive_integer(0)
        + b"\x01v"
        + b"C" * 4
        + b"Z",
    }[edits]
    note = struct.pack(">i", 987) if edits == "insert" else b""
    record = struct.pack(">H", 54) + marker + positive_integer(2) + note + struct.pack(">i", 73)
    record += positive_integer(0) * 5 + b"+" + struct.pack(">11d", *numbers)
    return body + record + bytes([0, 1, 0, 1])


@pytest.mark.parametrize("edits", ["unchanged", "copy", "insert"])
def test_torus_roles_follow_copied_fields_and_producer_frame(edits):
    results = [read_brep(torus(enc, edits=edits)) for enc in ("x_t", "x_b")]
    assert compare_documents(results[0].document, results[1].document).equivalent
    for result in results:
        assert result.document.schema_resolution.to_dict() == PROFILE
        assert result.brep.complete and result.brep.topology.valid
        assert result.brep.surfaces[0].kind == "torus"
        surface = result.brep.surfaces[0].definition
        assert tuple(surface.center) == (-0.011, 0.017, 0.023)
        assert tuple(surface.axis) == (0, 0.6, 0.8)
        assert tuple(surface.x_axis) == (1, 0, 0)
        assert (surface.major_radius, surface.minor_radius) == (0.029, 0.005)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("control", ["surface", "body"])
def test_replacement_fields_cannot_acquire_semantic_roles(encoding, control):
    data = torus(
        encoding,
        edits="replace_center" if control == "surface" else "unchanged",
        replace_region=control == "body",
    )
    (parse_xt if encoding == "x_t" else parse_xb)(data)
    with pytest.raises(ParseError) as captured:
        read_brep(data)
    assert captured.value.diagnostic.code == "brep.invalid_field"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_explicit_empty_provider_and_neighbor_keys_do_not_fallback(encoding):
    parser = parse_xt if encoding == "x_t" else parse_xb
    data = torus(encoding)
    with pytest.raises(SchemaError):
        parser(data, schema_provider=InMemorySchemaProvider())
    for key in ["SCH_3701213_37102_13006", "SCH_3701212_37103_13006"]:
        with pytest.raises(SchemaError) as captured:
            parser(data.replace(KEY.encode(), key.encode()))
        assert captured.value.diagnostic.code == "schema.missing_base_schema"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("kind", [3, 38, 40, 41, 133, 137, 141, 204])
def test_types_outside_reviewed_subset_fail_even_with_full_declarations(encoding, kind):
    # Zero-field full declaration cannot establish unknown base membership.
    if encoding == "x_t":
        data = text_header(KEY, schema_max_type=205) + f"{kind} 0 1 X1 X1 1 0 ".encode()
    else:
        data = (
            SyntheticXbBuilder(schema_name=KEY, schema_max_type=205)
            .add_raw_node(kind, b"\x00\x01X\x01X" + positive_integer(1))
            .build()
        )
    with pytest.raises(SchemaError) as captured:
        (parse_xt if encoding == "x_t" else parse_xb)(data)
    assert captured.value.diagnostic.code == "schema.unknown_base_type"


@pytest.mark.parametrize("key", [EMBEDDED, "SCH_3701229_37102_13006"])
def test_new_torus_base_membership_does_not_expand_existing_profiles(key):
    with pytest.raises(SchemaError) as captured:
        parse_xb(torus("x_b").replace(KEY.encode(), key.encode()))
    assert captured.value.diagnostic.code == "schema.unknown_base_type"


def test_every_truncated_binary_torus_record_is_rejected():
    data = torus("x_b")
    node = parse_xb(data).nodes[-1]
    for end in range(node.byte_range.start + 2, len(data)):
        with pytest.raises(ParseError):
            parse_xb(data[:end])


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_nonzero_user_fields_remain_outside_profile(encoding):
    if encoding == "x_t":
        data = text_header(KEY, schema_max_type=205, user_field_size=1) + b"82 255 0 1 1 0 "
    else:
        data = (
            SyntheticXbBuilder(schema_name=KEY, schema_max_type=205, user_field_size=1)
            .add_raw_node(82, b"\xff" + struct.pack(">i", 0) + positive_integer(1))
            .build()
        )
    with pytest.raises(ParseError) as captured:
        (parse_xt if encoding == "x_t" else parse_xb)(data)
    assert captured.value.diagnostic.code == "node.unsupported_user_fields"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("radius", [0, -0.005])
def test_invalid_minor_radius_remains_visible_in_raw_but_is_rejected_by_geometry(encoding, radius):
    data = torus(encoding, minor=radius)
    (parse_xt if encoding == "x_t" else parse_xb)(data)
    with pytest.raises(ParseError) as captured:
        read_brep(data)
    assert captured.value.diagnostic.code == "geometry.invalid_parameter"
