"""Partial OCCT conversion: faces whose construction fails are dropped and reported.

A face that cannot be built is left out with its reason, the solid it bounded
comes back as an open shell, a body without a converted face is skipped, and
the report lists all of it; a model where nothing fails converts exactly as in
strict mode.
"""

from __future__ import annotations

import importlib.util
import json
from dataclasses import replace

import pytest

from parasolid_kit.interop import OcctConversionError
from parasolid_kit.interop.occt import to_occt
from tests._occt_fixtures import make_cylinder_hole_model, make_half_ball_model

pytestmark = pytest.mark.skipif(
    importlib.util.find_spec("OCP") is None, reason="OCP is not installed"
)


def _bad_inner_cylinder():
    """The hole's cylinder surface no longer carries its ring circles."""
    model = make_cylinder_hole_model()
    surfaces = list(model.surfaces)
    inner = surfaces[1]
    surfaces[1] = replace(
        inner, definition=replace(inner.definition, radius=inner.definition.radius * 1.5)
    )
    return replace(model, surfaces=tuple(surfaces))


def _bad_disc_fin():
    """The disc of a half ball describes its rim with a FIN on the sphere, not the plane."""
    model = make_half_ball_model()
    curves = list(model.curves)
    index = next(i for i, curve in enumerate(curves) if curve.id == 12)
    sp_curve = curves[index]
    curves[index] = replace(sp_curve, definition=replace(sp_curve.definition, surface=1))
    return replace(model, curves=tuple(curves))


def test_strict_conversion_still_fails_on_a_face_that_cannot_be_built():
    with pytest.raises(OcctConversionError, match=r"occt\.construction_failed") as info:
        to_occt(_bad_inner_cylinder(), source_unit="mm")
    assert info.value.report.faces_dropped == ()
    assert not info.value.report.partial_requested


def test_partial_conversion_drops_the_face_and_returns_the_body_as_an_open_shell():
    result = to_occt(_bad_inner_cylinder(), source_unit="mm", partial=True)
    report = result.report
    assert report.partial_requested
    assert not report.conversion_complete
    assert report.occt_valid
    assert [item.face_id for item in report.faces_dropped] == [2]
    dropped = report.faces_dropped[0]
    assert dropped.body_id == 1
    assert "cylinder" in dropped.reason
    assert report.bodies_partial == (1,)
    assert report.bodies_failed == ()
    assert report.output_topology.to_dict()["faces"] == 3
    assert "partial_body_open_shell" in report.topology_operations
    by_code = {item.code: item for item in report.diagnostics}
    warning = by_code["occt.faces_dropped"]
    assert warning.severity.value == "warning"
    assert not warning.fatal
    assert json.loads(warning.details["faces"])[0]["face_id"] == 2
    assert "occt.metric_mismatch" not in by_code
    assert not any(
        relation.source.kind.value == "face" and relation.source.entity_id == 2
        for relation in result.source_map.relations
    )
    as_dict = report.to_dict()
    assert as_dict["partial"]["requested"] is True
    assert as_dict["partial"]["faces_dropped"][0]["face_id"] == 2
    assert as_dict["partial"]["bodies_partial"] == [1]
    assert as_dict["options"]["partial"] is True if "options" in as_dict else True


def test_partial_conversion_cannot_be_exported_as_step(tmp_path):
    from parasolid_kit.interop.occt import write_step

    result = to_occt(_bad_inner_cylinder(), source_unit="mm", partial=True)
    with pytest.raises(Exception, match=r"step\.incomplete_conversion"):
        write_step(result, tmp_path / "partial.step")


def test_partial_mode_changes_nothing_when_every_face_builds():
    strict = to_occt(make_cylinder_hole_model(), source_unit="mm")
    partial = to_occt(make_cylinder_hole_model(), source_unit="mm", partial=True)
    assert partial.report.conversion_complete
    assert partial.report.faces_dropped == ()
    assert partial.report.bodies_partial == ()
    assert partial.report.metrics == strict.report.metrics
    assert partial.report.output_topology == strict.report.output_topology
    assert partial.report.options.partial
    assert not strict.report.options.partial
    assert partial.report.to_dict()["partial"] == {
        "requested": True,
        "faces_dropped": [],
        "bodies_failed": [],
        "bodies_partial": [],
    }


def test_partial_parametric_conversion_drops_a_face_whose_fin_support_is_wrong():
    model = _bad_disc_fin()
    with pytest.raises(OcctConversionError, match=r"occt\.construction_failed"):
        to_occt(model, source_unit="mm")
    result = to_occt(model, source_unit="mm", partial=True)
    report = result.report
    assert [item.face_id for item in report.faces_dropped] == [2]
    assert "support differs" in report.faces_dropped[0].reason
    assert report.bodies_partial == (1,)
    assert report.occt_valid
    assert not report.conversion_complete
    assert report.output_topology.to_dict()["faces"] == 1


def test_partial_conversion_fails_when_no_body_survives(monkeypatch):
    from parasolid_kit.interop.occt.topology import TopologyBuilder

    def broken(self, edge):
        raise ValueError("edge construction is broken in this test")

    monkeypatch.setattr(TopologyBuilder, "_build_initial_edge", broken)
    with pytest.raises(OcctConversionError, match="no body of the B-Rep model") as info:
        to_occt(make_cylinder_hole_model(), source_unit="mm", partial=True)
    report = info.value.report
    assert [item.body_id for item in report.bodies_failed] == [1]
    assert len(report.faces_dropped) == 4
    assert all("edge" in item.reason for item in report.faces_dropped)


def test_partial_preview_reports_a_partial_conversion(tmp_path):
    from parasolid_kit.interop.preview import PreviewOptions, write_preview

    model = _bad_inner_cylinder()
    result = to_occt(model, source_unit="mm", partial=True)
    preview = write_preview(
        result, model, tmp_path / "preview", options=PreviewOptions(allow_partial=True)
    )
    assert preview.report.status == "partial"
    assert not preview.report.conversion_complete
    assert preview.report.source_complete


def test_partial_conversion_returns_a_shape_brepcheck_rejects_with_a_warning():
    # The authored Onshape cylinder with a hole converts whole, but OCCT's
    # BRepCheck rejects the shape; strict mode fails and partial mode returns
    # it as built, with the finding as a warning.
    from parasolid_kit import read_brep

    model = read_brep("data/onshape-cylinder-hole-v30.x_t").brep
    with pytest.raises(OcctConversionError, match=r"occt\.invalid_shape"):
        to_occt(model, source_unit="m")
    result = to_occt(model, source_unit="m", partial=True)
    report = result.report
    assert report.partial_requested and report.conversion_complete
    assert not report.occt_valid
    assert report.faces_dropped == () and report.bodies_failed == ()
    warnings = [item for item in report.diagnostics if item.code == "occt.invalid_shape"]
    assert [item.severity.value for item in warnings] == ["warning"]
    assert report.output_topology.to_dict()["faces"] == 4
