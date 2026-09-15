"""Independent analytic checks, including wrong geometry and real STEP decoding."""

from __future__ import annotations

import importlib.util
import math

import pytest

from parasolid_kit import SurfaceKind
from scripts import release_corpus_oracle as oracle
from tests._occt_fixtures import make_closed_analytic_surface_model, make_cone_frustum_model


def cone():
    return dict(
        type="CONE",
        origin=[2, -3, 4],
        axis=[0, 0, 1],
        radius=3,
        halfAngle=math.atan(0.5),
    )


def torus():
    return dict(type="TORUS", origin=[2, -3, 4], axis=[0, 0, 1], majorRadius=8, minorRadius=2)


def test_cone_matches_same_surface_with_different_reference_sections_and_axis_sign():
    a = cone()
    assert oracle.matches(a, {**a, "origin": [2, -3, 6], "radius": 4})
    assert oracle.matches(a, {**a, "axis": [0, 0, -1], "halfAngle": -a["halfAngle"]})
    assert not oracle.matches(a, {**a, "axis": [0, 0, -1]})


@pytest.mark.parametrize(
    "change",
    [
        {"origin": [2.001, -3, 4]},
        {"radius": 3.001},
        {"halfAngle": math.atan(0.501)},
        {"axis": [1, 0, 0]},
    ],
)
def test_cone_rejects_wrong_apex_angle_and_axis(change):
    assert not oracle.matches(cone(), {**cone(), **change})


@pytest.mark.parametrize("angle", [0, math.pi / 2, float("nan")])
def test_cone_degenerate_or_nonfinite_angle_is_not_a_match(angle):
    with pytest.raises(ValueError, match="cone angle"):
        oracle.matches(cone(), {**cone(), "halfAngle": angle})


def test_cone_source_coefficients_have_the_same_apex_as_the_independent_dimensions():
    values = oracle.source_geometry(make_cone_frustum_model(8.0, 4.0, 12.0))
    source = next(g for g in values if g["type"] == "CONE")
    expected = dict(
        type="CONE",
        origin=[0, 0, 0],
        axis=[0, 0, 1],
        radius=8,
        halfAngle=math.atan(-1 / 3),
    )
    assert oracle.matches(source, expected)
    with pytest.raises(ValueError, match="normalization"):
        oracle.matches({**source, "sinHalfAngle": 1}, expected)


def test_ring_torus_matches_axis_reversal_and_source_model():
    a = torus()
    assert oracle.matches(a, {**a, "axis": [0, 0, -1]})
    source = oracle.source_geometry(make_closed_analytic_surface_model(SurfaceKind.TORUS))
    assert len(source) == 1
    assert oracle.matches(source[0], {**a, "origin": [0, 0, 0]})


@pytest.mark.parametrize(
    "change",
    [
        {"majorRadius": 8.001},
        {"minorRadius": 2.001},
        {"origin": [2, -3, 4.001]},
        {"axis": [1, 0, 0]},
    ],
)
def test_ring_torus_rejects_wrong_radii_center_and_axis(change):
    assert not oracle.matches(torus(), {**torus(), **change})


@pytest.mark.parametrize("major,minor", [(-8, 2), (1, 2), (2, 2), (8, 0), (float("nan"), 2)])
def test_non_ring_or_invalid_torus_remains_outside_the_oracle(major, minor):
    with pytest.raises(ValueError, match="ring tori"):
        oracle.matches(torus(), {**torus(), "majorRadius": major, "minorRadius": minor})


@pytest.mark.skipif(
    importlib.util.find_spec("OCP") is None, reason="requires the optional OCCT profile"
)
@pytest.mark.parametrize("kind", ["cone", "torus"])
def test_step_oracle_reads_direct_kernel_primitives_in_metres(tmp_path, kind):
    # This STEP is synthesized directly by OCCT, without parasolid-kit conversion.
    # It validates the oracle reader, not Onshape export compatibility.
    from OCP.BRepPrimAPI import BRepPrimAPI_MakeCone, BRepPrimAPI_MakeTorus
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.Interface import Interface_Static
    from OCP.STEPControl import STEPControl_AsIs, STEPControl_Writer

    shape = (
        BRepPrimAPI_MakeCone(10, 5, 20) if kind == "cone" else BRepPrimAPI_MakeTorus(10, 2)
    ).Shape()
    writer = STEPControl_Writer()
    previous = {
        key: Interface_Static.CVal_s(key) for key in ("xstep.cascade.unit", "write.step.unit")
    }
    path = tmp_path / f"{kind}.step"
    try:
        for key in previous:
            assert Interface_Static.SetCVal_s(key, "MM")
        assert writer.Transfer(shape, STEPControl_AsIs) == IFSelect_RetDone
        assert writer.Write(str(path)) == IFSelect_RetDone
    finally:
        for key, value in previous.items():
            Interface_Static.SetCVal_s(key, value)
    result = oracle.step_geometry(path)
    assert result["bodies"] == 1
    actual = next(g for g in result["geometry"] if g["type"] == kind.upper())
    expected = (
        dict(type="CONE", origin=[0, 0, 0], axis=[0, 0, 1], radius=0.01, halfAngle=math.atan(-0.25))
        if kind == "cone"
        else dict(
            type="TORUS", origin=[0, 0, 0], axis=[0, 0, 1], majorRadius=0.01, minorRadius=0.002
        )
    )
    assert oracle.matches(actual, expected)
    volume = (
        math.pi * 0.02 * (0.01**2 + 0.01 * 0.005 + 0.005**2) / 3
        if kind == "cone"
        else 2 * math.pi**2 * 0.01 * 0.002**2
    )
    assert result["volume"] == pytest.approx(volume, rel=1e-9)
