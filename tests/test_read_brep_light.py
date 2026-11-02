"""The light read path: parse and map in Rust, materialize nodes only on request."""

import pytest

from parasolid_kit import ParseError, SchemaError, map_brep, parse_xb, parse_xt, read_brep
from tests.support.parasolid_records import records_fixture
from tests.test_builtin_assembly_runtime import CODES, assembly_records, fixture

KEY = "SCH_3701229_37102_13006"
FORMATS = {"x_t": ("x-t", parse_xt), "x_b": ("x-b", parse_xb)}


@pytest.mark.parametrize("encoding", list(FORMATS))
def test_read_brep_maps_without_materializing_nodes(encoding):
    source_format, parser = FORMATS[encoding]
    data = fixture(encoding, KEY)
    parsed = read_brep(data, source_format=source_format)
    document = parsed.document
    # The node tree stays in the native snapshot until it is asked for.
    assert document._nodes is None
    assert document.node_count == parsed.summary.node_count == 17
    assert parsed.brep == map_brep(parser(data))
    nodes = document.nodes
    assert document.nodes is nodes
    assert len(nodes) == 17 and nodes == parser(data).nodes
    assert len(document.to_dict()["nodes"]) == 17
    assert document.raw_bytes == data


@pytest.mark.parametrize("encoding", list(FORMATS))
def test_parsed_documents_materialize_nodes_lazily(encoding):
    _, parser = FORMATS[encoding]
    document = parser(fixture(encoding, KEY))
    assert document._nodes is None and document.node_count == 17
    assert [node.node_type for node in document.nodes[:3]] == [12, 19, 13]
    assert document._nodes is not None


@pytest.mark.parametrize("encoding", list(FORMATS))
def test_read_brep_errors_keep_their_stage(encoding):
    source_format, _ = FORMATS[encoding]
    data = fixture(encoding, KEY)
    other_schema = data.replace(b"SCH_3701229_37102_13006", b"SCH_3701229_37103_13006")
    with pytest.raises(SchemaError) as schema_error:
        read_brep(other_schema, source_format=source_format)
    assert schema_error.value.diagnostic.code == "schema.missing_base_schema"
    # The instance's part is a POINT.
    with pytest.raises(ParseError) as brep_error:
        read_brep(fixture(encoding, KEY, {13: {3: 5}}), source_format=source_format)
    assert brep_error.value.diagnostic.code == "brep.invalid_reference"
    structure_only = records_fixture(encoding, KEY, assembly_records(), CODES, embedded=True)
    with pytest.raises(ParseError) as missing_body:
        read_brep(structure_only, source_format=source_format)
    assert missing_body.value.diagnostic.code == "brep.missing_body"
