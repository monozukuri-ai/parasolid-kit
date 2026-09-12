"""Independent wire fixtures for the V30 complex-geometry extension."""

import struct

import pytest

from parasolid_kit import ParseError, compare_documents, parse_xb, parse_xt, read_brep
from tests.support.parasolid_schema import positive_integer
from tests.test_builtin_cone_runtime import cone
from tests.test_builtin_embedded_runtime import BASE, general_body
from tests.test_builtin_intersection_runtime import intersection
from tests.test_builtin_schema_runtime import general_body_payload
from tests.test_builtin_spcurve_runtime import spcurve


def v30(data, encoding, *, intersection_delta=False):
    """Replace only the independently specified body prefix and V30 limit additions."""
    if intersection_delta:
        parsed = (parse_xt if encoding == "x_t" else parse_xb)(data)
        insertions = []
        for node in parsed.nodes:
            if node.node_type == 38:
                insertions.append(
                    (node.byte_range.end, b"0 " if encoding == "x_t" else positive_integer(0))
                )
            if node.node_type == 41:
                insertions.append((node.fields[1].byte_range.start, b"N"))
        for offset, extra in sorted(insertions, reverse=True):
            data = data[:offset] + extra + data[offset:]
    return general_body_payload(encoding)[:-4] + data[len(general_body(encoding, BASE)) - 4 :]


@pytest.mark.parametrize("case", ["cone", "pcurve", "surface", "intersection"])
def test_v30_geometry_roles_and_text_binary_agree(case):
    outputs = []
    for encoding in ("x_t", "x_b"):
        if case == "cone":
            data = cone(encoding, BASE)
        elif case == "intersection":
            data = intersection(encoding, BASE)
        else:
            patch = (
                None if case == "pcurve" else dict(vertices=(0, 0, 0, 0, 3, 0, 2, 0, 0, 2, 3, 1))
            )
            data = spcurve(encoding, patch=patch)
        result = read_brep(v30(data, encoding, intersection_delta=case == "intersection"))
        assert result.brep.complete
        assert result.document.schema_resolution.profile_revision == 3
        if case == "intersection":
            curve = result.brep.curves[0].definition
            assert [p.to_tuple() for p in curve.chart_points] == [(-0.011, 0, 0), (0.019, 0, 0)]
            assert curve.intersection_data is None
            limits = [n for n in result.document.nodes if n.node_type == 41]
            assert all(n.fields[1].values[0].value == ord("N") for n in limits)
        outputs.append(result)
    assert compare_documents(outputs[0].document, outputs[1].document).equivalent
    # The body prefix differs between versions, but geometry values retain their interpretation.
    assert len(outputs[0].brep.curves) == len(outputs[1].brep.curves)


@pytest.mark.parametrize("kind", [87, 89, 204])
def test_v30_variable_dependencies_and_truncation(kind):
    if kind == 204:
        values = (2.125, -7.25, 0.0)
        text_payload = b"4 2.125 -7.25 0 "
        binary_payload = b"\x04" + struct.pack(">3d", *values)
    else:
        values = (1.25, -2.5, 3.75, -4.5, 5.25, 6.125, 0, 0, 1)
        text_payload = b"1.25 -2.5 3.75 -4.5 5.25 6.125 0 0 1 "
        binary_payload = struct.pack(">9d", *values)
    text = general_body_payload("x_t")[:-4] + f"{kind} 3 2 ".encode() + text_payload + b"1 0 "
    binary = (
        general_body_payload("x_b")[:-4]
        + struct.pack(">Hi", kind, 3)
        + positive_integer(2)
        + binary_payload
        + bytes([0, 1, 0, 1])
    )
    a, b = parse_xt(text), parse_xb(binary)
    assert compare_documents(a, b).equivalent
    assert b.nodes[-1].variable_length == 3
    start = b.nodes[-1].byte_range.start
    for end in range(start + 2, len(binary)):
        with pytest.raises(ParseError):
            parse_xb(binary[:end])
