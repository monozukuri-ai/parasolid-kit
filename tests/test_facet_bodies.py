"""Facet ("mesh") faces: MESH -> PSM_MESH -> vector and integer combs of teeth.

The facet types lie above the 13006 base's type table (184), so a stream with
base 13006 declares each of them in full, field names included. The fixtures
are authored from such declared layouts; no vendor file or definition is copied.
Corner indices are one-based, as the observed NX streams write them.
"""

from __future__ import annotations

import struct

import pytest

from parasolid_kit import (
    MeshSurface,
    ParseError,
    SchemaError,
    SurfaceKind,
    parse_xb,
    read_brep,
)
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import embedded_field, full_schema, positive_integer
from tests.test_builtin_icad_legacy_runtime import CODES as BASE_CODES
from tests.test_builtin_icad_legacy_runtime import wire_records

KEY = "SCH_3400176_34001_13006"
POSITIONS = (0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.01)
CORNERS = (1, 3, 2, 1, 2, 4, 2, 3, 4, 1, 4, 3)
LAYOUTS = {
    201: (
        "MESH",
        (
            ("node_id", "d", 0, 0),
            ("attributes_features", "p", 1019, 0),
            ("owner", "p", 1007, 0),
            ("next", "p", 1006, 0),
            ("previous", "p", 1006, 0),
            ("geometric_owner", "p", 141, 0),
            ("sense", "c", 0, 0),
            ("transform", "p", 100, 0),
            ("psm_imesh", "p", 189, 0),
        ),
    ),
    189: (
        "PSM_MESH",
        (
            ("precision", "u", 0, 0),
            ("owner", "p", 201, 0),
            ("position_pool", "p", 193, 0),
            ("position_indices", "p", 191, 0),
            ("normal_type", "u", 0, 0),
        ),
    ),
    193: (
        "VECTOR_COMB",
        (
            ("encoding", "u", 0, 0),
            ("n_vectors", "d", 0, 0),
            ("n_max_vectors", "d", 0, 0),
            ("shift", "d", 0, 0),
            ("teeth", "p", 192, 1),
        ),
    ),
    192: ("VECTOR_TOOTH", (("values", "f", 0, 1),)),
    191: (
        "INTEGER_COMB",
        (
            ("encoding", "u", 0, 0),
            ("n_integers", "d", 0, 0),
            ("n_max_integers", "d", 0, 0),
            ("n_bits_per_integer", "d", 0, 0),
            ("shift", "d", 0, 0),
            ("teeth", "p", 190, 1),
        ),
    ),
    190: ("INTEGER_TOOTH", (("values", "d", 0, 1),)),
}


def encode(code, value):
    if code == "p":
        return positive_integer(value)
    if code in "uc":
        return bytes((value,))
    return struct.pack(">d" if code == "f" else ">i", value)


def base_payload(kind, index, overrides, first):
    """A 13006 base record (the wire body): unchanged marker, then the fields."""
    payload = bytearray(b"\xff" if first else b"")
    payload += positive_integer(index)
    defaults = {"d": index + 100, "p": 0, "f": None, "u": 0, "c": "+", "v": (0, 0, 0), "n": 0}
    for ordinal, code in enumerate(BASE_CODES[kind]):
        value = overrides.get(ordinal, defaults[code])
        if code == "v":
            payload += struct.pack(">ddd", *value)
        elif code == "p":
            payload += positive_integer(value)
        elif code == "c":
            payload += value.encode()
        else:
            fmt = {"d": ">i", "n": ">h", "u": ">B", "f": ">d"}[code]
            payload += struct.pack(fmt, -3.14158e13 if value is None else value)
    return bytes(payload)


def stream(*, corners=CORNERS, transform=0, integer_teeth=(25, 26, 27, 0), extra=()):
    """A facet surface: 4 positions in one tooth, 12 corners in teeth of four."""
    records = [
        (201, 20, (1, 0, 0, 0, 0, 0, ord("+"), transform, 21)),
        (189, 21, (1, 0, 22, 24, 1)),
        # The tooth list grows ahead of the capacity; the extra entry is null.
        (193, 22, (1, 4, 4, 2, (23, 0))),
        (192, 23, (POSITIONS,)),
        (191, 24, (1, 12, 16, 32, 2, integer_teeth)),
        (190, 25, (corners[0:4],)),
        (190, 26, (corners[4:8],)),
        (190, 27, (corners[8:12],)),
        *extra,
    ]
    builder = SyntheticXbBuilder(schema_name=KEY, schema_max_type=231)
    # The mapper needs a body: the legacy wire body of the base records.
    seen = set()
    for kind, index, overrides in wire_records():
        builder.add_raw_node(kind, base_payload(kind, index, overrides, kind not in seen))
        seen.add(kind)
    declared = set()
    for kind, index, values in records:
        name, fields = LAYOUTS[kind]
        payload = bytearray()
        if kind not in declared:
            declared.add(kind)
            payload += full_schema(
                name,
                name.lower(),
                tuple(
                    embedded_field(field, field_type=code, pointer_class=klass, element_count=count)
                    for field, code, klass, count in fields
                ),
            )
        variable = [v for (_, _, _, count), v in zip(fields, values, strict=True) if count == 1]
        if variable:
            payload += struct.pack(">i", len(variable[0]))
        payload += positive_integer(index)
        for (_, code, _, count), value in zip(fields, values, strict=True):
            for item in value if count == 1 else (value,):
                payload += encode(code, item)
        builder.add_raw_node(kind, bytes(payload))
    return builder.build()


def test_facet_types_are_declared_in_full_under_a_13006_key():
    document = parse_xb(stream())
    first = {node.node_type: node.first_schema for node in document.nodes if node.first_schema}
    facets = {kind: schema for kind, schema in first.items() if kind > 184}
    assert set(facets) == set(LAYOUTS)
    assert all(schema.definition.source is SchemaSource.EMBEDDED_FULL for schema in facets.values())
    assert all(
        schema.definition.source is SchemaSource.EMBEDDED_UNCHANGED
        for kind, schema in first.items()
        if kind <= 184
    )


def test_stored_facets_map_to_a_mesh_surface():
    model = read_brep(stream()).brep
    assert model.complete
    (surface,) = model.surfaces
    assert surface.kind is SurfaceKind.MESH
    mesh = surface.definition
    assert isinstance(mesh, MeshSurface)
    assert (mesh.vertex_count, mesh.triangle_count) == (4, 4)
    assert tuple(mesh.position_array()) == POSITIONS
    assert tuple(mesh.triangle_array()) == tuple(corner - 1 for corner in CORNERS)
    assert [source.node_index for source in mesh.sources] == [21, 22, 24]


def test_placed_facets_are_retained_undecoded():
    model = read_brep(stream(transform=30)).brep
    assert not model.complete
    assert model.surfaces[0].kind is SurfaceKind.UNSUPPORTED
    assert model.diagnostics[0].code == "geometry.unsupported_surface"


@pytest.mark.parametrize(
    "variant",
    [
        {"corners": (*CORNERS[:5], 9, *CORNERS[6:])},
        {"integer_teeth": (25, 0, 27, 0)},
    ],
    ids=["corner-outside-pool", "null-stored-tooth"],
)
def test_malformed_pools_fail_closed(variant):
    with pytest.raises(ParseError, match=r"mesh|tooth"):
        read_brep(stream(**variant))


def test_membership_stays_bounded_to_the_facet_types():
    # Another type above the base table is still of unknown base membership.
    LAYOUTS[230] = ("FUTURE_RECORD", (("value", "d", 0, 0),))
    try:
        with pytest.raises(SchemaError) as captured:
            parse_xb(stream(extra=[(230, 40, (7,))]))
    finally:
        del LAYOUTS[230]
    assert captured.value.diagnostic.code == "schema.unknown_base_type"
    assert captured.value.diagnostic.node_type == 230
