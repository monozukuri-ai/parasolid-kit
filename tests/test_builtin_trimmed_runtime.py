"""Independent type-133 wire probes; real iCAD evidence remains local."""

import math
import struct

import pytest

from parasolid_kit import ParseError, compare_documents, parse_xb, parse_xt, read_brep, write_xb
from parasolid_kit.brep import TrimmedCurve
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_schema import embedded_field, positive_integer
from tests.test_builtin_embedded_runtime import BASE, EMBEDDED, general_body


def trimmed(
    encoding,
    *,
    key=EMBEDDED,
    edits="unchanged",
    basis=2,
    basis_sense="+",
    sense="+",
    start=-0.013,
    end=0.027,
    coordinate=0.011,
):
    data = bytearray(general_body(encoding, key)[:-4])
    seen = {12}

    def record(kind, index, fields):
        if kind == 133 and edits in ("shift", "duplicate"):
            fields.insert(0, ("d", [73]))
        if key != EMBEDDED or kind in seen:
            schema = b""
        elif kind != 133 or edits == "unchanged":
            schema = b"255 " if encoding == "x_t" else b"\xff"
        else:
            count = 13 if edits in ("shift", "duplicate") else 12
            schema = f"{count} ".encode() if encoding == "x_t" else bytes([count])
            if edits in ("shift", "duplicate"):
                name = "note" if edits == "shift" else "basis_curve"
                schema += b"I" + (
                    f"{len(name)} {name}0 0 1 d".encode()
                    if encoding == "x_t"
                    else embedded_field(name)
                )
            if edits == "replace":
                # Reinsert an identical field: correct bytes, unverified identity.
                schema += b"C" * 7 + b"DI"
                schema += (
                    b"11 basis_curve1008 0 "
                    if encoding == "x_t"
                    else embedded_field("basis_curve", pointer_class=1008)
                )
                schema += b"C" * 4
            else:
                schema += b"C" * 12
            schema += b"Z"
        if encoding == "x_t":
            data.extend(f"{kind} ".encode() + schema + f"{index} ".encode())
            for code, values in fields:
                for value in values:
                    data.extend(value.encode() if code == "c" else f"{value} ".encode())
        else:
            data.extend(struct.pack(">H", kind) + schema + positive_integer(index))
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

    def common(index, orientation):
        return [("d", [index]), *[("p", [0])] * 5, ("c", [orientation])]

    # A forward basis pointer and a second trim using the cached definition.
    for index, a, b in [(3, start, end), (4, -0.019, 0.031)]:
        if basis_sense == "-":
            a, b = b, a
        record(
            133,
            index,
            [
                *common(index, sense),
                ("p", [basis]),
                ("v", [coordinate, -0.017, a + 0.023]),
                ("v", [0.011, -0.017, b + 0.023]),
                ("f", [a]),
                ("f", [b]),
            ],
        )
    record(
        30,
        2,
        [*common(2, basis_sense), ("v", [0.011, -0.017, 0.023]), ("v", [0, 0, 1])],
    )
    data.extend(b"1 0 " if encoding == "x_t" else bytes([0, 1, 0, 1]))
    return bytes(data)


@pytest.mark.parametrize("edits", ["unchanged", "copy", "shift"])
@pytest.mark.parametrize("basis_sense", ["+", "-"])
def test_trim_roles_forward_reference_and_cached_instances(edits, basis_sense):
    a, b = [read_brep(trimmed(enc, edits=edits, basis_sense=basis_sense)) for enc in ["x_t", "x_b"]]
    assert compare_documents(a.document, b.document).equivalent
    assert write_xb(b.document) == trimmed("x_b", edits=edits, basis_sense=basis_sense)
    for result in [a, b]:
        assert result.brep.complete and result.brep.topology.valid
        curves = {c.source.node_index: c for c in result.brep.curves}
        for index in (3, 4):
            definition = curves[index].definition
            assert isinstance(definition, TrimmedCurve)
            assert definition.basis_curve == curves[2].id
            for point, t in [
                (definition.start_point, definition.start_parameter),
                (definition.end_point, definition.end_parameter),
            ]:
                assert (point.x, point.y, point.z) == pytest.approx((0.011, -0.017, t + 0.023))
        trims = [n for n in result.document.nodes if n.node_type == 133]
        assert trims[0].first_schema is not None and trims[1].first_schema is None
        assert trims[0].definition.source is (
            SchemaSource.EMBEDDED_UNCHANGED if edits == "unchanged" else SchemaSource.EMBEDDED_DELTA
        )


@pytest.mark.parametrize("encoding,parser", [("x_t", parse_xt), ("x_b", parse_xb)])
@pytest.mark.parametrize(
    "change",
    [
        {"basis": 0},
        {"basis": 1},
        {"basis": 99},
        {"basis": 3},
        {"start": 0.027},
        {"end": -0.019},
        {"sense": "-"},
        {"sense": "?"},
        {"edits": "replace"},
        {"edits": "duplicate"},
    ],
)
def test_invalid_trim_semantics_remain_raw_but_are_rejected_by_brep(encoding, parser, change):
    data = trimmed(encoding, **change)
    assert len(parser(data).nodes) == 4
    with pytest.raises(ParseError):
        read_brep(data)


@pytest.mark.parametrize("coordinate", [None, math.inf, math.nan])
def test_null_and_nonfinite_trim_points_are_rejected(coordinate):
    data = trimmed("x_b", coordinate=coordinate)
    assert len(parse_xb(data).nodes) == 4
    with pytest.raises(ParseError):
        read_brep(data)


@pytest.mark.parametrize("end", [math.inf, math.nan, -3.14158e13])
def test_null_and_nonfinite_trim_parameters_are_rejected(end):
    data = trimmed("x_b", end=end)
    assert len(parse_xb(data).nodes) == 4
    with pytest.raises(ParseError):
        read_brep(data)


def test_trim_scope_and_binary_truncation():
    from tests.test_builtin_embedded_runtime import builder, header

    for data, parser in [
        (header("SCH_3000001_30000") + b"133 1 0 ", parse_xt),
        (
            builder("SCH_3000001_30000").build()[:-4]
            + struct.pack(">H", 133)
            + positive_integer(1),
            parse_xb,
        ),
    ]:
        with pytest.raises(ParseError) as error:
            parser(data)
        assert error.value.diagnostic.code == "schema.missing_base_schema"
    data = trimmed("x_b")
    for end in range(parse_xb(data).nodes[1].byte_range.start + 2, len(data)):
        with pytest.raises(ParseError):
            parse_xb(data[:end])
    with pytest.raises(ParseError):
        parse_xb(data.replace(EMBEDDED.encode(), b"SCH_3000311_30000_13006"))


@pytest.mark.parametrize("basis_sense", ["+", "-"])
def test_v13_trim_has_the_same_semantics_without_embedded_markers(basis_sense):
    a, b = [read_brep(trimmed(enc, key=BASE, basis_sense=basis_sense)) for enc in ["x_t", "x_b"]]
    assert a.brep.complete and b.brep.complete
    assert a.brep.topology.valid and b.brep.topology.valid
    assert compare_documents(a.document, b.document).equivalent
    assert write_xb(b.document) == trimmed("x_b", key=BASE, basis_sense=basis_sense)
    curves = {c.source.node_index: c for c in b.brep.curves}
    for index in (3, 4):
        assert isinstance(curves[index].definition, TrimmedCurve)
        assert curves[index].definition.basis_curve == curves[2].id
    assert (
        next(s for s in b.document.schemas if s.definition.node_type == 133).definition.source
        is SchemaSource.BASE
    )
    for enc, parser in [("x_t", parse_xt), ("x_b", parse_xb)]:
        data = trimmed(enc, key=BASE, basis_sense=basis_sense, basis=1)
        assert len(parser(data).nodes) == 4
        with pytest.raises(ParseError):
            read_brep(data)
