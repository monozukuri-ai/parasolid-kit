"""Independent cone wire probes for the reviewed 13006 profiles."""

import math
import struct

import pytest

from parasolid_kit import ParseError, compare_documents, parse_xb, parse_xt, read_brep
from parasolid_kit.brep import ConeSurface
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_schema import positive_integer
from tests.test_builtin_embedded_runtime import BASE, EMBEDDED, general_body


def cone(encoding, key, *, radius=0.007013, edits="unchanged", sin=-0.6, cos=0.8):
    """General BODY plus an unowned cone; not evidence for real solid coverage."""
    body = general_body(encoding, key)[:-4]
    if encoding == "x_t":
        marker = b""
        if key == EMBEDDED:
            marker = b"255 " if edits == "unchanged" else b"13 " + b"C" * 13 + b"Z"
        scalar = "?" if radius is None else str(radius) + " "
        record = (
            b"52 "
            + marker
            + f"2 73 0 0 0 0 0 --0.011 0.017 -0.023 0.6 0.8 0 {scalar}{sin} {cos} 0 0 -1 ".encode()
        )
        return body + record + b"1 0 "
    marker = b""
    if key == EMBEDDED:
        marker = b"\xff" if edits == "unchanged" else b"\x0d" + b"C" * 13 + b"Z"
    record = struct.pack(">H", 52) + marker + positive_integer(2) + struct.pack(">i", 73)
    record += positive_integer(0) * 5 + b"-"
    record += struct.pack(
        ">12d",
        -0.011,
        0.017,
        -0.023,
        0.6,
        0.8,
        0,
        radius if radius is not None else -3.14158e13,
        sin,
        cos,
        0,
        0,
        -1,
    )
    return body + record + bytes([0, 1, 0, 1])


@pytest.mark.parametrize(
    "key,edits", [(BASE, "unchanged"), (EMBEDDED, "unchanged"), (EMBEDDED, "copy")]
)
@pytest.mark.parametrize("radius", [0.0, 0.007013])
def test_cone_fields_and_brep_roles_survive_base_and_embedded_copy(key, edits, radius):
    text, binary = [read_brep(cone(enc, key, radius=radius, edits=edits)) for enc in ("x_t", "x_b")]
    assert compare_documents(text.document, binary.document).equivalent
    for result in (text, binary):
        assert result.brep.complete and result.brep.topology.valid
        node = result.document.nodes[-1]
        assert node.node_type == 52 and len(node.fields) == 13
        assert [f.definition.field_type.value for f in node.fields] == list("dpppppcvvfffv")
        source = node.definition.source
        assert source is (
            SchemaSource.BASE
            if key == BASE
            else SchemaSource.EMBEDDED_UNCHANGED
            if edits == "unchanged"
            else SchemaSource.EMBEDDED_DELTA
        )
        surface = result.brep.surfaces[0]
        assert surface.sense.value == "negative"
        assert isinstance(surface.definition, ConeSurface)
        assert surface.source.node_index == 2
        geometry = surface.definition
        assert (geometry.point.x, geometry.point.y, geometry.point.z) == (-0.011, 0.017, -0.023)
        assert (geometry.axis.x, geometry.axis.y, geometry.axis.z) == (0.6, 0.8, 0)
        assert geometry.radius == radius
        assert geometry.sin_half_angle == -0.6 and geometry.cos_half_angle == 0.8
        assert (geometry.x_axis.x, geometry.x_axis.y, geometry.x_axis.z) == (0, 0, -1)
        assert math.isclose(math.hypot(geometry.sin_half_angle, geometry.cos_half_angle), 1)


@pytest.mark.parametrize("encoding,parse", [("x_t", parse_xt), ("x_b", parse_xb)])
@pytest.mark.parametrize("key", [BASE, EMBEDDED])
@pytest.mark.parametrize("radius", [None, -0.001])
def test_cone_raw_values_do_not_hide_null_or_negative_radius(encoding, parse, key, radius):
    data = cone(encoding, key, radius=radius)
    assert parse(data).nodes[-1].fields[9].values[0].value == radius
    with pytest.raises(ParseError) as captured:
        read_brep(data)
    assert captured.value.diagnostic.code == "geometry.invalid_parameter"


@pytest.mark.parametrize("key", [BASE, EMBEDDED])
def test_every_truncated_cone_binary_prefix_is_rejected(key):
    data = cone("x_b", key)
    start = parse_xb(data).nodes[-1].byte_range.start
    for end in range(start + 2, len(data)):
        with pytest.raises(ParseError):
            parse_xb(data[:end])
