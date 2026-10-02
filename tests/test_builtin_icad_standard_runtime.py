"""Authored fixtures for exact non-embedded iCAD keys; no catalog is required.

A standard key transmits no schema, so every record follows the field order
stated here. Codes and ordinals are authored independently of the runtime
profile declarations; only these synthetic fixtures choose the keys.
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
from parasolid_kit.brep import Vector3
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header

# BODY codes, then the ordinals of its two precisions, kind and three heads.
BODIES = {
    "SCH_1300218_13006": ("dppppppffpppupuuppppppp", (7, 8, 14, 20, 21, 22)),
    "SCH_1302234_13006": ("dppppppffpppupuuppppppp", (7, 8, 14, 20, 21, 22)),
    "SCH_1500000_15003": ("dppppppffpppupuuppppppp", (7, 8, 14, 20, 21, 22)),
    "SCH_1700000_16100": ("dppppppffpppupuuppppppp", (7, 8, 14, 20, 21, 22)),
    "SCH_1901000_19008": ("dppppppffpppupuupppppppdppp", (7, 8, 14, 20, 21, 22)),
    "SCH_2401000_20000": ("dppppppffpppupuupppppppdppp", (7, 8, 14, 20, 21, 22)),
    "SCH_2601000_26105": ("dppppppffpppupuuppppppppdppppd", (7, 8, 14, 20, 21, 22)),
    "SCH_2800000_28002": ("dppppppffpppupuuppppppppdppppd", (7, 8, 14, 20, 21, 22)),
    "SCH_2901000_28101": ("dppppppppffpppupuupppppppppdppppd", (9, 10, 16, 24, 25, 26)),
    "SCH_3200000_32001": ("dppppppppffpppupuupppppppppdppppdp", (9, 10, 16, 24, 25, 26)),
    "SCH_3301000_33103": ("dpppppppppffpppupuupppppppppdppppdp", (10, 11, 17, 25, 26, 27)),
}
KEYS = tuple(BODIES)
# The list header keeps its base order only under the two 13006 keys; REGION
# gains its owner from 26105 on.
BASE_LIST = KEYS[:2]
BASE_REGION = KEYS[:6]

CODES = {
    13: "dpppppppp",
    16: "dpfppppppp",
    17: "pppppppppc",
    18: "dpppppfp",
    29: "dppppv",
    30: "dpppppcvv",
}


def codes(key, kind):
    if kind == 12:
        return BODIES[key][0]
    if kind == 19:
        return "dpppppc" if key in BASE_REGION else "dpppppcp"
    if kind == 70:
        return "dpppddddppdl" if key in BASE_LIST else "dulpppdddpp"
    return CODES[kind]


def wire_records(key, *, bad_reference=False):
    size, linear, body_kind, region, edge, vertex = BODIES[key][1]
    return [
        (12, 1, {size: 1e-6, linear: 1e-8, body_kind: 2, region: 2, edge: 6, vertex: 4}),
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
    binary = SyntheticXbBuilder(schema_name=key, schema_max_type=None, user_field_size=user_fields)
    text = bytearray(text_header(key, user_field_size=user_fields))
    for kind, index, overrides in records:
        raw = bytearray(positive_integer(index))
        text.extend(f"{kind} {index} ".encode())
        defaults = {"d": index + 100, "p": 0, "f": None, "u": 0, "c": "+", "v": (0, 0, 0), "l": 0}
        for ordinal, code in enumerate(codes(key, kind)):
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
            elif code == "l":
                raw.append(value)
                text.extend(b"T" if value else b"F")
            else:
                raw.extend(
                    struct.pack(
                        ">" + {"d": "i", "u": "B", "f": "d"}[code],
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
    data = fixture(encoding, key, wire_records(key))
    doc = parse(encoding, data)
    assert doc.schema_resolution.schema_key == key
    assert doc.schema_resolution.profile_id == "icad-" + key[4:].replace("_", "-") + "-r1"
    assert doc.schema_resolution.profile_revision == 1
    assert all(s.definition.source.value == "base" and not s.edits for s in doc.schemas)
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
@pytest.mark.parametrize("key", KEYS)
def test_body_references_follow_the_revision_layout(encoding, key):
    # Every BODY ordinal gets its own value; a shifted layout cannot agree.
    layout, roles = BODIES[key]
    values = {
        ordinal: {"d": 500 + ordinal, "p": 40 + ordinal, "f": ordinal + 0.5, "u": ordinal}[code]
        for ordinal, code in enumerate(layout)
    }
    body = parse(encoding, fixture(encoding, key, [(12, 1, values)])).nodes[0]
    assert [f.values[0].value for f in body.fields] == list(values.values())
    names = [f.definition.name for f in body.fields]
    assert [names.index(n) for n in ("size_precision", "linear_precision", "body_kind")] == list(
        roles[:3]
    )
    assert [names.index(n) for n in ("region_head", "edge_head", "vertex_head")] == list(roles[3:])


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", KEYS)
def test_list_header_uses_the_revision_layout(encoding, key):
    # The logical flag is last in the base order and third in the later one.
    flag = 11 if key in BASE_LIST else 2
    values = [321, 9, 8, 4, 5, 6, 77, 88, 99, 10, 11, 12][: len(codes(key, 70))]
    values[flag] = 1
    record = (70, 20, dict(enumerate(values)))
    doc = parse(encoding, fixture(encoding, key, [*wire_records(key), record]))
    expected = [*values]
    expected[flag] = True
    assert [f.values[0].value for f in doc.nodes[-1].fields] == expected
    assert map_brep(doc).complete


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", KEYS)
def test_malformed_references_limits_and_user_fields(encoding, key):
    doc = parse(encoding, fixture(encoding, key, wire_records(key, bad_reference=True)))
    with pytest.raises(ParseError):
        map_brep(doc)
    data = fixture(encoding, key, wire_records(key))
    with pytest.raises(ParseError):
        parse(encoding, data, limits=ParseLimits(max_nodes=2))
    for end in range(len(data) - 1):
        with pytest.raises(ParseError):
            parse(encoding, data[:end])
    with pytest.raises(ParseError):
        parse(encoding, fixture(encoding, key, wire_records(key), user_fields=1))


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", KEYS)
def test_nearby_keys_and_unreviewed_types_fail_closed(encoding, key):
    for part in (1, 2):
        parts = key.split("_")
        parts[part] = str(int(parts[part]) + 1)
        with pytest.raises(SchemaError) as exc:
            parse(encoding, fixture(encoding, "_".join(parts), []))
        assert exc.value.diagnostic.code == "schema.missing_base_schema"
    # Swept and foreign surfaces were not seen under any of these keys.
    for kind in (67, 69):
        if encoding == "x_t":
            data = text_header(key) + f"{kind} 1 0 ".encode()
        else:
            data = (
                SyntheticXbBuilder(schema_name=key, schema_max_type=None)
                .add_raw_node(kind, positive_integer(1))
                .build()
            )
        with pytest.raises(SchemaError) as exc:
            parse(encoding, data)
        assert exc.value.diagnostic.code == "schema.builtin_profile_uncovered_type"
