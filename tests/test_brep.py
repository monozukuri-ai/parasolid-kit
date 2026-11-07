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
    PolylineCurve,
    SchemaCatalog,
    map_brep,
    parse_xb,
)
from parasolid_kit.brep import ConeSurface, MeshSurface, XtConeSurface
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
        "assemblies": [],
        "instances": [],
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


def test_native_brep_mapping_keeps_xt_cone_distinct_from_cone() -> None:
    value = _native_model()
    parameters = {
        "point": (1.0, 2.0, 3.0),
        "axis": (0.0, 0.0, 1.0),
        "radius": 4.0,
        "sin_half_angle": 0.6,
        "cos_half_angle": 0.8,
        "x_axis": (1.0, 0.0, 0.0),
    }
    value["surfaces"] = [
        {
            "id": 0,
            "sense": "positive",
            "owner": None,
            "kind": "cone",
            "parameters": parameters,
            "source": _source(52, "CONE", 2),
        },
        {
            "id": 1,
            "sense": "positive",
            "owner": None,
            "kind": "xt_cone",
            "parameters": parameters,
            "source": _source(52, "CONE", 3),
        },
    ]

    model = brep_from_native(value)

    assert isinstance(model.surfaces[0].definition, ConeSurface)
    assert isinstance(model.surfaces[1].definition, XtConeSurface)
    assert not isinstance(model.surfaces[0].definition, XtConeSurface)
    assert not isinstance(model.surfaces[1].definition, ConeSurface)


def test_native_brep_mapping_decodes_polyline_curve_fields() -> None:
    value = _native_model()
    polyline_source = _source(77, "POLYLINE", 5)
    value["curves"] = [
        {
            "id": 0,
            "sense": "negative",
            "owner": None,
            "kind": "polyline",
            "parameters": {
                "points": [(0.0, 1.0, 2.0), (3.0, 4.0, 5.0)],
                "closed": True,
                "sense": "positive",
                "base_parameter": 0.25,
                "source": polyline_source,
            },
            "source": polyline_source,
        }
    ]

    model = brep_from_native(value)

    curve = model.curves[0]
    assert isinstance(curve.definition, PolylineCurve)
    assert tuple(point.to_tuple() for point in curve.definition.points) == (
        (0.0, 1.0, 2.0),
        (3.0, 4.0, 5.0),
    )
    assert curve.definition.closed
    assert curve.definition.sense.value == "positive"
    assert curve.definition.base_parameter == 0.25
    assert curve.definition.source.node_index == 5


def test_native_brep_mapping_preserves_embedded_mesh_indices_and_null_links() -> None:
    value = _native_model()
    value["surfaces"] = [
        {
            "id": 0,
            "sense": "positive",
            "owner": None,
            "kind": "mesh",
            "parameters": {
                "source_mesh_index": 7,
                "embedded": {
                    "mesh_index": 9,
                    "transform_index": None,
                    "surface_sense": "negative",
                    "psm_mesh_index": 12,
                    "precision": 3,
                    "normal_type": 1,
                    "positions": {
                        "encoding": 2,
                        "vector_count": 3,
                        "capacity": 3,
                        "shift": 0,
                        "values": [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    },
                    "normals": None,
                    "position_indices": {
                        "integer_count": 3,
                        "capacity": 3,
                        "shift": 0,
                        "values": [1, 2, 3],
                    },
                    "normal_indices": None,
                },
                "transform_index": None,
                "transform": None,
                "unembedded_source_fields": None,
            },
            "source": _source(53, "MESH", 6),
        }
    ]

    model = brep_from_native(value)

    surface = model.surfaces[0]
    assert isinstance(surface.definition, MeshSurface)
    mesh = surface.definition
    assert mesh.source_mesh_index == 7
    assert mesh.embedded is not None
    assert mesh.embedded.mesh_index == 9
    assert mesh.embedded.psm_mesh_index == 12
    assert mesh.embedded.position_indices.values == (1, 2, 3)
    assert mesh.embedded.positions.vector_count == 3
    assert mesh.embedded.transform_index is None
    assert mesh.transform_index is None
    assert mesh.transform is None
    assert mesh.unembedded_source_fields is None


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
