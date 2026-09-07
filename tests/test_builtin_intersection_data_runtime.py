"""Full embedded type 204 and its bounded source-reference role, without catalogs."""

import struct

import pytest

from parasolid_kit import (
    ParseError,
    ParseLimits,
    compare_documents,
    parse_xb,
    parse_xt,
    read_brep,
    write_xb,
)
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_schema import embedded_field, full_schema, positive_integer
from tests.test_builtin_embedded_runtime import BASE, EMBEDDED, builder, header
from tests.test_builtin_intersection_runtime import intersection


def data_record(encoding, *, index=10, values=(2.125, -7.25, 0.0), first=True, field_name="values"):
    if encoding == "x_t":
        schema = (
            (
                "2 17 INTERSECTION_DATA17 Intersection data7 uv_type0 0 1 u"
                f"{len(field_name)} {field_name}0 1 1 fT"
            ).encode()
            if first
            else b""
        )
        values_text = "".join("?" if v is None else f"{v} " for v in values).encode()
        return b"204 " + schema + f"{len(values)} {index} 4 ".encode() + values_text
    schema = (
        full_schema(
            "INTERSECTION_DATA",
            "Intersection data",
            (
                embedded_field("uv_type", field_type="u"),
                embedded_field(field_name, field_type="f", element_count=1),
            ),
        )
        if first
        else b""
    )
    return (
        struct.pack(">H", 204)
        + schema
        + struct.pack(">i", len(values))
        + positive_integer(index)
        + b"\x04"
        + b"".join(struct.pack(">d", -3.14158e13 if v is None else v) for v in values)
    )


def data_document(encoding, *, key=EMBEDDED, field_name="values"):
    data = header(key) if encoding == "x_t" else builder(key).build()[:-4]
    data += data_record(encoding, index=1, field_name=field_name)
    data += data_record(encoding, index=2, values=(None, 1.75), first=False)
    data += data_record(encoding, index=3, values=(), first=False)
    return data + (b"1 0 " if encoding == "x_t" else bytes([0, 1, 0, 1]))


def intersection_with_data(
    encoding,
    *,
    target=10,
    op="A",
    pointer_class=204,
    count=0,
    shifted=False,
    field_name="intersection_data",
):
    data = intersection(encoding, EMBEDDED)[:-4]
    if encoding == "x_t":
        old = b"38 255 4 4 0 0 0 0 0 +2 3 5 6 7 "
        delta = f"{13 if shifted else 12} ".encode()
        if shifted:
            delta += b"I4 note0 0 1 d"
        delta += (
            b"C" * (10 if op == "I" else 11)
            + op.encode()
            + f"{len(field_name)} {field_name}{pointer_class} {count} ".encode()
            + (b"CZ" if op == "I" else b"Z")
        )
        new = b"38 " + delta + b"4 " + (b"73 " if shifted else b"")
        new += b"4 0 0 0 0 0 +2 3 5 6 7 " + f"{target} ".encode() * max(count, 1)
    else:
        payload = positive_integer(4) + struct.pack(">i", 4) + positive_integer(0) * 5
        payload += b"+" + b"".join(positive_integer(v) for v in [2, 3, 5, 6, 7])
        old = struct.pack(">H", 38) + b"\xff" + payload
        delta = bytes([13 if shifted else 12])
        if shifted:
            delta += b"I" + embedded_field("note")
        delta += (
            b"C" * (10 if op == "I" else 11)
            + op.encode()
            + embedded_field(field_name, pointer_class=pointer_class, element_count=count)
            + (b"CZ" if op == "I" else b"Z")
        )
        if shifted:
            payload = payload[:2] + struct.pack(">i", 73) + payload[2:]
        new = struct.pack(">H", 38) + delta + payload + positive_integer(target) * max(count, 1)
    assert data.count(old) == 1
    data = data.replace(old, new, 1) + data_record(encoding)
    return data + (b"1 0 " if encoding == "x_t" else bytes([0, 1, 0, 1]))


@pytest.mark.parametrize("field_name", ["values", "source_values"])
def test_full_declaration_drives_fields_and_is_cached_without_hardcoded_layout(field_name):
    text, binary = [
        parser(data_document(enc, field_name=field_name))
        for enc, parser in [("x_t", parse_xt), ("x_b", parse_xb)]
    ]
    assert compare_documents(text, binary).equivalent
    assert write_xb(binary) == data_document("x_b", field_name=field_name)
    for doc in [text, binary]:
        assert len(doc.schemas) == 1
        assert doc.schemas[0].definition.source is SchemaSource.EMBEDDED_FULL
        assert [n.variable_length for n in doc.nodes] == [3, 2, 0]
        assert [f.definition.name for f in doc.nodes[0].fields] == ["uv_type", field_name]
        assert [v.value for v in doc.nodes[0].fields[1].values] == [2.125, -7.25, 0.0]
        assert [v.value for v in doc.nodes[1].fields[1].values] == [None, 1.75]
        assert doc.nodes[2].fields[1].values == ()
        assert doc.nodes[0].first_schema is not None and doc.nodes[1].first_schema is None


@pytest.mark.parametrize("encoding,parser", [("x_t", parse_xt), ("x_b", parse_xb)])
def test_absence_is_scoped_to_type_204_of_the_exact_embedded_profile(encoding, parser):
    with pytest.raises(ParseError) as captured:
        parser(data_document(encoding, key=BASE))
    assert captured.value.diagnostic.code == "schema.builtin_profile_uncovered_type"
    data = data_document(encoding)
    for kind in [185, 203, 205]:
        replacement = f"{kind} ".encode() if encoding == "x_t" else struct.pack(">H", kind)
        marker = b"204 " if encoding == "x_t" else struct.pack(">H", 204)
        with pytest.raises(ParseError) as captured:
            parser(data.replace(marker, replacement, 1))
        assert captured.value.diagnostic.code == "schema.unknown_base_type"
    with pytest.raises(ParseError):
        parser(data, limits=ParseLimits(max_variable_elements=2))
    with pytest.raises(ParseError):
        parser(data, limits=ParseLimits(max_fields_per_type=1))
    with pytest.raises(ParseError):
        parser(data.replace(b"SCH_3000310_30000_13006", b"SCH_3000311_30000_13006"))


@pytest.mark.parametrize("shifted", [False, True])
@pytest.mark.parametrize("target", [0, 10])
def test_appended_data_reference_survives_shift_and_null(shifted, target):
    text, binary = [
        read_brep(intersection_with_data(enc, shifted=shifted, target=target))
        for enc in ["x_t", "x_b"]
    ]
    assert compare_documents(text.document, binary.document).equivalent
    for result in [text, binary]:
        assert result.brep.complete and result.brep.topology.valid
        reference = result.brep.curves[0].definition.intersection_data
        if target:
            node = result.document.nodes[-1]
            assert reference.node_index == node.index == 10
            assert reference.byte_range == node.byte_range
            assert reference.node_type == 204
        else:
            assert reference is None


@pytest.mark.parametrize("encoding,parser", [("x_t", parse_xt), ("x_b", parse_xb)])
@pytest.mark.parametrize(
    "change", [{"op": "I"}, {"pointer_class": 41}, {"count": 2}, {"target": 5}, {"target": 99}]
)
def test_unreviewed_append_or_bad_target_does_not_gain_a_brep_role(encoding, parser, change):
    data = intersection_with_data(encoding, **change)
    parser(data)
    with pytest.raises(ParseError):
        read_brep(data)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_unrecognized_appended_name_remains_raw(encoding):
    result = read_brep(intersection_with_data(encoding, field_name="extra_data"))
    assert result.brep.curves[0].definition.intersection_data is None
    assert result.document.nodes[3].fields[-1].definition.name == "extra_data"


def test_every_truncated_full_definition_and_binary_payload_is_rejected():
    data = data_document("x_b")
    start = parse_xb(data).nodes[0].byte_range.start
    for end in range(start + 2, len(data)):
        with pytest.raises(ParseError):
            parse_xb(data[:end])
