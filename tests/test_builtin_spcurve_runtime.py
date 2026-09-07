"""Public-reference V13 SP_CURVE bytes, independent of compiled definitions."""

import math
import struct

import pytest

from parasolid_kit import (
    ParseError,
    ParseLimits,
    compare_documents,
    parse_xb,
    parse_xt,
    read_brep,
    write_xb,
)
from parasolid_kit.brep import CylinderSurface, NurbsCurve, SurfaceParametricCurve
from tests.support.parasolid_schema import positive_integer
from tests.test_builtin_embedded_runtime import BASE, EMBEDDED, builder, general_body, header


def spcurve(
    encoding,
    *,
    degree=1,
    count=2,
    dimension=2,
    knot_count=2,
    rational=False,
    periodic=False,
    closed=False,
    vertices=(0.011, -0.017, 0.023, 0.031),
    knots=(0.0, 1.0),
    multiplicities=(2, 2),
    parameter=4,
    surface=2,
    nurbs=5,
    original=0,
    tolerance=None,
    cylinder=False,
    patch=None,
):
    data = bytearray(general_body(encoding, BASE)[:-4])

    def record(kind, index, codes, values, length=None):
        if encoding == "x_t":
            data.extend(f"{kind} ".encode())
            if length is not None:
                data.extend(f"{length} ".encode())
            data.extend(f"{index} ".encode())
        else:
            data.extend(struct.pack(">H", kind))
            if length is not None:
                data.extend(struct.pack(">i", length))
            data.extend(positive_integer(index))
        for code, value in zip(codes, values, strict=True):
            if encoding == "x_t":
                token = (
                    "?"
                    if value is None
                    else "T"
                    if code == "l" and value
                    else "F"
                    if code == "l"
                    else str(value)
                )
                data.extend((token + ("" if code in "cl" or value is None else " ")).encode())
            elif code == "p":
                data.extend(positive_integer(value))
            elif code == "c":
                data.extend(value.encode())
            else:
                if value is None:
                    value = -3.14158e13 if code == "f" else -32764
                data.extend(
                    struct.pack(
                        ">" + {"n": "h", "d": "i", "f": "d", "u": "B", "l": "B"}[code], value
                    )
                )

    def common(index):
        return [index, 0, 0, 0, 0, 0, "+"]

    # Forward links, repeated SP_CURVE and array types exercise cached definitions.
    for index in (3, 10):
        record(137, index, "dpppppcpppf", [*common(index), surface, parameter, original, tolerance])
    if patch is not None:
        record(124, 2, "dpppppcpp", [*common(2), patch.get("nurbs", 12), 13])
        record(
            126,
            12,
            "llnndduuddlllunppppp",
            [
                patch.get("u_periodic", False),
                False,
                patch.get("u_degree", 1),
                1,
                patch.get("u_count", 2),
                2,
                1,
                1,
                patch.get("u_knot_count", 2),
                2,
                patch.get("rational", False),
                patch.get("u_closed", False),
                False,
                3,
                patch.get("dimension", 3),
                14,
                15,
                16,
                17,
                18,
            ],
        )
        # Interval wire values are two consecutive doubles each.
        record(
            125,
            13,
            "f" * 8 + "u" + "c" * 12 + "p" * 4,
            [0, 1] * 4 + [1] + ["B"] * 8 + ["?"] * 4 + [0] * 4,
        )
        pv = patch.get("vertices", (0, 0, 0, 0, 0.03, 0, 0.02, 0, 0, 0.02, 0.03, 0.002))
        pm = patch.get("u_mult", (2, 2))
        pk = patch.get("u_knots", (0, 1))
        record(45, 14, "f" * len(pv), pv, len(pv))
        record(127, 15, "n" * len(pm), pm, len(pm))
        record(127, 16, "nn", [2, 2], 2)
        record(128, 17, "f" * len(pk), pk, len(pk))
        record(128, 18, "ff", [0, 1], 2)
    elif cylinder:
        record(
            51,
            2,
            "dpppppc" + "f" * 10,
            [*common(2), 0.013, -0.019, 0.029, 0, 0, -1, 0.011013, -1, 0, 0],
        )
    else:
        record(50, 2, "dpppppc" + "f" * 9, [*common(2), 0.013, -0.019, 0.029, 0, 0, 1, 1, 0, 0])
    record(134, 4, "dpppppcpp", [*common(4), nurbs, 6])
    record(
        136,
        5,
        "ndndullluppp",
        [degree, count, dimension, knot_count, 5, periodic, closed, rational, 1, 7, 8, 9],
    )
    record(135, 6, "up", [1, 0])
    record(45, 7, "f" * len(vertices), vertices, len(vertices))
    record(127, 8, "n" * len(multiplicities), multiplicities, len(multiplicities))
    record(128, 9, "f" * len(knots), knots, len(knots))
    record(128, 11, "", [], 0)
    data.extend(b"1 0 " if encoding == "x_t" else bytes([0, 1, 0, 1]))
    return bytes(data)


@pytest.mark.parametrize(
    "change",
    [
        {},
        {"knots": (0.0, 1.0, None), "multiplicities": (2, 2, 0)},
        {"original": 4, "tolerance": 1e-8},
        {
            "degree": 2,
            "count": 4,
            "dimension": 3,
            "knot_count": 3,
            "rational": True,
            "vertices": (0, 0, 1, 0.006, 0.012, 0.5, 0.034, 0.046, 2, 0.039, 0.051, 1),
            "knots": (-0.25, 0.375, 1.25),
            "multiplicities": (3, 1, 3),
        },
        {
            "degree": 3,
            "count": 7,
            "dimension": 3,
            "knot_count": 11,
            "rational": True,
            "periodic": True,
            "closed": True,
            "vertices": (
                0.01,
                0.02,
                1,
                0.015,
                0.01,
                0.5,
                0.06,
                0.08,
                2,
                0.01,
                0.04,
                1,
                0.01,
                0.02,
                1,
                0.015,
                0.01,
                0.5,
                0.06,
                0.08,
                2,
            ),
            "knots": (-0.75, -0.5, -0.25, 0, 0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75),
            "multiplicities": (1,) * 11,
        },
    ],
)
def test_v13_spcurve_dependencies_and_homogeneous_values(change):
    a, b = [read_brep(spcurve(e, **change)) for e in ("x_t", "x_b")]
    assert compare_documents(a.document, b.document).equivalent
    assert write_xb(b.document) == spcurve("x_b", **change)
    for result in (a, b):
        assert result.brep.complete and result.brep.topology.valid
        assert not result.brep.diagnostics
        curves = {c.source.node_index: c for c in result.brep.curves}
        spline = curves[4].definition
        assert isinstance(spline, NurbsCurve)
        assert tuple(v for p in spline.control_vertices for v in p) == change.get(
            "vertices", (0.011, -0.017, 0.023, 0.031)
        )
        assert spline.knots == change.get("knots", (0.0, 1.0))[: change.get("knot_count", 2)]
        assert spline.rational == change.get("rational", False)
        assert spline.degree == change.get("degree", 1)
        assert spline.periodic == change.get("periodic", False)
        assert spline.closed == change.get("closed", False)
        assert (
            spline.knot_multiplicities
            == change.get("multiplicities", (2, 2))[: change.get("knot_count", 2)]
        )
        assert [s.node_type for s in spline.sources] == [136, 45, 127, 128]
        for index in (3, 10):
            sp = curves[index].definition
            assert isinstance(sp, SurfaceParametricCurve)
            assert sp.parameter_curve == curves[4].id
            assert sp.surface == result.brep.surfaces[0].id
            assert sp.original_curve == (curves[4].id if change.get("original") else None)
            assert sp.tolerance_to_original == change.get("tolerance")
        nodes = [n for n in result.document.nodes if n.node_type == 137]
        assert nodes[0].first_schema is not None and nodes[1].first_schema is None
        assert next(n for n in result.document.nodes if n.index == 11).variable_length == 0


@pytest.mark.parametrize("reverse", [False, True])
def test_open_uv_curve_can_wind_once_around_a_cylinder(reverse):
    # A 2D open line with an unwrapped angle embeds as a closed 3D circle.
    # Keeping its angle span matters: reducing each pole modulo 2*pi collapses it.
    poles = [(math.pi, 0.015006), (3 * math.pi, 0.015006)]
    if reverse:
        poles.reverse()
    vertices = tuple(v for p in poles for v in p)
    a, b = [read_brep(spcurve(e, vertices=vertices, cylinder=True)) for e in ("x_t", "x_b")]
    assert compare_documents(a.document, b.document).equivalent
    assert write_xb(b.document) == spcurve("x_b", vertices=vertices, cylinder=True)
    for result in (a, b):
        assert result.brep.complete and result.brep.topology.valid
        surface = result.brep.surfaces[0].definition
        assert isinstance(surface, CylinderSurface)
        curves = {c.id: c for c in result.brep.curves}
        sp = next(
            c.definition
            for c in curves.values()
            if isinstance(c.definition, SurfaceParametricCurve)
        )
        spline = curves[sp.parameter_curve].definition
        assert isinstance(spline, NurbsCurve)
        assert spline.control_vertices == tuple(poles)
        assert not spline.closed and not spline.periodic
        points = []
        for t in (0.0, 0.25, 0.5, 1.0):
            u, v = [(1 - t) * a + t * b for a, b in zip(*spline.control_vertices, strict=True)]
            points.append(
                (
                    surface.point.x - surface.radius * math.cos(u),
                    surface.point.y + surface.radius * math.sin(u),
                    surface.point.z - v,
                )
            )
        assert math.dist(points[0], points[-1]) < 1e-14
        assert math.dist(points[0], points[2]) == pytest.approx(2 * surface.radius)
        assert points[1][1] == pytest.approx(
            surface.point.y + (1 if reverse else -1) * surface.radius
        )


@pytest.mark.parametrize("encoding,parser", [("x_t", parse_xt), ("x_b", parse_xb)])
@pytest.mark.parametrize(
    "change",
    [
        {"parameter": 0},
        {"parameter": 2},
        {"parameter": 3},
        {"parameter": 99},
        {"surface": 0},
        {"surface": 4},
        {"nurbs": 6},
        {"original": 2},
        {"tolerance": -1e-8},
        {"degree": 0},
        {"degree": None},
        {"degree": 2},
        {"count": -1},
        {"count": 3},
        {"dimension": 0},
        {"dimension": 3},
        {"knot_count": 3},
        {"knots": (1, 0)},
        {"knots": (0, 0)},
        {"knots": (0, None)},
        {"multiplicities": (0, 4)},
        {"multiplicities": (1, 2)},
        {"multiplicities": (2, 2, 1)},
        {
            "degree": 2,
            "count": 6,
            "knot_count": 3,
            "vertices": tuple(v for i in range(6) for v in (i * 0.01, i * 0.02)),
            "knots": (0, 0.5, 1),
            "multiplicities": (3, 3, 3),
        },
        {
            "degree": 3,
            "count": 4,
            "knot_count": 3,
            "vertices": tuple(v for i in range(4) for v in (i * 0.01, i * 0.02)),
            "knots": (0, 0.5, 1),
            "multiplicities": (2, 3, 3),
        },
        {"knots": (0, 1, 0.5)},
        {"periodic": True},
        {"rational": True, "dimension": 3, "vertices": (0, 0, 1, 1, 1, 0)},
        {"rational": True, "dimension": 3, "vertices": (0, 0, 1, 1, 1, -1)},
    ],
)
def test_invalid_spcurve_semantics_are_retained_raw_and_rejected_by_brep(encoding, parser, change):
    data = spcurve(encoding, **change)
    assert len(parser(data).nodes) == 11
    with pytest.raises(ParseError):
        read_brep(data)


@pytest.mark.parametrize("value", [None, math.inf, math.nan])
def test_invalid_control_coordinates(value):
    data = spcurve("x_b", vertices=(value, 0, 1, 1))
    assert parse_xb(data)
    with pytest.raises(ParseError):
        read_brep(data)


@pytest.mark.parametrize("value", [math.inf, math.nan])
def test_nonfinite_spcurve_tolerance(value):
    with pytest.raises(ParseError):
        read_brep(spcurve("x_b", tolerance=value))


def test_spcurve_scope_truncation_and_array_limits():
    for key in (EMBEDDED, "SCH_3000000_30000"):
        for data, parser in [
            (header(key) + b"137 1 ", parse_xt),
            (builder(key).build()[:-4] + struct.pack(">H", 137), parse_xb),
        ]:
            with pytest.raises(ParseError) as error:
                parser(data)
            assert error.value.diagnostic.code in (
                "schema.unknown_base_type",
                "schema.builtin_profile_uncovered_type",
            )
    data = spcurve("x_b")
    for end in range(parse_xb(data).nodes[1].byte_range.start + 2, len(data)):
        with pytest.raises(ParseError):
            parse_xb(data[:end])
    with pytest.raises(ParseError):
        parse_xb(data.replace(BASE.encode(), b"SCH_1300001_13006"))
    for encoding, parser in [("x_t", parse_xt), ("x_b", parse_xb)]:
        with pytest.raises(ParseError) as error:
            parser(spcurve(encoding), limits=ParseLimits(max_variable_elements=3))
        assert error.value.diagnostic.code == "limits.exceeded"
