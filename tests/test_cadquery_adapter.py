from __future__ import annotations

import importlib.util
import math
import sys

import pytest

from parasolid_kit import CurveKind
from parasolid_kit.interop import CadQueryConversionError, InteropDependencyError, dependency
from parasolid_kit.interop.cadquery import load_runtime, to_cadquery, to_cadquery_shapes
from tests._occt_fixtures import (
    make_analytic_curve_sheet_model,
    make_box_model,
    make_two_box_model,
)

HAS_CADQUERY = importlib.util.find_spec("cadquery") is not None


def test_cadquery_api_import_is_lazy_and_exposes_the_i5_contract() -> None:
    assert callable(load_runtime)
    assert callable(to_cadquery)
    assert callable(to_cadquery_shapes)
    assert to_cadquery.__module__ == "parasolid_kit.interop.cadquery"
    assert issubclass(CadQueryConversionError, Exception)


def test_cadquery_adapter_validates_the_core_input_before_the_optional_profile() -> None:
    with pytest.raises(TypeError, match="brep must be BrepModel"):
        to_cadquery(object(), source_unit="mm")  # type: ignore[arg-type]


def test_missing_cadquery_profile_is_reported_before_occt_conversion(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    imported_before = {
        name
        for name in sys.modules
        if name == "OCP"
        or name.startswith("OCP.")
        or name == "cadquery"
        or name.startswith("cadquery.")
    }
    monkeypatch.setattr(dependency, "_distribution_version", lambda _name: None)

    with pytest.raises(InteropDependencyError) as captured:
        to_cadquery(make_box_model(), source_unit="mm")

    assert captured.value.diagnostic.code == "interop.missing_dependency"
    assert captured.value.diagnostic.details["required_extra"] == "cadquery"
    assert captured.value.diagnostic.details["minimum_python"] == "3.11"
    imported_after = {
        name
        for name in sys.modules
        if name == "OCP"
        or name.startswith("OCP.")
        or name == "cadquery"
        or name.startswith("cadquery.")
    }
    assert imported_after == imported_before


@pytest.mark.skipif(not HAS_CADQUERY, reason="requires the optional CadQuery profile")
def test_single_body_returns_the_most_specific_shape_with_matching_metrics() -> None:
    import cadquery as cq

    shape = to_cadquery(make_box_model(), source_unit="mm")
    bounds = shape.BoundingBox()

    assert isinstance(shape, cq.Shape)
    assert isinstance(shape, cq.Solid)
    assert not isinstance(shape, cq.Compound)
    assert not isinstance(shape, cq.Workplane)
    assert len(shape.Faces()) == 6
    assert math.isclose(shape.Area(), 5200.0, rel_tol=1.0e-10)
    assert math.isclose(shape.Volume(), 24000.0, rel_tol=1.0e-10)
    assert (bounds.xmin, bounds.ymin, bounds.zmin) == pytest.approx((0.0, 0.0, 0.0))
    assert (bounds.xmax, bounds.ymax, bounds.zmax) == pytest.approx((40.0, 30.0, 20.0))


@pytest.mark.skipif(not HAS_CADQUERY, reason="requires the optional CadQuery profile")
def test_multiple_bodies_return_compound_and_per_body_shapes_in_source_order() -> None:
    import cadquery as cq

    model = make_two_box_model()
    compound = to_cadquery(model, source_unit="mm")
    shapes = to_cadquery_shapes(model, source_unit="mm")

    assert isinstance(compound, cq.Compound)
    assert not isinstance(compound, cq.Assembly)
    assert len(compound.Solids()) == 2
    assert len(compound.Faces()) == 12
    assert math.isclose(compound.Area(), 10400.0, rel_tol=1.0e-10)
    assert math.isclose(compound.Volume(), 48000.0, rel_tol=1.0e-10)
    assert compound.BoundingBox().xmax == pytest.approx(90.0)
    assert isinstance(shapes, tuple)
    assert len(shapes) == 2
    assert all(isinstance(shape, cq.Solid) for shape in shapes)
    assert [shape.BoundingBox().xmin for shape in shapes] == pytest.approx([0.0, 50.0])
    assert [shape.Volume() for shape in shapes] == pytest.approx([24000.0, 24000.0])


@pytest.mark.skipif(not HAS_CADQUERY, reason="requires the optional CadQuery profile")
def test_source_units_are_forwarded_to_the_strict_occt_conversion() -> None:
    shape = to_cadquery(make_box_model(0.04, 0.03, 0.02), source_unit="m")

    assert shape.BoundingBox().xmax == pytest.approx(40.0)
    assert math.isclose(shape.Area(), 5200.0, rel_tol=1.0e-10)
    assert math.isclose(shape.Volume(), 24000.0, rel_tol=1.0e-10)


@pytest.mark.skipif(not HAS_CADQUERY, reason="requires the optional CadQuery profile")
def test_sheet_body_uses_solid_only_volume_validation() -> None:
    import cadquery as cq

    shape = to_cadquery(
        make_analytic_curve_sheet_model(CurveKind.ELLIPSE),
        source_unit="mm",
    )

    assert isinstance(shape, cq.Shell)
    assert shape.isValid()
    assert len(shape.Faces()) == 1
    assert len(shape.Solids()) == 0
    assert math.isclose(shape.Area(), 32.0 * math.pi, rel_tol=1.0e-10)
