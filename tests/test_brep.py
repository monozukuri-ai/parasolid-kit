from __future__ import annotations

import struct

import pytest

from parasolid_kit import (
    BlendedEdgeSurface,
    BlendType,
    InMemorySchemaProvider,
    LineCurve,
    ParseError,
    PlaneSurface,
    SchemaCatalog,
    map_brep,
    parse_xb,
)
from parasolid_kit.brep._native import brep_from_native
from parasolid_kit.schema import FieldDefinition, FieldType, SchemaSource, TypeDefinition
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer


def _source(node_type: int, type_name: str, node_index: int) -> dict[str, object]:
    return {
        "node_index": node_index,
        "node_type": node_type,
        "type_name": type_name,
        "node_id": node_index * 10,
        "byte_range": (node_index * 10, node_index * 10 + 8),
    }


def _native_model() -> dict[str, object]:
    line_source = _source(75, "LINE", 2)
    plane_source = _source(89, "PLANE", 3)
    point_source = _source(88, "POINT", 4)
    return {
        "source_format": "binary",
        "schema_key": "SCH_3000000_30000",
        "complete": False,
        "bodies": [],
        "regions": [],
        "shells": [],
        "faces": [],
        "loops": [],
        "half_edges": [],
        "edges": [],
        "vertices": [],
        "points": [
            {
                "id": 0,
                "position": (1.0, 2.0, 3.0),
                "owner": None,
                "source": point_source,
            }
        ],
        "curves": [
            {
                "id": 0,
                "sense": "positive",
                "owner": None,
                "kind": "line",
                "parameters": {
                    "point": (0.0, 0.0, 0.0),
                    "direction": (1.0, 0.0, 0.0),
                },
                "source": line_source,
            }
        ],
        "surfaces": [
            {
                "id": 0,
                "sense": "negative",
                "owner": None,
                "kind": "plane",
                "parameters": {
                    "point": (0.0, 0.0, 0.0),
                    "normal": (0.0, 0.0, 1.0),
                    "x_axis": (1.0, 0.0, 0.0),
                },
                "source": plane_source,
            },
            {
                "id": 1,
                "sense": "positive",
                "owner": None,
                "kind": "blended_edge",
                "parameters": {
                    "blend_type": "rolling_ball",
                    "supporting_surfaces": (0, 0),
                    "spine_curve": 0,
                    "ranges": (0.005, -0.005),
                    "thumb_weights": (1.0, 1.0),
                    "boundary_surfaces": (None, None),
                    "start": None,
                    "end": None,
                },
                "source": _source(56, "BLENDED_EDGE", 5),
            },
        ],
        "topology": {
            "valid": True,
            "closed_loop_count": 0,
            "closed_edge_ring_count": 0,
            "euler_characteristic": 0,
        },
        "metrics": {
            "bounding_box": {
                "minimum": (1.0, 2.0, 3.0),
                "maximum": (1.0, 2.0, 3.0),
            },
            "surface_area": None,
            "volume": None,
        },
        "diagnostics": [
            {
                "code": "geometry.unsupported_curve",
                "message": "unsupported construction curve retained",
                "role": "curve",
                "source": line_source,
            }
        ],
    }


def test_native_brep_mapping_builds_typed_geometry_and_provenance() -> None:
    model = brep_from_native(_native_model())

    assert not model.complete
    assert isinstance(model.curves[0].definition, LineCurve)
    assert model.curves[0].definition.direction.to_tuple() == (1.0, 0.0, 0.0)
    assert isinstance(model.surfaces[0].definition, PlaneSurface)
    assert isinstance(model.surfaces[1].definition, BlendedEdgeSurface)
    assert model.surfaces[1].definition.blend_type is BlendType.ROLLING_BALL
    assert model.points[0].source.node_index == 4
    assert model.metrics.bounding_box is not None
    assert model.metrics.bounding_box.extents.to_tuple() == (0.0, 0.0, 0.0)
    assert model.diagnostics[0].code == "geometry.unsupported_curve"
    assert model.diagnostics[0].details["role"] == "curve"
    assert model.to_dict()["curves"][0]["kind"] == "line"  # type: ignore[index]


def test_map_brep_reports_missing_required_body_semantics_from_rust() -> None:
    definition = TypeDefinition(
        node_type=12,
        name="BODY",
        description="Deliberately incomplete body",
        variable=False,
        fields=(
            FieldDefinition("next", FieldType.POINTER_INDEX, 12, 0, True),
            FieldDefinition("value", FieldType.DOUBLE, 0, 0, True),
        ),
        source=SchemaSource.BASE,
    )
    provider = InMemorySchemaProvider((SchemaCatalog("30000", (definition,)),))
    payload = positive_integer(1) + positive_integer(0) + struct.pack(">d", 1.0)
    document = parse_xb(
        SyntheticXbBuilder(schema_name="SCH_3000000_30000", schema_max_type=None)
        .add_raw_node(12, payload)
        .build(),
        schema_provider=provider,
    )

    with pytest.raises(ParseError) as captured:
        map_brep(document)
    assert captured.value.diagnostic.code == "brep.invalid_field"
    assert captured.value.diagnostic.details == {"node_index": 1, "field": "body_type"}

    with pytest.raises(TypeError, match="ParasolidDocument"):
        map_brep(object())  # type: ignore[arg-type]


def test_native_brep_mapping_rejects_invalid_vector_shape() -> None:
    value = _native_model()
    curves = value["curves"]
    assert isinstance(curves, list)
    curve = curves[0]
    assert isinstance(curve, dict)
    parameters = curve["parameters"]
    assert isinstance(parameters, dict)
    parameters["direction"] = (1.0, 0.0)

    with pytest.raises(RuntimeError, match="three-float tuple"):
        brep_from_native(value)
