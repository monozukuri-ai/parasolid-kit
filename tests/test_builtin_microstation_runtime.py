"""Authored fixture for the MicroStation modeler key ``SCH_901297_9008``.

The public XT Format Reference's transmit example (printed page 10, a disc
under schema 12006) is written here in the layouts the public Bentley streams
of Parasolid 9 show: BODY without ``nom_geom_state``, FIN without its attribute
pointer, and ATTRIB_DEF without field names and with thirteen legal owners. The
real values become integer values, the only value node those streams carry. No
catalog is required and the Bentley files are not part of the tests.
"""

from __future__ import annotations

import struct

import pytest

from parasolid_kit import (
    ParseError,
    SchemaError,
    builtin_profiles,
    map_brep,
    parse_xb,
    parse_xt,
    write_xb,
)
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header

KEY = "SCH_901297_9008"
PROFILE = "microstation-901297-9008-r1"
MODELLER = ": TRANSMIT FILE created by modeller version 901297"
DISC = (
    "12 1 12 0 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 3 4 5 0 6 7 0 "
    "70 2 0 1 0 0 4 1 20 8 8 8 1 T"
    "13 3 3 0 1 0 9 0 0 6 9 "
    "50 4 11 0 9 0 0 0 +0 0 0 0 0 1 1 0 0 "
    "31 5 10 0 7 0 0 0 +0 0 0 0 0 1 1 0 0 1 "
    "19 6 5 0 1 0 0 3 V"
    "16 7 6 0 ?10 0 0 5 0 0 1 "
    "17 10 11 10 10 0 12 7 0 0 +"
    "15 11 7 0 10 9 0 "
    "17 12 0 0 0 0 10 7 0 0 -"
    "14 9 2 13 ?0 0 11 3 4 +0 0 0 0 3 "
    "81 1 13 12 14 9 0 0 0 0 15 "
    "80 1 14 0 16 8001 0 0 0 0 3 5 0 0 FFFFTFTFFFFFF1 "
    "82 3 15 1 2 3 "
    "79 15 16 SDL/TYSA_COLOUR"
    "74 20 8 1 0 13 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 "
)


def stream(key: str = KEY, nodes: str = DISC) -> bytes:
    return text_header(key, modeller_version=MODELLER) + nodes.encode() + b"1 0 "


def test_exact_key_reads_the_reshaped_reference_example():
    doc = parse_xt(stream())
    resolution = doc.schema_resolution
    assert resolution.schema_key == KEY
    assert resolution.profile_id == PROFILE
    assert resolution.profile_revision == 1
    assert resolution.coverage == "verified_subset"
    assert all(s.definition.source.value == "base" and not s.edits for s in doc.schemas)
    assert len(doc.nodes) == 16

    body = doc.nodes[0]
    names = [f.definition.name for f in body.fields]
    assert len(names) == 22
    assert "geometry_state" not in names
    assert names[14:17] == ["body_kind", "legacy_shell", "boundary_surfaces"]
    assert body.fields[14].values[0].value == 3
    assert body.fields[19].definition.name == "region_head"
    assert body.fields[19].values[0].value == 6

    fin = doc.nodes[7]
    assert (fin.node_type, fin.index) == (17, 10)
    assert [f.definition.name for f in fin.fields][:2] == ["loop_ref", "forward_fin"]
    assert len(fin.fields) == 9

    definition = doc.nodes[12]
    assert (definition.node_type, definition.index) == (80, 14)
    assert [f.definition.name for f in definition.fields] == [
        "next_definition",
        "identifier_ref",
        "kind_id",
        "event_actions",
        "allowed_owners",
        "value_kinds",
    ]
    owners = [v.value for v in definition.fields[4].values]
    assert len(owners) == 13
    assert [i for i, flag in enumerate(owners) if flag] == [4, 6]
    assert [v.value for v in definition.fields[5].values] == [1]

    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert len(model.bodies) == 1
    assert len(model.faces) == 1
    assert len(model.edges) == 1
    assert len(model.vertices) == 0
    assert model.curves[0].definition.radius == 1.0


def binary() -> bytes:
    """The same disc in neutral binary, authored field by field."""
    p = positive_integer
    d = lambda value: struct.pack(">i", value)  # noqa: E731
    f = lambda value: struct.pack(">d", value)  # noqa: E731
    u = lambda value: struct.pack(">B", value)  # noqa: E731
    v = lambda x, y, z: struct.pack(">ddd", x, y, z)  # noqa: E731
    c = lambda text: text.encode()  # noqa: E731
    flags = lambda text: bytes(int(ch == "T") for ch in text)  # noqa: E731
    unset = f(-3.14158e13)
    builder = SyntheticXbBuilder(
        schema_name=KEY, schema_max_type=None, user_field_size=0, modeller_version=MODELLER
    )
    body = d(12) + p(0) + p(2) + p(0) * 4 + f(1e3) + f(1e-8) + p(0) * 3 + u(1) + p(0) + u(3)
    body += p(3) + p(4) + p(5) + p(0) + p(6) + p(7) + p(0)
    builder.add_raw_node(12, p(1) + body)
    builder.add_raw_node(
        70,
        p(2) + d(0) + p(1) + p(0) * 2 + d(4) + d(1) + d(20) + d(8) + p(8) * 2 + d(1) + flags("T"),
    )
    builder.add_raw_node(13, p(3) + d(3) + p(0) + p(1) + p(0) + p(9) + p(0) * 2 + p(6) + p(9))
    axes = v(0, 0, 0) + v(0, 0, 1) + v(1, 0, 0)
    builder.add_raw_node(50, p(4) + d(11) + p(0) + p(9) + p(0) * 3 + c("+") + axes)
    builder.add_raw_node(31, p(5) + d(10) + p(0) + p(7) + p(0) * 3 + c("+") + axes + f(1))
    builder.add_raw_node(19, p(6) + d(5) + p(0) + p(1) + p(0) * 2 + p(3) + c("V"))
    builder.add_raw_node(16, p(7) + d(6) + p(0) + unset + p(10) + p(0) * 2 + p(5) + p(0) * 2 + p(1))
    builder.add_raw_node(17, p(10) + p(11) + p(10) * 2 + p(0) + p(12) + p(7) + p(0) * 2 + c("+"))
    builder.add_raw_node(15, p(11) + d(7) + p(0) + p(10) + p(9) + p(0))
    builder.add_raw_node(17, p(12) + p(0) * 4 + p(10) + p(7) + p(0) * 2 + c("-"))
    face = d(2) + p(13) + unset + p(0) * 2 + p(11) + p(3) + p(4) + c("+") + p(0) * 4 + p(3)
    builder.add_raw_node(14, p(9) + face)
    builder.add_raw_node(81, d(1) + p(13) + d(12) + p(14) + p(9) + p(0) * 4 + p(15))
    actions = b"".join(u(value) for value in (0, 0, 0, 0, 3, 5, 0, 0))
    definition = p(0) + p(16) + d(8001) + actions + flags("FFFFTFTFFFFFF") + u(1)
    builder.add_raw_node(80, d(1) + p(14) + definition)
    builder.add_raw_node(82, d(3) + p(15) + d(1) + d(2) + d(3))
    builder.add_raw_node(79, d(15) + p(16) + b"SDL/TYSA_COLOUR")
    builder.add_raw_node(74, d(20) + p(8) + d(1) + p(0) + p(13) + p(0) * 19)
    return builder.build()


def test_the_layouts_read_the_same_values_from_neutral_binary():
    doc = parse_xt(stream())
    again = parse_xb(binary())
    assert again.schema_resolution.profile_id == PROFILE
    assert [n.node_type for n in again.nodes] == [n.node_type for n in doc.nodes]
    assert [[f.values for f in n.fields] for n in again.nodes] == [
        [f.values for f in n.fields] for n in doc.nodes
    ]
    assert write_xb(again) == binary()
    assert map_brep(again).complete


def test_other_builds_of_schema_9008_read_through_the_family():
    doc = parse_xt(stream(key="SCH_900000_9008"))
    assert doc.schema_resolution.profile_id == "family-9008"
    assert doc.schema_resolution.coverage == "family_inferred"
    assert doc.schema_resolution.schema_key == "SCH_900000_9008"
    assert map_brep(doc).complete


def test_builtin_listing_has_the_profile_and_its_family():
    by_id = {p.profile_id: p for p in builtin_profiles()}
    profile = by_id[PROFILE]
    assert profile.schema_keys == (KEY,)
    assert profile.provider_schema == "9008"
    assert profile.coverage == "verified_subset"
    assert profile.type_count == 27
    family = by_id["family-9008"]
    assert [m.profile_id for m in family.members] == [PROFILE]
    assert family.base_members == ()
    assert family.type_count == 27


def test_other_layouts_and_unobserved_types_fail_closed():
    # The V13 attribute definition carries field names and fourteen owners.
    v13 = DISC.replace("FFFFTFTFFFFFF1 ", "0 FFFFTFTFFFFFFF1 ")
    with pytest.raises(ParseError):
        parse_xt(stream(nodes=v13))
    # REAL_VALUES (83) is not observed in the Bentley files.
    reals = DISC.replace("82 3 15 1 2 3 ", "83 3 15 1 2 3 ")
    with pytest.raises(SchemaError, match=r"schema\.builtin_profile_uncovered_type"):
        parse_xt(stream(nodes=reals))
    data = stream()
    for end in range(len(data) - 1):
        with pytest.raises(ParseError):
            parse_xt(data[:end])
