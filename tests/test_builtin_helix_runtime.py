"""Issue #13: public-reference HELIX_CU_FORM, XT reference pp. 43-44, 118.

The descriptor is seven field groups (vvvciff), without a geometry header.
Fixtures are authored independently of the compiled profile definitions.
"""

import importlib.util
import json
import struct

import pytest

from parasolid_kit import (
    ParseError,
    SchemaError,
    compare_documents,
    parse_xb,
    parse_xt,
    read_brep,
    write_xb,
)
from parasolid_kit.brep import NurbsCurve
from parasolid_kit.cli import main
from parasolid_kit.schema import SchemaSource
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import embedded_field, positive_integer
from tests.support.parasolid_text import text_header
from tests.test_builtin_assembly_runtime import fixture as assembly_fixture
from tests.test_builtin_embedded_runtime import EMBEDDED
from tests.test_builtin_spcurve_runtime import spcurve

KEY = "SCH_3601228_36001_13006"
KEYS = (KEY, "SCH_3601299_36001_13006", "SCH_3701229_37102_13006")
AXIS_POINT = (0.013, -0.017, 0.019)
AXIS_DIRECTION = (0.0, 0.0, 1.0)
POINT = (0.023, -0.017, 0.019)


def helix_records(encoding, *, schema="unchanged", turns=(-0.25, 3.5), user_fields=0):
    """A CURVE_DATA link, two descriptors and a following integer-values node."""
    # X_T encodes a null composite with one '?'; X_B keeps both doubles.
    wire_turns = turns[:1] if turns == (None, None) and schema != "split" else turns
    if encoding == "x_t":
        marker = {
            "unchanged": b"255 ",
            "copy": b"7 CCCCCCCZ",
            # Delete the interval and insert two doubles with identical wire values.
            "split": b"8 CCCCDI5 start0 0 1 fI3 end0 0 1 fCCZ",
            "bad_count": b"8 CCCCCCCZ",
        }[schema]
        data = bytearray(b"135 255 40 1 41 ")
        if user_fields:
            data.extend(b"-401 ")
        for index, hand in ((41, "+"), (42, "-")):
            data.extend(b"163 " + (marker if index == 41 else b""))
            data.extend(f"{index} ".encode())
            data.extend(" ".join(map(str, (*AXIS_POINT, *AXIS_DIRECTION, *POINT))).encode())
            data.extend(b" ")
            data.extend(hand.encode())
            data.extend(b"".join(b"?" if v is None else f"{v} ".encode() for v in wire_turns))
            data.extend(b"0.007 1e-8 ")
            if user_fields:
                data.extend(f"{-index} ".encode())
        data.extend(b"82 255 2 43 -7 123456 ")
        if user_fields:
            data.extend(b"-43 ")
        return bytes(data)
    marker = {
        "unchanged": b"\xff",
        "copy": b"\x07CCCCCCCZ",
        "split": (
            b"\x08CCCCDI"
            + embedded_field("start", field_type="f")
            + b"I"
            + embedded_field("end", field_type="f")
            + b"CCZ"
        ),
        "bad_count": b"\x08CCCCCCCZ",
    }[schema]
    data = bytearray(struct.pack(">HB", 135, 255) + positive_integer(40))
    data.extend(b"\x01" + positive_integer(41))
    if user_fields:
        data.extend(struct.pack(">i", -401))
    for index, hand in ((41, b"+"), (42, b"-")):
        data.extend(struct.pack(">H", 163) + (marker if index == 41 else b""))
        data.extend(positive_integer(index))
        data.extend(struct.pack(">9d", *AXIS_POINT, *AXIS_DIRECTION, *POINT) + hand)
        for value in turns:
            data.extend(struct.pack(">d", value if value is not None else -3.14158e13))
        data.extend(struct.pack(">2d", 0.007, 1e-8))
        if user_fields:
            data.extend(struct.pack(">i", -index))
    data.extend(struct.pack(">HBi", 82, 255, 2) + positive_integer(43))
    data.extend(struct.pack(">2i", -7, 123456))
    if user_fields:
        data.extend(struct.pack(">i", -43))
    return bytes(data)


def helix_document(encoding, key=KEY, **options):
    user_fields = options.get("user_fields", 0)
    header = (
        text_header(key, schema_max_type=231, user_field_size=user_fields)
        if encoding == "x_t"
        else SyntheticXbBuilder(
            schema_name=key, schema_max_type=231, user_field_size=user_fields
        ).header()
    )
    terminator = b"1 0 " if encoding == "x_t" else b"\x00\x01\x00\x01"
    return header + helix_records(encoding, **options) + terminator


@pytest.mark.parametrize("key", KEYS)
@pytest.mark.parametrize("schema", ["unchanged", "copy", "split"])
@pytest.mark.parametrize("turns", [(-0.25, 3.5), (None, None)])
def test_helix_fields_cached_schema_and_following_record(key, schema, turns):
    xt = parse_xt(helix_document("x_t", key, schema=schema, turns=turns, user_fields=1))
    binary = helix_document("x_b", key, schema=schema, turns=turns, user_fields=1)
    xb = parse_xb(binary)
    # These raw-record fixtures have no BODY root for topology comparison.
    comparison = compare_documents(xt, xb)
    assert comparison.field_values_equal and comparison.node_index_layout_equal
    assert write_xb(xb) == binary
    for doc in (xt, xb):
        assert doc.header.schema_max_type == 231
        assert doc.nodes[0].fields[1].values[0].value == 41
        assert doc.nodes[0].user_fields == (-401,)
        for node, hand in zip(doc.nodes[1:3], "+-", strict=True):
            values = [field.values[0].value for field in node.fields]
            expected = [AXIS_POINT, AXIS_DIRECTION, POINT, ord(hand)]
            expected += list(turns) if schema == "split" else [turns]
            assert values == [*expected, 0.007, 1e-8]
            assert node.user_fields == (-node.index,)
            assert len(node.definition.fields) == (8 if schema == "split" else 7)
        assert doc.nodes[1].first_schema.definition.source == (
            SchemaSource.EMBEDDED_UNCHANGED
            if schema == "unchanged"
            else SchemaSource.EMBEDDED_DELTA
        )
        assert doc.nodes[2].first_schema is None
        assert [v.value for v in doc.nodes[-1].fields[0].values] == [-7, 123456]
        assert doc.nodes[-1].user_fields == (-43,)
        assert doc.terminator.byte_range.end <= len(doc.raw_bytes)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_helix_invalid_delta_and_truncation_fail_closed(encoding):
    parser = parse_xt if encoding == "x_t" else parse_xb
    with pytest.raises(SchemaError) as captured:
        parser(helix_document(encoding, schema="bad_count"))
    assert captured.value.diagnostic.code == "schema.field_count_mismatch"
    data = helix_document(encoding)
    doc = parser(data)
    for end in range(doc.nodes[1].byte_range.start, doc.nodes[1].byte_range.end):
        with pytest.raises(ParseError):
            parser(data[:end])


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_helix_descriptor_keeps_the_owning_nurbs_curve(encoding):
    original = spcurve(encoding, embedded=True).replace(EMBEDDED.encode(), KEY.encode())
    extra = helix_records(encoding)
    if encoding == "x_t":
        old = b"135 255 6 1 0 "
        new = b"135 255 6 1 41 "
        extra = extra.replace(b"135 255 ", b"135 ", 1)
    else:
        prefix = struct.pack(">HBhB", 135, 255, 7, 1)
        old, new = prefix + positive_integer(0), prefix + positive_integer(41)
        extra = extra[:2] + extra[3:]  # CURVE_DATA already has its schema.
    assert original.count(old) == 1
    data = original.replace(old, new)[:-4] + extra + original[-4:]
    parsed = read_brep(data)
    assert parsed.brep.complete and parsed.brep.topology.valid
    spline = next(c.definition for c in parsed.brep.curves if c.source.node_index == 4)
    assert isinstance(spline, NurbsCurve)
    assert spline.knots == (0.0, 1.0)
    assert tuple(v for p in spline.control_vertices for v in p) == (0.011, -0.017, 0.023, 0.031)
    descriptor = next(n for n in parsed.document.nodes if n.index == 41)
    assert descriptor.node_type == 163


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_issue13_assembly_check_keeps_helix_descriptors(tmp_path, capsys, encoding):
    # The descriptors are metadata beside the independently authored assembly.
    data = assembly_fixture(encoding, KEY)[:-4] + helix_records(encoding)
    data += b"1 0 " if encoding == "x_t" else b"\x00\x01\x00\x01"
    parsed = read_brep(data)
    assert parsed.brep.complete and parsed.brep.topology.valid
    assert (len(parsed.brep.assemblies), len(parsed.brep.instances)) == (2, 3)
    assert sum(node.node_type == 163 for node in parsed.document.nodes) == 2
    path = tmp_path / f"assembly.{encoding}"
    path.write_bytes(data)
    assert main(["check", str(path), "--json"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["status"] == "complete"
    assert report["summary"]["schema_resolution"]["profile_id"] == (
        "solidworks-3601228-36001-13006-r2"
    )


@pytest.mark.skipif(importlib.util.find_spec("OCP") is None, reason="requires OCCT")
def test_issue13_step_export_passes_the_schema_boundary(tmp_path, capsys):
    source = tmp_path / "wire.x_t"
    data = assembly_fixture("x_t", KEY)[:-4] + helix_records("x_t") + b"1 0 "
    source.write_bytes(data)
    output = tmp_path / "wire.step"
    assert main(["export-step", str(source), str(output), "--source-unit", "m"]) == 0
    assert output.is_file()
    report = json.loads(capsys.readouterr().out)
    assert report["status"] == "validated"
    assert report["conversion"]["occt_valid"]
