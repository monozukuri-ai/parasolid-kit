"""FIELD_NAMES (99) over base 13006, XT reference printed pp. 102-103 and 106.

An attribute definition may name its fields: ``field_names`` points at one
variable-length node of ``CHAR_VALUES`` or ``UNICODE_VALUES`` pointers, one per
field. NX writes it with the unchanged base declaration. The records are
authored independently of the compiled profile definitions.
"""

import struct

import pytest

from parasolid_kit import ParseError, compare_documents, parse_xb, parse_xt, write_xb
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header

# A key of the NX transmit profiles and another build of a SolidWorks schema.
KEYS = ("SCH_3400176_34001_13006", "SCH_3601193_36001_13006")
OWNERS = "FFTFFFFFFFFFFF"
TEXT = (
    f"80 255 2 10 0 11 9000 0 0 0 0 3 5 0 0 12 {OWNERS}2 3 "
    "99 255 2 12 13 14 "
    "84 255 6 13 volume"
    "84 4 14 note"
    "79 255 12 11 NX/mass_info"
)


def text_stream(key):
    return text_header(key, schema_max_type=231) + TEXT.encode() + b"1 0 "


def binary_stream(key):
    p = positive_integer
    builder = SyntheticXbBuilder(schema_name=key, schema_max_type=231)
    actions = bytes((0, 0, 0, 0, 3, 5, 0, 0))
    owners = bytes(int(flag == "T") for flag in OWNERS)
    builder.add_raw_node(
        80,
        b"\xff"
        + struct.pack(">i", 2)
        + p(10)
        + p(0)
        + p(11)
        + struct.pack(">i", 9000)
        + actions
        + p(12)
        + owners
        + bytes((2, 3)),
    )
    builder.add_raw_node(99, b"\xff" + struct.pack(">i", 2) + p(12) + p(13) + p(14))
    builder.add_raw_node(84, b"\xff" + struct.pack(">i", 6) + p(13) + b"volume")
    builder.add_raw_node(84, struct.pack(">i", 4) + p(14) + b"note")
    builder.add_raw_node(79, b"\xff" + struct.pack(">i", 12) + p(11) + b"NX/mass_info")
    return builder.build()


@pytest.mark.parametrize("key", KEYS)
def test_field_names_read_with_the_unchanged_base_declaration(key):
    xt = parse_xt(text_stream(key))
    data = binary_stream(key)
    xb = parse_xb(data)
    assert compare_documents(xt, xb).field_values_equal
    assert write_xb(xb) == data
    for doc in (xt, xb):
        definition, names = doc.nodes[0], doc.nodes[1]
        assert definition.fields[4].definition.name == "field_labels"
        assert definition.fields[4].values[0].value == 12
        assert (names.node_type, names.index, names.variable_length) == (99, 12, 2)
        assert [f.definition.name for f in names.fields] == ["names"]
        assert names.fields[0].definition.pointer_class == 1037
        assert [v.value for v in names.fields[0].values] == [13, 14]
        assert names.first_schema.definition.source == SchemaSource.EMBEDDED_UNCHANGED
        texts = ["".join(chr(v.value) for v in node.fields[0].values) for node in doc.nodes[2:4]]
        assert texts == ["volume", "note"]


def test_a_truncated_field_names_record_fails_closed():
    data = text_stream(KEYS[0])
    doc = parse_xt(data)
    for end in range(doc.nodes[1].byte_range.start, doc.nodes[1].byte_range.end):
        with pytest.raises(ParseError):
            parse_xt(data[:end])
