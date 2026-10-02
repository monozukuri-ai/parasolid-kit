"""Authored exact-key fixtures; no catalog or private iCAD input is required.

The wire has independently chosen endpoints and a 5-unit length. Surface
records follow the public XT reference, pp. 62-65 and 74-76. Only these
synthetic fixtures substitute keys to test selection; real-input validation
preserves every original key.
"""

import struct

import pytest

from parasolid_kit import (
    InMemorySchemaProvider,
    ParseError,
    ParseLimits,
    SchemaError,
    map_brep,
    parse_xb,
    parse_xt,
    write_xb,
)
from parasolid_kit.brep import NurbsSurface, Vector3
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header
from tests.test_builtin_embedded_runtime import EMBEDDED, general_body
from tests.test_builtin_spcurve_runtime import spcurve

KEYS = (
    "SCH_1500137_15003_13006",
    "SCH_1500245_15003_13006",
    "SCH_1700223_16100_13006",
    "SCH_1700256_16100_13006",
    "SCH_1901315_19008_13006",
    "SCH_2100293_20000_13006",
    "SCH_2100311_20000_13006",
    "SCH_2401260_20000_13006",
    "SCH_2800188_28002_13006",
    "SCH_2901199_28101_13006",
    "SCH_3200152_32001_13006",
    "SCH_3200252_32001_13006",
    "SCH_3301231_33103_13006",
)
# Keys under which B-spline surfaces (124-126) were observed.
SURFACE_KEYS = (KEYS[5], KEYS[6], KEYS[9], KEYS[11], KEYS[12])

# Fixed scalar order authored independently of the runtime profile declarations.
CODES = {
    12: "dppppppffpppupuuppppppp",
    13: "dpppppppp",
    16: "dpfppppppp",
    17: "pppppppppc",
    18: "dpppppfp",
    19: "dpppppc",
    29: "dppppv",
    30: "dpppppcvv",
    51: "dpppppcvvfv",
    56: "dpppppccpppffffpppp",  # Four fixed arrays are flattened here.
    59: "dpppppcnp",
    68: "dpppppcpvvvvffvf",
}


def wire_records(*, bad_reference=False):
    return [
        (12, 1, {7: 1e-6, 8: 1e-8, 14: 2, 20: 2, 21: 6, 22: 4}),
        (19, 2, {2: 1, 5: 3, 6: "V"}),
        (13, 3, {7: 2}),
        (18, 4, {4: 9, 5: 999 if bad_reference else 5, 7: 1}),
        (29, 5, {2: 4, 5: (2, -1, 3)}),
        (16, 6, {3: 7, 6: 11, 9: 1}),
        (17, 7, {4: 4, 5: 8, 6: 6, 9: "+"}),
        (17, 8, {4: 9, 5: 7, 6: 6, 9: "-"}),
        (18, 9, {5: 10, 7: 1}),
        (29, 10, {2: 9, 5: (5, 3, 3)}),
        (30, 11, {2: 6, 7: (2, -1, 3), 8: (0.6, 0.8, 0)}),
    ]


def fixture(encoding, key, records, *, user_fields=0):
    binary = SyntheticXbBuilder(schema_name=key, schema_max_type=205, user_field_size=user_fields)
    text = bytearray(text_header(key, schema_max_type=205, user_field_size=user_fields))
    seen = set()
    for kind, index, overrides in records:
        raw = bytearray(b"\xff" if kind not in seen else b"")
        text.extend(f"{kind} ".encode())
        if kind not in seen:
            text.extend(b"255 ")
        seen.add(kind)
        raw.extend(positive_integer(index))
        text.extend(f"{index} ".encode())
        defaults = {"d": index + 100, "p": 0, "f": None, "u": 0, "c": "+", "v": (0, 0, 0), "n": 0}
        for ordinal, code in enumerate(CODES[kind]):
            value = overrides.get(ordinal, defaults[code])
            if code == "v":
                raw.extend(struct.pack(">ddd", *value))
                text.extend((" ".join(str(v) for v in value) + " ").encode())
            elif code == "p":
                raw.extend(positive_integer(value))
                text.extend(f"{value} ".encode())
            elif code == "c":
                raw.extend(value.encode())
                text.extend(value.encode())
            else:
                raw.extend(
                    struct.pack(
                        ">" + {"d": "i", "n": "h", "u": "B", "f": "d"}[code],
                        -3.14158e13 if value is None else value,
                    )
                )
                text.extend(b"?" if value is None else f"{value} ".encode())
        binary.add_raw_node(kind, raw)
    text.extend(b"1 0 ")
    return bytes(text) if encoding == "x_t" else binary.build()


def parse(encoding, data, **kwargs):
    return (parse_xt if encoding == "x_t" else parse_xb)(data, **kwargs)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", KEYS)
def test_exact_keys_map_authored_wire_with_source_values(encoding, key):
    data = fixture(encoding, key, wire_records())
    doc = parse(encoding, data)
    assert doc.schema_resolution.schema_key == key
    assert doc.schema_resolution.profile_id == "icad-" + key[4:].replace("_", "-") + "-r1"
    assert doc.schema_resolution.profile_revision == 1
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert len(model.edges) == 1
    assert len(model.vertices) == 2
    assert [p.position for p in model.points] == [Vector3(2.0, -1.0, 3.0), Vector3(5.0, 3.0, 3.0)]
    assert model.metrics.bounding_box.minimum == Vector3(2.0, -1.0, 3.0)
    assert model.metrics.bounding_box.maximum == Vector3(5.0, 3.0, 3.0)
    assert [p.source.node_index for p in model.points] == [5, 10]
    assert model.curves[0].definition.direction == Vector3(0.6, 0.8, 0.0)
    if encoding == "x_b":
        assert write_xb(doc) == data
    with pytest.raises(SchemaError, match=r"schema\.missing_base_schema"):
        parse(encoding, data, schema_provider=InMemorySchemaProvider())


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_blend_and_boundary_map_existing_exact_semantics(encoding):
    records = [
        *wire_records(),
        (51, 20, {7: (1, 2, 3), 8: (0, 0, 1), 9: 7, 10: (1, 0, 0)}),
        (51, 21, {7: (1, 12, 3), 8: (0, 0, 1), 9: 3, 10: (1, 0, 0)}),
        (56, 22, {7: "R", 8: 20, 9: 21, 10: 11, 11: 2, 12: -2, 13: 1, 14: 1}),
        (59, 23, {7: 1, 8: 22}),
    ]
    doc = parse(encoding, fixture(encoding, KEYS[1], records))
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    blend, boundary = model.surfaces[2:]
    assert blend.definition.ranges == (2, -2)
    assert blend.definition.thumb_weights == (1, 1)
    assert blend.definition.supporting_surfaces == (model.surfaces[0].id, model.surfaces[1].id)
    assert blend.definition.spine_curve == model.curves[0].id
    assert boundary.definition.boundary_index == 1
    assert boundary.definition.blend_surface == blend.id
    assert boundary.source.node_type == 59


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", [KEYS[1], KEYS[3]])
def test_spun_surface_preserves_raw_parameters_and_stays_partial(encoding, key):
    values = {
        7: 11,
        8: (1, 2, 3),
        9: (0, 0, 1),
        10: (1, 2, 5),
        11: (1, 2, 9),
        12: -0.5,
        13: 2.5,
        14: (1, 0, 0),
        15: 1.25,
    }
    data = fixture(encoding, key, [*wire_records(), (68, 20, values)])
    doc = parse(encoding, data)
    node = doc.nodes[-1]
    assert [node.fields[i].values[0].value for i in range(7, 16)] == list(values.values())
    model = map_brep(doc)
    assert not model.complete and model.topology.valid
    assert model.surfaces[0].definition.type_name == "SPUN_SURF"
    assert [d.code for d in model.diagnostics] == ["geometry.unsupported_surface"]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_surface_parameter_curve_and_all_dependencies(encoding):
    data = spcurve(encoding, embedded=True).replace(EMBEDDED.encode(), KEYS[3].encode())
    model = map_brep(parse(encoding, data))
    assert model.complete
    curve = next(c for c in model.curves if c.source.node_type == 134)
    assert curve.definition.degree == 1
    assert curve.definition.control_vertices == ((0.011, -0.017), (0.023, 0.031))
    assert curve.definition.knots == (0, 1)
    assert curve.definition.knot_multiplicities == (2, 2)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", KEYS)
def test_bspline_surface_dependencies_are_scoped_to_observed_keys(encoding, key):
    vertices = (0, 0, 0, 0, 0.03, 0, 0.02, 0, 0, 0.02, 0.03, 0.002)
    patch = dict(rational=False, dimension=3, vertices=vertices)
    # SP_CURVE (137) was not observed under the 28101 key and stays unknown there.
    data = spcurve(
        encoding, embedded=True, patch=patch, include_surface_curves=key != KEYS[9]
    ).replace(EMBEDDED.encode(), key.encode())
    if key not in SURFACE_KEYS:
        with pytest.raises(SchemaError) as exc:
            parse(encoding, data)
        assert exc.value.diagnostic.code == "schema.unknown_base_type"
        return
    model = map_brep(parse(encoding, data))
    assert model.complete and model.topology.valid
    surface = model.surfaces[0].definition
    assert isinstance(surface, NurbsSurface)
    assert tuple(v for p in surface.control_vertices for v in p) == vertices
    assert surface.u_knots == surface.v_knots == (0, 1)
    assert [s.node_type for s in surface.sources] == [126, 45, 127, 127, 128, 128]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", KEYS)
def test_role_replacement_is_rejected_even_with_matching_field_name(encoding, key):
    data = general_body(encoding, EMBEDDED, replace_region=True).replace(
        EMBEDDED.encode(), key.encode()
    )
    doc = parse(encoding, data)
    with pytest.raises(ParseError) as exc:
        map_brep(doc)
    assert exc.value.diagnostic.code == "brep.invalid_field"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", KEYS)
def test_malformed_references_and_limits(encoding, key):
    doc = parse(encoding, fixture(encoding, key, wire_records(bad_reference=True)))
    with pytest.raises(ParseError):
        map_brep(doc)
    data = fixture(encoding, key, wire_records())
    with pytest.raises(ParseError):
        parse(encoding, data, limits=ParseLimits(max_nodes=2))
    with pytest.raises(ParseError) as exc:
        parse(encoding, fixture(encoding, key, wire_records(), user_fields=1))
    assert exc.value.diagnostic.code == "node.unsupported_user_fields"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_spline_arrays_truncation_limits_and_bad_links(encoding):
    data = spcurve(encoding, embedded=True).replace(EMBEDDED.encode(), KEYS[3].encode())
    doc = parse(encoding, data)
    for node in doc.nodes:
        if node.node_type in (45, 127, 128):
            for field in node.fields:
                for end in range(field.byte_range.start, field.byte_range.end):
                    with pytest.raises(ParseError):
                        parse(encoding, data[:end])
    with pytest.raises(ParseError):
        parse(encoding, data, limits=ParseLimits(max_variable_elements=1))
    for kwargs in ({"parameter": 999}, {"surface": 999}, {"replace_nurbs": True}):
        data = spcurve(encoding, embedded=True, **kwargs).replace(
            EMBEDDED.encode(), KEYS[3].encode()
        )
        with pytest.raises(ParseError):
            map_brep(parse(encoding, data))


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", KEYS)
def test_nearby_keys_and_unreviewed_types_fail_closed(encoding, key):
    for part in range(1, 4):
        parts = key.split("_")
        parts[part] = str(int(parts[part]) + 1)
        with pytest.raises(SchemaError) as exc:
            parse(encoding, fixture(encoding, "_".join(parts), wire_records()))
        assert exc.value.diagnostic.code == "schema.missing_base_schema"
    if encoding == "x_t":
        data = text_header(key, schema_max_type=205) + b"60 255 1 0 "
    else:
        data = (
            SyntheticXbBuilder(schema_name=key, schema_max_type=205)
            .add_raw_node(60, b"\xff" + positive_integer(1))
            .build()
        )
    with pytest.raises(SchemaError) as exc:
        parse(encoding, data)
    assert exc.value.diagnostic.code == "schema.unknown_base_type"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_fixed_blend_arrays_and_boundary_references_are_checked(encoding):
    records = [*wire_records(), (56, 20, {7: "R"}), (59, 21, {7: 1, 8: 999})]
    data = fixture(encoding, KEYS[1], records)
    doc = parse(encoding, data)
    # Full raw records cannot turn dangling geometry references into a complete B-Rep.
    with pytest.raises(ParseError):
        map_brep(doc)
    for field in doc.nodes[-2].fields:
        if field.definition.element_count == 2:
            for end in range(field.byte_range.start, field.byte_range.end):
                with pytest.raises(ParseError):
                    parse(encoding, data[:end])
    for boundary in ({7: 2, 8: 11}, {7: 1, 8: 999}):
        data = fixture(encoding, KEYS[1], [*wire_records(), (59, 20, boundary)])
        with pytest.raises(ParseError):
            map_brep(parse(encoding, data))
