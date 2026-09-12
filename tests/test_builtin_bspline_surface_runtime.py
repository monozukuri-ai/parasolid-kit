"""Independent V13 wire examples for SP_CURVE's B-spline surface dependency."""

import struct

import pytest

from parasolid_kit import ParseError, compare_documents, parse_xb, parse_xt, read_brep, write_xb
from parasolid_kit.brep import NurbsSurface, SurfaceParametricCurve
from tests.test_builtin_embedded_runtime import EMBEDDED, builder, header
from tests.test_builtin_spcurve_runtime import spcurve


@pytest.mark.parametrize("rational", [False, True])
def test_bilinear_surface_preserves_axes_and_homogeneous_coordinates(rational):
    poles = ((0, 0, 0), (0, 0.03, 0), (0.02, 0, 0), (0.02, 0.03, 0.002))
    weights = (1, 0.5, 2, 1)
    vertices = tuple(
        v
        for p, w in zip(poles, weights, strict=True)
        for v in ((*[x * w for x in p], w) if rational else p)
    )
    patch = dict(rational=rational, dimension=4 if rational else 3, vertices=vertices)
    a, b = [read_brep(spcurve(e, patch=patch)) for e in ("x_t", "x_b")]
    assert compare_documents(a.document, b.document).equivalent
    assert write_xb(b.document) == spcurve("x_b", patch=patch)
    for result in (a, b):
        assert result.brep.complete and result.brep.topology.valid
        assert not result.brep.diagnostics
        surface = result.brep.surfaces[0]
        n = surface.definition
        assert isinstance(n, NurbsSurface)
        assert n.u_degree == n.v_degree == 1
        assert n.u_control_vertex_count == n.v_control_vertex_count == 2
        assert n.rational == rational
        assert tuple(v for p in n.control_vertices for v in p) == vertices
        assert n.u_knots == n.v_knots == (0, 1)
        assert n.u_knot_multiplicities == n.v_knot_multiplicities == (2, 2)
        assert [s.node_type for s in n.sources] == [126, 45, 127, 127, 128, 128]
        for c in result.brep.curves:
            if isinstance(c.definition, SurfaceParametricCurve):
                assert c.definition.surface == surface.id
        assert any(n.node_type == 125 for n in result.document.nodes)


@pytest.mark.parametrize("rational", [False, True])
@pytest.mark.parametrize("periodic_axis", [None, "u", "v", "uv"])
def test_tensor_surface_preserves_nonuniform_knots_and_periodic_overlap(rational, periodic_axis):
    # Asymmetric rows/columns catch transposition. Periodic rows include the
    # three overlapping poles and imaginary knots seen in producer exports.
    rows = 6 if periodic_axis else 5
    columns = 5 if periodic_axis == "uv" else 4
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
        "v": (2, tuple(i / 5 for i in range(-2, 8)), (1,) * 10)
        if periodic_axis == "uv"
        else (2, (0, 0.619, 1), (3, 1, 3)),
    }
    if periodic_axis == "uv":
        poles = [row + row[:2] for row in poles]
        weights = [row + row[:2] for row in weights]
    if periodic_axis == "v":
        poles = list(zip(*poles, strict=True))
        weights = list(zip(*weights, strict=True))
        axes = {"u": axes["v"], "v": axes["u"]}
    vertices = tuple(
        coordinate
        for row, weight_row in zip(poles, weights, strict=True)
        for point, weight in zip(row, weight_row, strict=True)
        for coordinate in ((*[x * weight for x in point], weight) if rational else point)
    )
    patch = dict(rational=rational, dimension=4 if rational else 3, vertices=vertices)
    for axis, count in [("u", len(poles)), ("v", len(poles[0]))]:
        degree, knots, mult = axes[axis]
        patch.update(
            {
                f"{axis}_degree": degree,
                f"{axis}_count": count,
                f"{axis}_knots": knots,
                f"{axis}_knot_count": len(knots),
                f"{axis}_mult": mult,
                f"{axis}_periodic": periodic_axis in (axis, "uv"),
                f"{axis}_closed": periodic_axis in (axis, "uv"),
            }
        )
    a, b = [read_brep(spcurve(e, patch=patch)) for e in ("x_t", "x_b")]
    assert compare_documents(a.document, b.document).equivalent
    assert write_xb(b.document) == spcurve("x_b", patch=patch)
    for result in (a, b):
        assert result.brep.complete and result.brep.topology.valid
        n = result.brep.surfaces[0].definition
        assert isinstance(n, NurbsSurface)
        assert tuple(x for p in n.control_vertices for x in p) == vertices
        assert (n.u_control_vertex_count, n.v_control_vertex_count) == (len(poles), len(poles[0]))
        for axis, (degree, knots, mult) in axes.items():
            assert getattr(n, f"{axis}_degree") == degree
            assert getattr(n, f"{axis}_knots") == knots
            assert getattr(n, f"{axis}_knot_multiplicities") == mult
            assert getattr(n, f"{axis}_periodic") == (periodic_axis in (axis, "uv"))
            assert getattr(n, f"{axis}_closed") == (periodic_axis in (axis, "uv"))
        assert n.rational == rational


@pytest.mark.parametrize("encoding,parser", [("x_t", parse_xt), ("x_b", parse_xb)])
@pytest.mark.parametrize(
    "patch",
    [
        {"nurbs": 4},
        {"nurbs": 0},
        {"u_degree": 0},
        {"u_degree": 2},
        {"u_count": 0},
        {"dimension": 2},
        {"u_knot_count": 3},
        {"u_knots": (1, 0)},
        {"u_knots": (0, 0)},
        {"u_knots": (0, None)},
        {"u_mult": (1, 2)},
        {"u_mult": (0, 4)},
        {"u_periodic": True},
        {"v_degree": 0},
        {"v_degree": 2},
        {"v_count": 0},
        {"v_knot_count": 3},
        {"v_knots": (1, 0)},
        {"v_knots": (0, 0)},
        {"v_mult": (1, 2)},
        {"v_periodic": True},
        {"rational": True},
        {"rational": True, "dimension": 4, "vertices": (0, 0, 0, 0) * 4},
    ],
)
def test_invalid_surface_dependencies_remain_raw(encoding, parser, patch):
    data = spcurve(encoding, patch=patch)
    assert parser(data)
    with pytest.raises(ParseError):
        read_brep(data)


@pytest.mark.parametrize("key", [EMBEDDED])
@pytest.mark.parametrize("kind", [124, 125, 126])
def test_bspline_surface_layouts_do_not_leak_to_other_profiles(key, kind):
    for data, parser in [
        (header(key) + f"{kind} 1 ".encode(), parse_xt),
        (builder(key).build()[:-4] + struct.pack(">H", kind), parse_xb),
    ]:
        with pytest.raises(ParseError) as error:
            parser(data)
        assert error.value.diagnostic.code in (
            "schema.unknown_base_type",
            "schema.builtin_profile_uncovered_type",
        )
