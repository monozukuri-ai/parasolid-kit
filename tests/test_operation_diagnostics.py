"""Public failure contracts: codes, source attribution, batch positions and JSON."""

import json
from dataclasses import replace

import pytest

from parasolid_kit import (
    AssemblyPlacementError,
    DiagnosticKind,
    FacetedGeometryError,
    GeometryEvaluationError,
    LineCurve,
    ParasolidError,
    TrimmedCurve,
    UnsupportedGeometry,
    Vector3,
    assembly_occurrences,
    decode_embedded_mesh,
    decode_polyline,
    evaluate_curve,
    evaluate_curve_2d,
    evaluate_curve_2d_many,
    evaluate_surface,
    evaluate_surface_curve,
    evaluate_surface_curve_many,
    faceted_geometry,
    sample_curve,
)
from parasolid_kit.interop import InteropLimits, OcctConversionError, PreviewError
from parasolid_kit.interop.occt import conversion, to_occt
from parasolid_kit.interop.preview import tessellate_preview, write_faceted_preview
from tests.support.faceted import stored_document
from tests.support.import_contract import assert_dependency_free_imports
from tests.test_geometry_evaluation import curve as curve_3d
from tests.test_occt_placement import _assembled_box
from tests.test_surface_curve_evaluation import ORIGIN, curve_entity, model_for, uv_curve


def _assert_failure(error, code, *, source=None, field=None):
    assert isinstance(error, ValueError)
    assert isinstance(error, ParasolidError)
    diagnostic = error.diagnostic
    assert diagnostic.code == code
    assert diagnostic.severity.value == "error"
    assert diagnostic.fatal and not diagnostic.recoverable
    if field is not None:
        assert diagnostic.details["field"] == field
    if source is not None:
        assert diagnostic.node_type == source.node_type
        assert diagnostic.node_id == source.node_id
        assert diagnostic.location.byte_offset == source.byte_range.start
        assert diagnostic.details["node_index"] == source.node_index
    payload = json.loads(json.dumps(diagnostic.to_dict(), allow_nan=False))
    assert payload["code"] == code
    return diagnostic


def test_scalar_curve_range_error_preserves_valueerror_message_and_stored_source():
    model = model_for()
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_curve_2d(model.curves[0], 1.25, model=model)
    diagnostic = _assert_failure(
        caught.value,
        "geometry.parameter_out_of_range",
        source=model.curves[0].source,
        field="parameter",
    )
    assert str(caught.value) == "parameter is outside the NURBS active knot interval"
    assert diagnostic.schema_key == model.schema_key
    assert diagnostic.details["entity_id"] == model.curves[0].id
    assert diagnostic.details["entity_kind"] == "curve"
    assert {key: diagnostic.details[key] for key in ("parameter", "minimum", "maximum")} == {
        "parameter": 1.25,
        "minimum": 0,
        "maximum": 1,
    }


@pytest.mark.parametrize(
    "parameter",
    [float("nan"), float("inf"), -float("inf"), True, 10**1000, 10**5000, None, "bad"],
    ids=["nan", "positive_inf", "negative_inf", "bool", "huge", "oversized", "none", "text"],
)
def test_invalid_parameters_still_produce_strict_json_diagnostics(parameter):
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_curve_2d(uv_curve(), parameter)
    diagnostic = _assert_failure(caught.value, "geometry.invalid_parameter", field="parameter")
    assert "parameter" in diagnostic.details
    assert diagnostic.node_id is None and diagnostic.location is None


def test_batch_reports_failing_input_index_without_partial_return():
    model = model_for()
    for evaluate, curve in (
        (evaluate_curve_2d_many, model.curves[0]),
        (evaluate_surface_curve_many, model.curves[1]),
    ):
        with pytest.raises(GeometryEvaluationError) as caught:
            evaluate(curve, iter((0.0, 0.3, 1.25, 0.2)), model=model)
        diagnostic = _assert_failure(
            caught.value,
            "geometry.parameter_out_of_range",
            source=model.curves[0].source,
            field="parameter",
        )
        assert diagnostic.details["sample_index"] == 2
        assert diagnostic.details["parameter"] == 1.25


def test_lifting_keeps_support_source_uv_axis_and_driving_curve_parameter():
    uv = replace(uv_curve(), control_vertices=((2.0, 0.0), (2.5, 0.0), (3.0, 1.0)))
    model = model_for(uv)
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_surface_curve_many(model.curves[1], [0.5], model=model)
    diagnostic = _assert_failure(
        caught.value, "geometry.parameter_out_of_range", source=model.surfaces[0].source, field="u"
    )
    assert diagnostic.details["entity_kind"] == "surface"
    assert diagnostic.details["parameter"] == 2.5
    assert diagnostic.details["u"] == 2.5 and diagnostic.details["v"] == 0.25
    assert diagnostic.details["curve_parameter"] == 0.5
    assert diagnostic.details["sample_index"] == 0


def test_nested_preparation_keeps_leaf_source_and_does_not_invent_missing_node_id():
    model = model_for(replace(uv_curve(), periodic=True))
    uv = replace(model.curves[0], source=replace(model.curves[0].source, node_id=None))
    model = replace(model, curves=(uv, model.curves[1]))
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_surface_curve(model.curves[1], 0.5, model=model)
    diagnostic = _assert_failure(
        caught.value, "geometry.unsupported_geometry", source=uv.source, field="periodic"
    )
    assert diagnostic.kind is DiagnosticKind.UNSUPPORTED
    assert diagnostic.details["entity_id"] == uv.id
    assert "sample_index" not in diagnostic.details
    assert "node_id" not in diagnostic.to_dict()


def test_signed_source_tags_do_not_mask_the_original_failure():
    curve = curve_entity(1, uv_curve())
    curve = replace(curve, source=replace(curve.source, node_id=-7))
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_curve_2d(curve, 1.25)
    diagnostic = _assert_failure(caught.value, "geometry.parameter_out_of_range")
    assert diagnostic.node_id is None
    assert diagnostic.details["source_node_id"] == -7
    assert diagnostic.details["node_index"] == curve.source.node_index
    assert diagnostic.location.byte_offset == curve.source.byte_range.start


def test_missing_reference_identifies_the_referring_node_and_field():
    model = model_for()
    sp = replace(
        model.curves[1], definition=replace(model.curves[1].definition, parameter_curve=999)
    )
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_surface_curve(sp, 0.5, model=model)
    diagnostic = _assert_failure(
        caught.value, "geometry.invalid_reference", source=sp.source, field="parameter_curve"
    )
    assert diagnostic.details["reference_id"] == 999


def test_surface_knot_errors_identify_the_correct_axis():
    model = model_for()
    surface = replace(
        model.surfaces[0], definition=replace(model.surfaces[0].definition, v_knots=(0.0, 0.0))
    )
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_surface(surface, 0.5, 0.5)
    diagnostic = _assert_failure(
        caught.value, "geometry.invalid_nurbs", source=surface.source, field="v_knots"
    )
    assert diagnostic.details["u"] == diagnostic.details["v"] == 0.5


def test_singular_direction_and_sampling_have_actionable_fields_and_indices():
    curve = curve_entity(10, LineCurve(ORIGIN, ORIGIN))
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_curve(curve, 0.5)
    _assert_failure(
        caught.value, "geometry.degenerate_geometry", source=curve.source, field="direction"
    )
    curve = replace(curve, definition=curve_3d())
    with pytest.raises(GeometryEvaluationError) as caught:
        sample_curve(curve, 0, 2, count=3)
    diagnostic = _assert_failure(
        caught.value, "geometry.parameter_out_of_range", source=curve.source
    )
    assert diagnostic.details["sample_index"] == 2


def test_cyclic_trim_and_missing_model_have_distinct_codes():
    trim = TrimmedCurve(3, ORIGIN, ORIGIN, 0, 1)
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_curve_2d(trim, 0.5)
    _assert_failure(caught.value, "geometry.missing_model", field="model")
    curve = curve_entity(3, trim)
    model = replace(model_for(), curves=(curve,))
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_curve_2d(curve, 0.5, model=model)
    _assert_failure(
        caught.value, "geometry.cyclic_reference", source=curve.source, field="basis_curve"
    )


def test_unknown_geometry_does_not_fabricate_source_context():
    with pytest.raises(GeometryEvaluationError) as caught:
        evaluate_curve(UnsupportedGeometry("unqualified"), 0.5)
    diagnostic = _assert_failure(caught.value, "geometry.unsupported_geometry")
    assert diagnostic.kind is DiagnosticKind.UNSUPPORTED
    assert "node_index" not in diagnostic.details
    assert "node_id" not in diagnostic.to_dict() and "location" not in diagnostic.to_dict()
    for error_type in (GeometryEvaluationError, AssemblyPlacementError, FacetedGeometryError):
        assert str(error_type("legacy message")) == "legacy message"


def test_assembly_reference_error_names_instance_part_and_schema():
    model, _, _ = _assembled_box()
    instance = replace(model.instances[0], part=999)
    model = replace(model, instances=(instance, model.instances[1]))
    with pytest.raises(AssemblyPlacementError) as caught:
        assembly_occurrences(model)
    diagnostic = _assert_failure(
        caught.value, "assembly.invalid_reference", source=instance.source, field="part"
    )
    assert diagnostic.schema_key == model.schema_key
    assert diagnostic.details["reference_id"] == 999


def test_occt_keeps_outer_code_and_underlying_transform_cause_before_runtime(monkeypatch):
    model, _, _ = _assembled_box()
    transform = replace(model.instances[0].transform, perspective=Vector3(1.0, 0.0, 0.0))
    instance = replace(model.instances[0], transform=transform)
    model = replace(model, instances=(instance, model.instances[1]))
    monkeypatch.setattr(conversion, "require_occt", lambda: pytest.fail("runtime was loaded"))
    with pytest.raises(OcctConversionError) as caught:
        to_occt(model, source_unit="mm", place_instances=True)
    diagnostic = caught.value.diagnostic
    assert diagnostic.code == "occt.invalid_topology"
    assert diagnostic.node_id == transform.source.node_id
    assert diagnostic.location.byte_offset == transform.source.byte_range.start
    assert diagnostic.details["cause_code"] == "assembly.unsupported_transform"
    assert diagnostic.details["field"] == "perspective"
    assert diagnostic.details["instance_path"] == "0"
    assert caught.value.report.ocp_version is None
    assert diagnostic in caught.value.report.diagnostics


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    "parameters,code,index,field",
    [
        ({"capacity": 3}, "faceted.invalid_pool", 6, "teeth"),
        ({"normal_type": 2}, "faceted.unsupported_geometry", 5, "normal_type"),
        ({"indices": (-1, 1, 2)}, "faceted.invalid_indices", 5, "position_indices"),
        ({"position_pool_link": 999}, "faceted.invalid_reference", 5, "position_pool"),
    ],
)
def test_stored_decode_errors_keep_exact_source_record_and_field(
    encoding, parameters, code, index, field
):
    document = stored_document(encoding, **parameters)
    with pytest.raises(FacetedGeometryError) as caught:
        faceted_geometry(document)
    diagnostic = _assert_failure(caught.value, code, field=field)
    node = document.nodes[index - 1]
    assert diagnostic.node_type == node.node_type
    assert diagnostic.details["node_index"] == node.index
    assert diagnostic.location.byte_offset == node.byte_range.start
    assert diagnostic.schema_key == document.schema_key.raw
    assert diagnostic.node_id is None  # Raw record indices are not invented node IDs.


def test_stored_pool_budget_and_ambiguous_indices_are_distinguishable():
    document = stored_document()
    with pytest.raises(FacetedGeometryError) as caught:
        faceted_geometry(document, max_pool_values=12)
    diagnostic = _assert_failure(caught.value, "faceted.limit_exceeded", field="n_max_vectors")
    assert diagnostic.kind is DiagnosticKind.LIMIT
    assert diagnostic.details["resource"] == "max_pool_values"
    assert diagnostic.details["observed"] == 21 and diagnostic.details["limit"] == 12
    mesh = decode_embedded_mesh(stored_document(indices=(1, 2, 3), position_count=4), 4)
    with pytest.raises(FacetedGeometryError) as caught:
        mesh.triangles()
    _assert_failure(
        caught.value, "faceted.ambiguous_index_origin", source=mesh.source, field="index_origin"
    )
    assert mesh.triangles(index_origin=0) != mesh.triangles(index_origin=1)
    with pytest.raises(FacetedGeometryError) as caught:
        decode_polyline(document, 4)
    diagnostic = _assert_failure(caught.value, "faceted.invalid_reference", field="node_index")
    assert diagnostic.details["reference_id"] == 4


def test_stored_preview_budget_and_cli_emit_structured_errors_without_writing(
    tmp_path, monkeypatch, capsys
):
    from parasolid_kit import cli

    geometry = faceted_geometry(stored_document())
    destination = tmp_path / "budget"
    with pytest.raises(FacetedGeometryError) as caught:
        write_faceted_preview(
            geometry, destination, source_unit="mm", limits=InteropLimits(max_vertices=2)
        )
    diagnostic = _assert_failure(
        caught.value, "faceted.limit_exceeded", source=geometry.meshes[0].source
    )
    assert diagnostic.details["resource"] == "max_vertices"
    assert not destination.exists()
    document = stored_document(normal_type=2)
    source = tmp_path / "stored.x_t"
    source.write_bytes(document.raw_bytes)
    monkeypatch.setattr(cli, "_document", lambda *_: document)
    assert (
        cli.main(
            [
                "viewer",
                str(source),
                "--stored-geometry",
                "--schema-dir",
                str(tmp_path),
                "--source-unit",
                "mm",
                "--write-only",
            ]
        )
        == 2
    )
    result = json.loads(capsys.readouterr().err)
    assert result["error_type"] == "FacetedGeometryError"
    assert result["diagnostic"]["code"] == "faceted.unsupported_geometry"
    assert result["diagnostic"]["details"]["field"] == "normal_type"
    assert result["diagnostic"]["details"]["node_index"] == 5
    assert not (tmp_path / "stored.parasolid-preview").exists()


def test_preview_preserves_negative_parent_source_as_structured_cause():
    pytest.importorskip("OCP")
    model, _, _ = _assembled_box(nested=True)
    negative = replace(model.instances[1], negative=True)
    model = replace(model, instances=(model.instances[0], negative, model.instances[2]))
    result = to_occt(model, source_unit="mm")
    with pytest.raises(PreviewError) as caught:
        tessellate_preview(result, model, assembly=True)
    diagnostic = caught.value.diagnostic
    assert diagnostic.code == "preview.tessellation_failed"
    assert diagnostic.details["cause_code"] == "assembly.negative_instance"
    assert diagnostic.details["field"] == "negative"
    assert diagnostic.details["node_index"] == negative.source.node_index
    assert diagnostic.node_id == negative.source.node_id
    assert diagnostic.details["instance_path"] == "1/2"


def test_structured_errors_remain_dependency_free_in_a_fresh_interpreter():
    assert_dependency_free_imports("""
import json
from parasolid_kit import (
    NurbsCurve, GeometryEvaluationError, ParasolidError, evaluate_curve_2d_many,
)
curve = NurbsCurve(1,2,2,0,False,False,False,0,((0.,0.),(1.,1.)),(0.,1.),(2,2),())
try:
    evaluate_curve_2d_many(curve,[0.0,float('nan')])
except GeometryEvaluationError as error:
    assert isinstance(error,ValueError) and isinstance(error,ParasolidError)
    assert error.diagnostic.code == 'geometry.invalid_parameter'
    assert error.diagnostic.details['sample_index'] == 1
    json.dumps(error.diagnostic.to_dict(),allow_nan=False)
else:
    raise AssertionError('invalid sample was accepted')
""")
