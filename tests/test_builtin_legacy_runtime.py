"""The standard keys without an embedded schema: the reference's transmit example.

The public XT Format Reference prints one complete transmit file (printed page
11): a disc under ``SCH_1200000_12006`` whose face carries a colour attribute.
The schema-12006 files of current writers (SolidWorks, CAD Exchanger) list
fourteen legal owners in the attribute definition where the example lists
thirteen, so the example is authored here with fourteen under its own key, in
the 8008 layouts under the Parasolid 8 key and in the 25001 layouts under the
Autodesk Inventor key. The other keys are checked through the compiled
listing. No catalog is required and no real file is part of the tests.
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

REFERENCE_KEY = "SCH_1200000_12006"
REFERENCE_MODELLER = ": TRANSMIT FILE created by modeller version 1200000"
# The reference's example, line by line (the annotations are not part of it),
# with the fourteenth legal owner of the attribute definition.
REFERENCE_EXAMPLE = (
    "12 1 12 0 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 1 3 4 5 0 6 7 0 "
    "70 2 0 1 0 0 4 1 20 8 8 8 1 T"
    "13 3 3 0 1 0 9 0 0 6 9 "
    "50 4 11 0 9 0 0 0 +0 0 0 0 0 1 1 0 0 "
    "31 5 10 0 7 0 0 0 +0 0 0 0 0 1 1 0 0 1 "
    "19 6 5 0 1 0 0 3 V"
    "16 7 6 0 ?10 0 0 5 0 0 1 "
    "17 10 0 11 10 10 0 12 7 0 0 +"
    "15 11 7 0 10 9 0 "
    "17 12 0 0 0 0 0 10 7 0 0 -"
    "14 9 2 13 ?0 0 11 3 4 +0 0 0 0 3 "
    "81 1 13 12 14 9 0 0 0 0 15 "
    "80 1 14 0 16 8001 0 0 0 0 3 5 0 0 FFFFTFTFFFFFFF2 "
    "83 3 15 1 2 3 "
    "79 15 16 SDL/TYSA_COLOUR"
    "74 20 8 1 0 13 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 "
    "1 0 "
)
VERBATIM_OWNERS = "FFFFTFTFFFFFF2 "

# Schema 8008: BODY without ``geometry_state``, both fins without their
# attribute pointer, the list without its transmission flag and thirteen legal
# owners without field names, as SolidWorks writes Parasolid 8 transmit files.
KEY_8008 = "SCH_800000_8008"
DISC_8008 = (
    REFERENCE_EXAMPLE.replace(
        "12 1 12 0 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 1 3 4 5 0 6 7 0 ",
        "12 1 12 0 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 3 4 5 0 6 7 0 ",
    )
    .replace("70 2 0 1 0 0 4 1 20 8 8 8 1 T", "70 2 0 1 0 0 4 1 20 8 8 8 1 ")
    .replace("17 10 0 11 10 10 0 12 7 0 0 +", "17 10 11 10 10 0 12 7 0 0 +")
    .replace("17 12 0 0 0 0 0 10 7 0 0 -", "17 12 0 0 0 0 10 7 0 0 -")
    .replace("FFFFTFTFFFFFFF2 ", VERBATIM_OWNERS)
)

# Schema 25001: BODY with the owner of class 1040 and six appended fields, the
# region with its owning body, the list header and the pointer block with an
# index origin, as embedded 25001 streams declare them; the attribute
# definition keeps its field names and fourteen owners.
KEY_25001 = "SCH_2600169_25001"
DISC_25001 = (
    REFERENCE_EXAMPLE.replace(
        "12 1 12 0 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 1 3 4 5 0 6 7 0 ",
        "12 1 12 0 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 1 3 4 5 0 6 7 0 0 0 0 0 0 0 ",
    )
    .replace("70 2 0 1 0 0 4 1 20 8 8 8 1 T", "70 2 0 4 T1 0 0 1 20 1 8 8 ")
    .replace("19 6 5 0 1 0 0 3 V", "19 6 5 0 1 0 0 3 V1 ")
    .replace("FFFFTFTFFFFFFF2 ", "0 FFFFTFTFFFFFFF2 ")
    .replace("74 20 8 1 0 13 ", "74 20 8 1 0 0 13 ")
)

STANDARD = {
    KEY_8008: ("solidworks-800000-8008-r1", "8008"),
    "SCH_900141_9003": ("nx-900141-9003-r1", "9003"),
    "SCH_901297_9008": ("microstation-901297-9008-r1", "9008"),
    "SCH_1000252_10004": ("nx-1000252-10004-r2", "10004"),
    "SCH_1100129_11004": ("ironcad-1100129-11004-r1", "11004"),
    REFERENCE_KEY: ("solidworks-1200000-12006-r2", "12006"),
    "SCH_1600000_16004": ("nx-1600000-16004-r1", "16004"),
    KEY_25001: ("inventor-2600169-25001-r1", "25001"),
}


def stream(key: str, nodes: str) -> bytes:
    return text_header(key, modeller_version=REFERENCE_MODELLER) + nodes.encode()


def assert_disc(doc, *, body_fields: int, fin_fields: int, owners: int) -> None:
    assert doc.schema_resolution.coverage in ("verified_subset", "family_inferred")
    assert len(doc.nodes) == 16
    assert all(s.definition.source.value == "base" and not s.edits for s in doc.schemas)
    body = doc.nodes[0]
    assert len(body.fields) == body_fields
    names = [f.definition.name for f in body.fields]
    assert body.fields[names.index("region_head")].values[0].value == 6
    assert body.fields[names.index("body_kind")].values[0].value == 3
    fin = doc.nodes[7]
    assert (fin.node_type, fin.index, len(fin.fields)) == (17, 10, fin_fields)
    definition = doc.nodes[12]
    assert (definition.node_type, definition.index) == (80, 14)
    allowed = next(f for f in definition.fields if f.definition.name == "allowed_owners")
    flags = [v.value for v in allowed.values]
    assert len(flags) == owners
    assert [i for i, flag in enumerate(flags) if flag] == [4, 6]
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert len(model.faces) == 1 and len(model.edges) == 1 and not model.vertices
    assert model.colour_of("face", model.faces[0].id) == (1.0, 2.0, 3.0)


def test_the_reference_example_decodes_under_its_key_with_fourteen_owners():
    doc = parse_xt(stream(REFERENCE_KEY, REFERENCE_EXAMPLE))
    assert doc.schema_resolution.profile_id == "solidworks-1200000-12006-r2"
    assert doc.schema_resolution.profile_revision == 2
    assert_disc(doc, body_fields=23, fin_fields=10, owners=14)
    definition = doc.nodes[12]
    assert [f.definition.name for f in definition.fields] == [
        "next_definition",
        "identifier_ref",
        "kind_id",
        "event_actions",
        "allowed_owners",
        "value_kinds",
    ]
    # The reference's own thirteen owners leave the definition out of step.
    verbatim = REFERENCE_EXAMPLE.replace("FFFFTFTFFFFFFF2 ", VERBATIM_OWNERS)
    with pytest.raises(ParseError):
        parse_xt(stream(REFERENCE_KEY, verbatim))


def test_the_12006_profile_declares_the_observed_types_only():
    by_id = {p.profile_id: p for p in builtin_profiles()}
    assert by_id["solidworks-1200000-12006-r2"].type_count == 42
    # INT_VALUES (82) is observed under the key; UNICODE_VALUES (98) is not.
    doc = parse_xt(stream(REFERENCE_KEY, REFERENCE_EXAMPLE.replace("83 3 15 ", "82 3 15 ")))
    assert doc.nodes[13].node_type == 82
    unicode = REFERENCE_EXAMPLE.replace("83 3 15 1 2 3 ", "98 3 15 1 2 3 ")
    with pytest.raises(SchemaError, match=r"schema\.builtin_profile_uncovered_type"):
        parse_xt(stream(REFERENCE_KEY, unicode))


@pytest.mark.parametrize("key", [KEY_8008, "SCH_800194_8008"])
def test_the_example_in_the_8008_layouts_reads_under_the_parasolid_8_key(key):
    doc = parse_xt(stream(key, DISC_8008))
    expected = "solidworks-800000-8008-r1" if key == KEY_8008 else "family-8008"
    assert doc.schema_resolution.profile_id == expected
    assert_disc(doc, body_fields=22, fin_fields=9, owners=13)
    names = [f.definition.name for f in doc.nodes[0].fields]
    assert "geometry_state" not in names
    assert len(doc.nodes[1].fields) == 11
    assert "transmission_flag" not in [f.definition.name for f in doc.nodes[1].fields]


@pytest.mark.parametrize("key", [KEY_25001, "SCH_2600000_25001"])
def test_the_example_in_the_25001_layouts_reads_under_the_inventor_key(key):
    doc = parse_xt(stream(key, DISC_25001))
    expected = "inventor-2600169-25001-r1" if key == KEY_25001 else "family-25001"
    assert doc.schema_resolution.profile_id == expected
    assert_disc(doc, body_fields=29, fin_fields=10, owners=14)
    body = doc.nodes[0]
    assert [f.definition.name for f in body.fields][13] == "container_ref"
    assert [f.definition.name for f in body.fields][23:] == [
        "index_origin",
        "index_values",
        "node_id_values",
        "schema_values",
        "child_body",
        "min_local_id",
    ]
    region = doc.nodes[5]
    assert (region.node_type, len(region.fields)) == (19, 8)
    assert region.fields[7].values[0].value == 1
    assert [f.definition.name for f in doc.nodes[1].fields][:3] == [
        "local_id",
        "entry_kind",
        "transmission_flag",
    ]
    block = doc.nodes[-1]
    assert (block.node_type, block.fields[1].definition.name) == (74, "index_origin")


def binary_25001() -> bytes:
    """The 25001 disc in neutral binary, authored field by field."""
    p = positive_integer
    d = lambda value: struct.pack(">i", value)  # noqa: E731
    f = lambda value: struct.pack(">d", value)  # noqa: E731
    u = lambda value: struct.pack(">B", value)  # noqa: E731
    v = lambda x, y, z: struct.pack(">ddd", x, y, z)  # noqa: E731
    c = lambda text: text.encode()  # noqa: E731
    flags = lambda text: bytes(int(ch == "T") for ch in text)  # noqa: E731
    unset = f(-3.14158e13)
    builder = SyntheticXbBuilder(
        schema_name=KEY_25001,
        schema_max_type=None,
        user_field_size=0,
        modeller_version=REFERENCE_MODELLER,
    )
    body = d(12) + p(0) + p(2) + p(0) * 4 + f(1e3) + f(1e-8) + p(0) * 3 + u(1) + p(0) + u(3)
    body += u(1) + p(3) + p(4) + p(5) + p(0) + p(6) + p(7) + p(0)
    body += d(0) + p(0) * 4 + d(0)
    builder.add_raw_node(12, p(1) + body)
    builder.add_raw_node(
        70,
        p(2) + d(0) + u(4) + flags("T") + p(1) + p(0) * 2 + d(1) + d(20) + d(1) + p(8) * 2,
    )
    builder.add_raw_node(13, p(3) + d(3) + p(0) + p(1) + p(0) + p(9) + p(0) * 2 + p(6) + p(9))
    axes = v(0, 0, 0) + v(0, 0, 1) + v(1, 0, 0)
    builder.add_raw_node(50, p(4) + d(11) + p(0) + p(9) + p(0) * 3 + c("+") + axes)
    builder.add_raw_node(31, p(5) + d(10) + p(0) + p(7) + p(0) * 3 + c("+") + axes + f(1))
    builder.add_raw_node(19, p(6) + d(5) + p(0) + p(1) + p(0) * 2 + p(3) + c("V") + p(1))
    builder.add_raw_node(16, p(7) + d(6) + p(0) + unset + p(10) + p(0) * 2 + p(5) + p(0) * 2 + p(1))
    builder.add_raw_node(
        17, p(10) + p(0) + p(11) + p(10) * 2 + p(0) + p(12) + p(7) + p(0) * 2 + c("+")
    )
    builder.add_raw_node(15, p(11) + d(7) + p(0) + p(10) + p(9) + p(0))
    builder.add_raw_node(17, p(12) + p(0) * 5 + p(10) + p(7) + p(0) * 2 + c("-"))
    face = d(2) + p(13) + unset + p(0) * 2 + p(11) + p(3) + p(4) + c("+") + p(0) * 4 + p(3)
    builder.add_raw_node(14, p(9) + face)
    builder.add_raw_node(81, d(1) + p(13) + d(12) + p(14) + p(9) + p(0) * 4 + p(15))
    actions = b"".join(u(value) for value in (0, 0, 0, 0, 3, 5, 0, 0))
    definition = p(0) + p(16) + d(8001) + actions + p(0) + flags("FFFFTFTFFFFFFF") + u(2)
    builder.add_raw_node(80, d(1) + p(14) + definition)
    builder.add_raw_node(83, d(3) + p(15) + f(1) + f(2) + f(3))
    builder.add_raw_node(79, d(15) + p(16) + b"SDL/TYSA_COLOUR")
    builder.add_raw_node(74, d(20) + p(8) + d(1) + d(0) + p(0) + p(13) + p(0) * 19)
    return builder.build()


def test_the_25001_layouts_read_the_same_values_from_neutral_binary():
    text = parse_xt(stream(KEY_25001, DISC_25001))
    data = binary_25001()
    doc = parse_xb(data)
    assert doc.schema_resolution.profile_id == "inventor-2600169-25001-r1"
    assert [n.node_type for n in doc.nodes] == [n.node_type for n in text.nodes]
    assert [[f.values for f in n.fields] for n in doc.nodes] == [
        [f.values for f in n.fields] for n in text.nodes
    ]
    assert write_xb(doc) == data
    assert map_brep(doc).complete
    for end in range(len(data) - 1):
        with pytest.raises(ParseError):
            parse_xb(data[:end])


def test_an_attribute_field_index_without_its_node_is_an_empty_field():
    # XT reference, printed page 11: an index that names no node reads as zero.
    missing = REFERENCE_EXAMPLE.replace(
        "81 1 13 12 14 9 0 0 0 0 15 ", "81 1 13 12 14 9 0 0 0 0 99 "
    )
    model = map_brep(parse_xt(stream(REFERENCE_KEY, missing)))
    assert model.complete
    attribute = model.attributes[0]
    assert attribute.identifier == "SDL/TYSA_COLOUR"
    assert attribute.fields[0].kind == "reals"
    assert attribute.fields[0].values == ()
    assert model.colour_of("face", model.faces[0].id) is None


def test_the_standard_keys_are_listed_with_their_families():
    by_id = {p.profile_id: p for p in builtin_profiles()}
    for key, (profile_id, schema) in STANDARD.items():
        profile = by_id[profile_id]
        assert profile.schema_keys == (key,)
        assert profile.provider_schema == schema
        assert profile.coverage == "verified_subset"
        family = by_id[f"family-{schema}"]
        assert [m.profile_id for m in family.members] == [profile_id]
        assert family.base_members == ()
