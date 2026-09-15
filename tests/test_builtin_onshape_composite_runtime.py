"""Current-key wrapper roles and full-only transmit records, using independent wires."""

import struct

import pytest

from parasolid_kit import ParseError, compare_documents, parse_xb, parse_xt, read_brep
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import embedded_field, full_schema, positive_integer
from tests.support.parasolid_text import text_header
from tests.test_builtin_embedded_runtime import EMBEDDED
from tests.test_builtin_intersection_data_runtime import intersection_with_data
from tests.test_builtin_onshape_current_runtime import KEY, PROFILE
from tests.test_builtin_spcurve_runtime import spcurve
from tests.test_builtin_trimmed_runtime import trimmed


@pytest.mark.parametrize("family", ["intersection", "trimmed", "surface_parametric"])
def test_current_wrappers_preserve_independent_wire_values(family):
    def fixture(encoding):
        if family == "intersection":
            data = intersection_with_data(encoding)
        elif family == "trimmed":
            data = trimmed(encoding, edits="shift")
        else:
            data = spcurve(encoding, embedded=True, cylinder=True)
        return data.replace(EMBEDDED.encode(), KEY.encode())

    results = [read_brep(fixture(encoding)) for encoding in ("x_t", "x_b")]
    assert compare_documents(results[0].document, results[1].document).equivalent
    for result in results:
        assert result.document.schema_resolution.to_dict() == PROFILE
        assert result.brep.complete and result.brep.topology.valid
        definitions = [c.definition for c in result.brep.curves if c.kind == family]
        assert definitions
        if family == "intersection":
            assert definitions[0].intersection_data.node_type == 204
            assert [tuple(p) for p in definitions[0].chart_points] == [
                (-0.011, 0, 0),
                (0.019, 0, 0),
            ]
        elif family == "trimmed":
            assert definitions[0].start_parameter == -0.013
            assert definitions[0].end_parameter == 0.027
        else:
            assert definitions[0].original_curve is None
            assert definitions[0].tolerance_to_original is None


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("family", ["trimmed", "surface_parametric", "intersection_data"])
def test_current_wrappers_reject_replaced_roles_and_wrong_data_class(encoding, family):
    if family == "trimmed":
        data = trimmed(encoding, edits="replace")
    elif family == "surface_parametric":
        data = spcurve(encoding, embedded=True, replace_nurbs=True)
    else:
        data = intersection_with_data(encoding, pointer_class=45)
    data = data.replace(EMBEDDED.encode(), KEY.encode())
    with pytest.raises(ParseError):
        read_brep(data)


def part_block(encoding):
    if encoding == "x_t":
        declaration = (
            b"176 6 14 PART_XMT_BLOCK9 Part list9 n_entries0 0 1 d"
            b"16 index_map_offset0 0 1 d9 index_map82 0 "
            b"20 schema_embedding_map82 0 16 mesh_offset_data206 0 7 entries1005 1 T"
        )
        return text_header(KEY, schema_max_type=205) + declaration + b"2 1 2 0 0 0 0 2 3 1 0 "
    declaration = full_schema(
        "PART_XMT_BLOCK",
        "Part list",
        (
            embedded_field("n_entries"),
            embedded_field("index_map_offset"),
            embedded_field("index_map", pointer_class=82),
            embedded_field("schema_embedding_map", pointer_class=82),
            embedded_field("mesh_offset_data", pointer_class=206),
            embedded_field("entries", pointer_class=1005, element_count=1),
        ),
    )
    payload = declaration + struct.pack(">i", 2) + positive_integer(1)
    payload += (
        struct.pack(">ii", 2, 0)
        + positive_integer(0) * 3
        + positive_integer(2)
        + positive_integer(3)
    )
    return (
        SyntheticXbBuilder(schema_name=KEY, schema_max_type=205).add_raw_node(176, payload).build()
    )


def test_part_transmit_block_uses_full_source_declaration_and_retains_entries():
    text, binary = parse_xt(part_block("x_t")), parse_xb(part_block("x_b"))
    assert compare_documents(text, binary).equivalent
    for document in (text, binary):
        node = document.nodes[0]
        assert node.definition.source == SchemaSource.EMBEDDED_FULL
        assert [v.value for v in node.fields[-1].values] == [2, 3]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("kind", [176, 204])
def test_absent_types_cannot_use_unchanged_or_copy_base_declarations(encoding, kind):
    for marker in [b"255 ", b"1 CZ"] if encoding == "x_t" else [b"\xff", b"\x01CZ"]:
        data = (
            text_header(KEY, schema_max_type=205) + f"{kind} ".encode() + marker + b"1 1 0 "
            if encoding == "x_t"
            else SyntheticXbBuilder(schema_name=KEY, schema_max_type=205)
            .add_raw_node(kind, marker + b"\x01")
            .build()
        )
        with pytest.raises(ParseError) as captured:
            (parse_xt if encoding == "x_t" else parse_xb)(data)
        # Confirmed absence selects the full-declaration grammar. These base
        # edit markers consequently fail while reading that declaration.
        assert captured.value.diagnostic.code == (
            "text.invalid_token" if encoding == "x_t" else "binary.truncated_field"
        )
