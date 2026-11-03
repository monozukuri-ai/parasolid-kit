"""Attributes of the public reference's transmit example map to the entities they decorate.

The example (XT Format Reference, printed page 10) is a disc whose face carries
``SDL/TYSA_COLOUR``. It is written here under the 13006 key with that colour
as three reals and a second attribute, ``SDL/TYSA_NAME`` on the body, as
character values; no catalog is required.
"""

from __future__ import annotations

import pytest

from parasolid_kit import AttributeOwnerKind, map_brep, parse_xt
from tests.support.parasolid_text import text_header

KEY = "SCH_1300000_13006"
DISC = (
    "12 1 12 17 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 1 3 4 5 0 6 7 0 "
    "70 2 0 1 0 0 4 2 20 8 8 8 1 T"
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
    "80 1 14 0 16 8001 0 0 0 0 3 5 0 0 0 FFFFTFTFFFFFFF2 "
    "83 3 15 .5 .25 1 "
    "79 15 16 SDL/TYSA_COLOUR"
    "81 1 17 13 18 1 0 0 0 0 19 "
    "80 1 18 0 20 8017 0 0 0 0 3 5 0 0 0 TTTFTFFFFFFFFF3 "
    "84 5 19 Plate"
    "79 13 20 SDL/TYSA_NAME"
    "74 20 8 2 0 13 17 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 "
)


# Two more attributes on the body: an axis (two vectors) under ``MY/AXIS``,
# and a tag, two points (the second null) and an empty vector field under
# ``MY/VALUES``. The value nodes are the reference's (printed pages 106-108).
VALUES = (
    "81 1 21 14 22 1 0 0 0 0 23 "
    "80 1 22 0 24 9001 0 0 0 0 3 5 0 0 0 TTTFTFFFFFFFFF7 "
    "87 2 23 0 0 0 0 0 1 "
    "79 7 24 MY/AXIS"
    "81 3 25 15 26 1 0 0 0 0 27 28 0 "
    "80 3 26 0 29 9002 0 0 0 0 3 5 0 0 0 TTTFTFFFFFFFFF8 4 5 "
    "88 1 27 42 "
    "85 2 28 1 2 3 ?"
    "79 9 29 MY/VALUES"
)


def stream(nodes: str = DISC) -> bytes:
    return text_header(KEY) + nodes.encode() + b"1 0 "


def test_attributes_map_to_their_owners_with_their_definition_identifiers():
    doc = parse_xt(stream())
    model = map_brep(doc)
    assert model.complete
    assert [item.identifier for item in model.attributes] == ["SDL/TYSA_COLOUR", "SDL/TYSA_NAME"]
    colour, name = model.attributes
    assert colour.kind_id == 8001
    assert colour.owner.kind is AttributeOwnerKind.FACE
    assert colour.owner.id == model.faces[0].id
    assert [field.kind for field in colour.fields] == ["reals"]
    assert colour.fields[0].values == (0.5, 0.25, 1.0)
    assert colour.source.node_type == 81
    assert name.kind_id == 8017
    assert name.owner.kind is AttributeOwnerKind.BODY
    assert name.owner.id == model.bodies[0].id
    assert name.fields[0].kind == "text"
    assert name.fields[0].value == "Plate"
    assert model.faces[0].attributes == (colour.id,)
    assert model.bodies[0].attributes == (name.id,)
    assert model.attributes_of("face", model.faces[0].id) == (colour,)
    assert model.attributes_of(AttributeOwnerKind.BODY, model.bodies[0].id) == (name,)
    assert model.attributes_of("edge", model.edges[0].id) == ()
    assert model.colour_of("face", model.faces[0].id) == (0.5, 0.25, 1.0)
    assert model.colour_of("body", model.bodies[0].id) is None
    assert model.name_of("body", model.bodies[0].id) == "Plate"
    assert model.name_of("face", model.faces[0].id) is None
    as_dict = model.to_dict()
    assert as_dict["attributes"][0]["owner"] == {"kind": "face", "id": 0, "source": None}
    assert as_dict["faces"][0]["attributes"] == [0]


def test_an_attribute_on_an_unmapped_node_keeps_its_source():
    # The colour attribute now decorates the fin (node 10), which the model
    # does not map as an entity.
    doc = parse_xt(stream(DISC.replace("81 1 13 12 14 9 ", "81 1 13 12 14 10 ")))
    model = map_brep(doc)
    colour = model.attributes[0]
    assert colour.owner.kind is AttributeOwnerKind.OTHER
    assert colour.owner.id is None
    assert colour.owner.source is not None
    assert colour.owner.source.node_type == 17
    assert model.faces[0].attributes == ()


def test_an_empty_field_is_a_null_pointer_typed_by_the_definition():
    # The colour attribute's only field is empty: three reals declared, none stored.
    doc = parse_xt(
        stream(DISC.replace("81 1 13 12 14 9 0 0 0 0 15 ", "81 1 13 12 14 9 0 0 0 0 0 "))
    )
    model = map_brep(doc)
    colour = model.attributes[0]
    assert colour.fields[0].kind == "reals"
    assert colour.fields[0].values == ()
    assert model.colour_of("face", model.faces[0].id) is None


def test_vector_axis_and_tag_value_nodes_map_to_typed_fields():
    model = map_brep(parse_xt(stream(DISC + VALUES)))
    assert model.complete
    _colour, name, axis, values = model.attributes
    assert (axis.identifier, axis.kind_id, axis.owner.kind) == (
        "MY/AXIS",
        9001,
        AttributeOwnerKind.BODY,
    )
    assert [(f.kind, f.values) for f in axis.fields] == [
        ("axes", ((0.0, 0.0, 0.0), (0.0, 0.0, 1.0)))
    ]
    assert (values.identifier, values.kind_id) == ("MY/VALUES", 9002)
    assert [(f.kind, f.values) for f in values.fields] == [
        ("tags", (42,)),
        ("points", ((1.0, 2.0, 3.0), None)),
        ("vectors", ()),
    ]
    assert model.bodies[0].attributes == (name.id, axis.id, values.id)
    assert model.name_of("body", model.bodies[0].id) == "Plate"
    as_dict = model.to_dict()
    assert as_dict["attributes"][3]["fields"][1] == {
        "kind": "points",
        "values": [[1.0, 2.0, 3.0], None],
        "value": None,
        "type_name": None,
        "node_index": None,
    }


def test_an_attribute_with_a_dangling_definition_fails_the_mapping():
    from parasolid_kit import ParseError

    doc = parse_xt(stream(DISC.replace("81 1 13 12 14 9 ", "81 1 13 12 99 9 ")))
    with pytest.raises(ParseError):
        map_brep(doc)
