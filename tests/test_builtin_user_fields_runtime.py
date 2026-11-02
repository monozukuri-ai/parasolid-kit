"""Application user fields are read with the compiled profiles.

The header's ``USFLD_SIZE`` declares integer words appended to every node, as
Parasolid 26 and 27 transmit files show (Siemens' acceptance-test examples
carry one word after value nodes too). The words are read exactly as for a
caller-supplied catalog and kept apart from the schema fields.
"""

from __future__ import annotations

import struct

import pytest

from parasolid_kit import parse_xb, parse_xt
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header

KEY = "SCH_3000000_30000"


def point_with_user_word(encoding: str, word: int = 777) -> bytes:
    """Point: local id, four references, coordinates, then one user integer."""
    if encoding == "x_t":
        return text_header(KEY, user_field_size=1) + f"29 1 4 0 0 0 0 1 2 3 {word} 1 0 ".encode()
    record = positive_integer(1) + struct.pack(">i", 4) + positive_integer(0) * 4
    record += struct.pack(">ddd", 1.0, 2.0, 3.0) + struct.pack(">i", word)
    return (
        SyntheticXbBuilder(schema_name=KEY, schema_max_type=None, user_field_size=1)
        .add_raw_node(29, record)
        .build()
    )


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_user_words_are_retained_per_visible_node(encoding):
    parser = parse_xt if encoding == "x_t" else parse_xb
    document = parser(point_with_user_word(encoding))
    assert document.header.user_field_size == 1
    assert document.schema_resolution.profile_id == "onshape-sch30000-r3"
    assert document.nodes[0].node_type == 29
    assert document.nodes[0].user_fields == (777,)
    assert document.nodes[0].to_dict()["user_fields"] == [777]
    # User words are application data, kept apart from the schema fields.
    assert [v.value for v in document.nodes[0].fields[0].values] == [4]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_value_nodes_carry_the_user_word_too(encoding):
    # REAL_VALUES (83): two doubles, then the user word, as Parasolid 27 writes it.
    if encoding == "x_t":
        data = text_header(KEY, user_field_size=1) + b"83 2 1 .5 .25 9 1 0 "
    else:
        data = (
            SyntheticXbBuilder(schema_name=KEY, schema_max_type=None, user_field_size=1)
            .add_raw_node(
                83,
                struct.pack(">i", 2) + positive_integer(1) + struct.pack(">ddi", 0.5, 0.25, 9),
            )
            .build()
        )
    document = (parse_xt if encoding == "x_t" else parse_xb)(data)
    assert document.nodes[0].node_type == 83
    assert [v.value for v in document.nodes[0].fields[0].values] == [0.5, 0.25]
    assert document.nodes[0].user_fields == (9,)
