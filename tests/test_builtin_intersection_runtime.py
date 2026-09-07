"""Independent wire probes; these small general bodies are not real-solid evidence."""

import math
import struct

import pytest

from parasolid_kit import ParseError, compare_documents, parse_xb, parse_xt, read_brep, write_xb
from parasolid_kit.brep import IntersectionCurve
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_schema import positive_integer
from tests.test_builtin_embedded_runtime import BASE, EMBEDDED, general_body


def intersection(
    encoding,
    key,
    *,
    edits="unchanged",
    chart_count=2,
    chart_ref=5,
    start_ref=6,
    end_ref=7,
    surface_ref=3,
    limit_kind="L",
    limit_count=1,
    coordinate=-0.011,
):
    data = bytearray(general_body(encoding, key)[:-4])
    seen = {12}

    def record(kind, index, fields, variable=None):
        # Independent field groups and values; no runtime definition lookup.
        if encoding == "x_t":
            data.extend(f"{kind} ".encode())
            if key == EMBEDDED and kind not in seen:
                data.extend(
                    b"255 "
                    if edits == "unchanged"
                    else f"{len(fields)} ".encode() + b"C" * len(fields) + b"Z"
                )
            if variable is not None:
                data.extend(f"{variable} ".encode())
            data.extend(f"{index} ".encode())
            for code, values in fields:
                if code == "h" and None in values:
                    values = [None, *values[3:]]
                for value in values:
                    if code == "c":
                        data.extend(value.encode())
                    elif value is None:
                        data.extend(b"?")
                    else:
                        data.extend(f"{value} ".encode())
        else:
            data.extend(struct.pack(">H", kind))
            if key == EMBEDDED and kind not in seen:
                data.extend(
                    b"\xff"
                    if edits == "unchanged"
                    else bytes([len(fields)]) + b"C" * len(fields) + b"Z"
                )
            if variable is not None:
                data.extend(struct.pack(">i", variable))
            data.extend(positive_integer(index))
            for code, values in fields:
                for value in values:
                    if code == "p":
                        data.extend(positive_integer(value))
                    elif code == "c":
                        data.extend(value.encode())
                    elif code == "d":
                        data.extend(struct.pack(">i", value))
                    else:
                        data.extend(struct.pack(">d", -3.14158e13 if value is None else value))
        seen.add(kind)

    def common(index, indirect=0):
        return [("d", [index]), *[("p", [v]) for v in (0, 0, 0, 0, indirect)], ("c", ["+"])]

    for index, normal, owner in [(2, [0, 0, 1], 8), (3, [0, 1, 0], 9)]:
        record(
            50, index, [*common(index, owner), ("v", [0, 0, 0]), ("v", normal), ("v", [1, 0, 0])]
        )
    record(
        38,
        4,
        [
            *common(4),
            ("p", [2, surface_ref]),
            ("p", [chart_ref]),
            ("p", [start_ref]),
            ("p", [end_ref]),
        ],
    )
    record(
        40,
        5,
        [
            ("f", [0]),
            ("f", [1]),
            ("d", [chart_count]),
            ("f", [None]),
            ("f", [None]),
            ("f", [None, None]),
            ("h", [coordinate, 0, 0, 0.019, 0, 0]),
        ],
        2,
    )
    for index, point in [(6, [coordinate, 0, 0]), (7, [0.019, 0, 0])]:
        record(41, index, [("c", [limit_kind]), ("h", point * limit_count)], limit_count)
    for index, shared in [(8, 2), (9, 3)]:
        record(141, index, [("p", [v]) for v in [4, index, index, shared]])
    data.extend(b"1 0 " if encoding == "x_t" else bytes([0, 1, 0, 1]))
    return bytes(data)


@pytest.mark.parametrize(
    "key,edits", [(BASE, "unchanged"), (EMBEDDED, "unchanged"), (EMBEDDED, "copy")]
)
@pytest.mark.parametrize("limit_kind,limit_count", [("H", 1), ("L", 1), ("T", 2)])
def test_intersection_references_and_hvec_arrays_preserve_source(
    key, edits, limit_kind, limit_count
):
    text, binary = [
        read_brep(
            intersection(enc, key, edits=edits, limit_kind=limit_kind, limit_count=limit_count)
        )
        for enc in ("x_t", "x_b")
    ]
    assert compare_documents(text.document, binary.document).equivalent
    assert write_xb(binary.document) == intersection(
        "x_b", key, edits=edits, limit_kind=limit_kind, limit_count=limit_count
    )
    for result in [text, binary]:
        assert result.brep.complete and result.brep.topology.valid
        curve = result.brep.curves[0].definition
        assert isinstance(curve, IntersectionCurve)
        assert curve.surfaces == tuple(s.id for s in result.brep.surfaces)
        assert (curve.chart.node_index, curve.start.node_index, curve.end.node_index) == (5, 6, 7)
        assert curve.intersection_data is None
        nodes = {n.index: n for n in result.document.nodes}
        chart = nodes[5]
        assert chart.variable_length == 2
        assert len(chart.fields[-1].values) == 2
        assert chart.fields[-1].values[0].value == (-0.011, 0.0, 0.0)
        if result is binary:
            assert (
                chart.fields[-1].byte_range.length == 48
            )  # h is 3 doubles, not cached tangent/UV/t.
        assert nodes[4].fields[7].definition.element_count == 2
        assert len(nodes[6].fields[1].values) == limit_count
        assert [f.values[0].value for f in nodes[8].fields] == [4, 8, 8, 2]
        for index in [4, 5, 6, 8]:
            assert nodes[index].definition.source is (
                SchemaSource.BASE
                if key == BASE
                else SchemaSource.EMBEDDED_UNCHANGED
                if edits == "unchanged"
                else SchemaSource.EMBEDDED_DELTA
            )


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", [BASE, EMBEDDED])
@pytest.mark.parametrize(
    "change",
    [
        {"chart_count": 1},
        {"chart_count": -1},
        {"chart_count": 0},
        {"chart_ref": 2},
        {"start_ref": 5},
        {"end_ref": 0},
        {"surface_ref": 5},
        {"limit_kind": "B"},
        {"limit_kind": "?"},
        {"limit_kind": "T"},
        {"limit_count": 2},
        {"coordinate": None},
    ],
)
def test_invalid_intersection_structure_is_rejected_by_brep(encoding, key, change):
    data = intersection(encoding, key, **change)
    (parse_xt if encoding == "x_t" else parse_xb)(data)  # Raw values remain inspectable.
    with pytest.raises(ParseError):
        read_brep(data)


@pytest.mark.parametrize("key", [BASE, EMBEDDED])
def test_nonfinite_hvec_rejected_and_every_truncated_binary_prefix_fails(key):
    data = intersection("x_b", key, coordinate=math.inf)
    assert parse_xb(data).nodes[4].fields[-1].values[0].value[0] == math.inf
    with pytest.raises(ParseError):
        read_brep(data)
    data = intersection("x_b", key)
    start = parse_xb(data).nodes[3].byte_range.start
    for end in range(start + 2, len(data)):
        with pytest.raises(ParseError):
            parse_xb(data[:end])
