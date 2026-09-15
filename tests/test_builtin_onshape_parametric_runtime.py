"""Synthetic embedded controls for the current-key direct B-spline subset."""

import struct

import pytest

from parasolid_kit import ParseError, compare_documents, parse_xb, parse_xt, read_brep, write_xb
from parasolid_kit.brep import EllipseCurve, NurbsCurve, NurbsSurface
from tests.support.parasolid_schema import positive_integer
from tests.test_builtin_embedded_runtime import EMBEDDED, general_body
from tests.test_builtin_onshape_current_runtime import KEY, PROFILE
from tests.test_builtin_spcurve_runtime import spcurve


def splines(encoding, **kwargs):
    return spcurve(encoding, embedded=True, include_surface_curves=False, **kwargs).replace(
        EMBEDDED.encode(), KEY.encode()
    )


def ellipse(encoding):
    data = general_body(encoding, EMBEDDED)[:-4].replace(EMBEDDED.encode(), KEY.encode())
    # Public base ELLIPSE: owner prefix, orientation, center, normal, major axis, major/minor radii.
    numbers = (-0.013, 0.017, 0.023, 0, 0.6, 0.8, 1, 0, 0, 0.019, 0.007)
    if encoding == "x_t":
        return data + b"32 255 2 73 0 0 0 0 0 +" + " ".join(map(str, numbers)).encode() + b" 1 0 "
    return (
        data
        + struct.pack(">H", 32)
        + b"\xff"
        + positive_integer(2)
        + struct.pack(">i", 73)
        + positive_integer(0) * 5
        + b"+"
        + struct.pack(">11d", *numbers)
        + bytes([0, 1, 0, 1])
    )


def test_ellipse_preserves_reference_frame_and_distinct_radii():
    a, b = [read_brep(ellipse(e)) for e in ("x_t", "x_b")]
    assert compare_documents(a.document, b.document).equivalent
    for r in (a, b):
        assert r.document.schema_resolution.to_dict() == PROFILE
        c = r.brep.curves[0].definition
        assert isinstance(c, EllipseCurve)
        assert tuple(c.center) == (-0.013, 0.017, 0.023)
        assert tuple(c.normal) == (0, 0.6, 0.8)
        assert (c.major_radius, c.minor_radius) == (0.019, 0.007)


@pytest.mark.parametrize("rational", [False, True])
@pytest.mark.parametrize("periodic_axis", [None, "u", "v"])
def test_nurbs_preserves_asymmetric_coefficients_and_periodic_overlap(rational, periodic_axis):
    rows, columns = (6, 4) if periodic_axis else (5, 4)
    poles = [
        [(0.003 * i, 0.007 * j, 0.001 * (i * j + i)) for j in range(columns)] for i in range(rows)
    ]
    weights = [[1 + 0.1 * i + 0.03 * j for j in range(columns)] for i in range(rows)]
    if periodic_axis:
        poles += poles[:3]
        weights += weights[:3]
    axes = {
        "u": (3, tuple(i / 6 for i in range(-3, 10)), (1,) * 13)
        if periodic_axis
        else (3, (0, 0.371, 1), (4, 1, 4)),
        "v": (2, (0, 0.619, 1), (3, 1, 3)),
    }
    if periodic_axis == "v":
        poles = list(zip(*poles, strict=True))
        weights = list(zip(*weights, strict=True))
        axes = {"u": axes["v"], "v": axes["u"]}
    coefficients = tuple(
        x
        for row, wr in zip(poles, weights, strict=True)
        for point, w in zip(row, wr, strict=True)
        for x in ((*[v * w for v in point], w) if rational else point)
    )
    patch = dict(rational=rational, dimension=4 if rational else 3, vertices=coefficients)
    for axis, count in [("u", len(poles)), ("v", len(poles[0]))]:
        degree, knots, mult = axes[axis]
        patch.update(
            {
                f"{axis}_degree": degree,
                f"{axis}_count": count,
                f"{axis}_knots": knots,
                f"{axis}_knot_count": len(knots),
                f"{axis}_mult": mult,
                f"{axis}_periodic": periodic_axis == axis,
                f"{axis}_closed": periodic_axis == axis,
            }
        )
    a, b = [
        read_brep(
            splines(
                e, patch=patch, dimension=3, vertices=(0.011, -0.017, 0.023, 0.031, 0.019, -0.007)
            )
        )
        for e in ("x_t", "x_b")
    ]
    assert compare_documents(a.document, b.document).equivalent
    assert write_xb(b.document) == splines(
        "x_b", patch=patch, dimension=3, vertices=(0.011, -0.017, 0.023, 0.031, 0.019, -0.007)
    )
    for r in (a, b):
        assert r.brep.complete and r.brep.topology.valid
        assert r.document.schema_resolution.to_dict() == PROFILE
        n = r.brep.surfaces[0].definition
        assert isinstance(n, NurbsSurface) and n.rational == rational
        assert tuple(x for point in n.control_vertices for x in point) == coefficients
        assert (n.u_control_vertex_count, n.v_control_vertex_count) == (len(poles), len(poles[0]))
        for axis, (degree, knots, mult) in axes.items():
            assert getattr(n, f"{axis}_degree") == degree
            assert getattr(n, f"{axis}_knots") == knots
            assert getattr(n, f"{axis}_knot_multiplicities") == mult
            assert getattr(n, f"{axis}_periodic") == (periodic_axis == axis)
        assert isinstance(r.brep.curves[0].definition, NurbsCurve)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    "change",
    [
        {"patch": {"u_mult": (0, 4)}},
        {"patch": {"u_degree": 0}},
        {"patch": {"rational": True, "dimension": 4, "vertices": (0, 0, 0, 0) * 4}},
        {"knots": (1, 0)},
        {"replace_nurbs": True},
    ],
)
def test_invalid_dependencies_and_reinserted_roles_remain_raw_only(encoding, change):
    data = splines(encoding, **change)
    (parse_xt if encoding == "x_t" else parse_xb)(data)
    with pytest.raises(ParseError):
        read_brep(data)


def test_every_truncated_binary_spline_prefix_is_rejected():
    data = splines("x_b", patch={})
    for end in range(len(data)):
        with pytest.raises(ParseError):
            parse_xb(data[:end])
