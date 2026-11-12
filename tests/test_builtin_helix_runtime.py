"""Helical forms of B-surfaces and B-curves (issue #13), XT reference pp. 43-44, 72, 118.

Streams write the analytic form of a B-surface (``SURFACE_DATA`` with analytic
form type 'H') as node type 163 with the reference's ``HELIX_SU_FORM`` fields
(vvcifff: axis, hand, turns, pitch, gap, tolerance) and the analytic form of a
B-curve (``CURVE_DATA``) as node type 184 with the ``HELIX_CU_FORM`` fields
(vvvciff: axis, point, hand, turns, pitch, tolerance); the reference's table
gives the two numbers the other way round. Fixtures are authored independently
of the compiled profile definitions.
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
from parasolid_kit.brep import NurbsCurve, NurbsSurface
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
PITCH, GAP, TOLERANCE = 0.007, None, 1e-8
# Node type, the link record that refers to the descriptor, the vectors before
# the hand and the doubles after the turns.
FORMS = {
    "surface": (163, (AXIS_POINT, AXIS_DIRECTION), (PITCH, GAP, TOLERANCE)),
    "curve": (184, (AXIS_POINT, AXIS_DIRECTION, POINT), (PITCH, TOLERANCE)),
}
UNSET = -3.14158e13


def link(encoding, form, first=40):
    """SURFACE_DATA (analytic type 'H') or CURVE_DATA naming the next descriptor."""
    target = first + 1
    if encoding == "x_t":
        if form == "surface":
            return f"125 255 {first} 0 1 0 1 0 1 0 1 1 BBBBBBBBH???{target} 0 0 0 ".encode()
        return f"135 255 {first} 1 {target} ".encode()
    if form == "surface":
        data = struct.pack(">HB", 125, 255) + positive_integer(first)
        data += struct.pack(">8dB", 0, 1, 0, 1, 0, 1, 0, 1, 1) + b"BBBBBBBBH???"
        return data + positive_integer(target) + positive_integer(0) * 3
    data = struct.pack(">HB", 135, 255) + positive_integer(first) + b"\x01"
    return data + positive_integer(target)


def markers(encoding, form):
    kind, vectors, _ = FORMS[form]
    # Copy every base field, or replace the turns interval by two doubles.
    before = "C" * (len(vectors) + 1)
    after = "C" * (2 if kind == 184 else 3)
    if encoding == "x_t":
        return {
            "unchanged": b"255 ",
            "copy": b"7 CCCCCCCZ",
            "split": f"8 {before}DI5 start0 0 1 fI3 end0 0 1 f{after}Z".encode(),
            "bad_count": b"8 CCCCCCCZ",
        }
    return {
        "unchanged": b"\xff",
        "copy": b"\x07CCCCCCCZ",
        "split": (
            b"\x08"
            + before.encode()
            + b"DI"
            + embedded_field("start", field_type="f")
            + b"I"
            + embedded_field("end", field_type="f")
            + after.encode()
            + b"Z"
        ),
        "bad_count": b"\x08CCCCCCCZ",
    }


def helix_records(
    encoding, form, *, schema="unchanged", turns=(-0.25, 3.5), user_fields=0, first=40
):
    """A link record, two descriptors and a following integer-values node."""
    kind, vectors, tail = FORMS[form]
    marker = markers(encoding, form)[schema]
    # X_T encodes a null composite with one '?'; X_B keeps both doubles.
    wire_turns = turns[:1] if turns == (None, None) and schema != "split" else turns
    data = bytearray(link(encoding, form, first))
    if user_fields:
        data.extend(b"-401 " if encoding == "x_t" else struct.pack(">i", -401))
    for index, hand in ((first + 1, "+"), (first + 2, "-")):
        if encoding == "x_t":
            data.extend(f"{kind} ".encode() + (marker if index == first + 1 else b""))
            data.extend(f"{index} ".encode())
            data.extend(" ".join(str(v) for vector in vectors for v in vector).encode())
            data.extend(b" " + hand.encode())
            data.extend(b"".join(b"?" if v is None else f"{v} ".encode() for v in wire_turns))
            data.extend(b"".join(b"?" if v is None else f"{v} ".encode() for v in tail))
            if user_fields:
                data.extend(f"{-index} ".encode())
        else:
            data.extend(struct.pack(">H", kind) + (marker if index == first + 1 else b""))
            data.extend(positive_integer(index))
            data.extend(b"".join(struct.pack(">3d", *vector) for vector in vectors))
            data.extend(hand.encode())
            for value in (*turns, *tail):
                data.extend(struct.pack(">d", UNSET if value is None else value))
            if user_fields:
                data.extend(struct.pack(">i", -index))
    values = first + 3
    if encoding == "x_t":
        data.extend(b"82 " + (b"255 " if first == 40 else b"") + f"2 {values} -7 123456 ".encode())
        if user_fields:
            data.extend(f"{-values} ".encode())
    else:
        data.extend(struct.pack(">H", 82) + (b"\xff" if first == 40 else b""))
        data.extend(struct.pack(">i", 2) + positive_integer(values))
        data.extend(struct.pack(">2i", -7, 123456))
        if user_fields:
            data.extend(struct.pack(">i", -values))
    return bytes(data)


def helix_document(encoding, form, key=KEY, **options):
    user_fields = options.get("user_fields", 0)
    header = (
        text_header(key, schema_max_type=231, user_field_size=user_fields)
        if encoding == "x_t"
        else SyntheticXbBuilder(
            schema_name=key, schema_max_type=231, user_field_size=user_fields
        ).header()
    )
    terminator = b"1 0 " if encoding == "x_t" else b"\x00\x01\x00\x01"
    return header + helix_records(encoding, form, **options) + terminator


@pytest.mark.parametrize("form", list(FORMS))
@pytest.mark.parametrize("key", KEYS)
@pytest.mark.parametrize("schema", ["unchanged", "copy", "split"])
@pytest.mark.parametrize("turns", [(-0.25, 3.5), (None, None)])
def test_helix_fields_cached_schema_and_following_record(form, key, schema, turns):
    kind, vectors, tail = FORMS[form]
    options = dict(schema=schema, turns=turns, user_fields=1)
    xt = parse_xt(helix_document("x_t", form, key, **options))
    binary = helix_document("x_b", form, key, **options)
    xb = parse_xb(binary)
    # These raw-record fixtures have no BODY root for topology comparison.
    comparison = compare_documents(xt, xb)
    assert comparison.field_values_equal and comparison.node_index_layout_equal
    assert write_xb(xb) == binary
    for doc in (xt, xb):
        assert doc.header.schema_max_type == 231
        reference = next(f for f in doc.nodes[0].fields if f.values[0].value == 41)
        assert reference.definition.name == "analytic_form"
        assert doc.nodes[0].user_fields == (-401,)
        for node, hand in zip(doc.nodes[1:3], "+-", strict=True):
            assert node.node_type == kind
            values = [field.values[0].value for field in node.fields]
            expected = [*vectors, ord(hand)]
            expected += list(turns) if schema == "split" else [turns]
            assert values == [*expected, *tail]
            assert node.user_fields == (-node.index,)
            assert len(node.definition.fields) == (8 if schema == "split" else 7)
        names = [f.name for f in doc.nodes[1].definition.fields]
        assert names[-1] == "tolerance"
        assert ("gap" in names) == (form == "surface")
        assert ("point" in names) == (form == "curve")
        assert doc.nodes[1].first_schema.definition.source == (
            SchemaSource.EMBEDDED_UNCHANGED
            if schema == "unchanged"
            else SchemaSource.EMBEDDED_DELTA
        )
        assert doc.nodes[2].first_schema is None
        assert [v.value for v in doc.nodes[-1].fields[0].values] == [-7, 123456]
        assert doc.nodes[-1].user_fields == (-43,)
        assert doc.terminator.byte_range.end <= len(doc.raw_bytes)


@pytest.mark.parametrize("form", list(FORMS))
@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_helix_invalid_delta_and_truncation_fail_closed(encoding, form):
    parser = parse_xt if encoding == "x_t" else parse_xb
    with pytest.raises(SchemaError) as captured:
        parser(helix_document(encoding, form, schema="bad_count"))
    assert captured.value.diagnostic.code == "schema.field_count_mismatch"
    data = helix_document(encoding, form)
    doc = parser(data)
    for end in range(doc.nodes[1].byte_range.start, doc.nodes[1].byte_range.end):
        with pytest.raises(ParseError):
            parser(data[:end])


def test_each_number_reads_only_its_own_form():
    # A curve-form record written under 163 (or the other way round) leaves
    # the stream out of step before the following record.
    for form, other in (("surface", 184), ("curve", 163)):
        kind = FORMS[form][0]
        data = helix_document("x_t", form).replace(f"{kind} ".encode(), f"{other} ".encode())
        with pytest.raises(ParseError):
            parse_xt(data)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_helix_curve_form_keeps_the_owning_nurbs_curve(encoding):
    original = spcurve(encoding, embedded=True).replace(EMBEDDED.encode(), KEY.encode())
    extra = helix_records(encoding, "curve")
    if encoding == "x_t":
        old = b"135 255 6 1 0 "
        new = b"135 255 6 1 41 "
        extra = extra[len(link(encoding, "curve")) :]  # CURVE_DATA already exists.
    else:
        prefix = struct.pack(">HBhB", 135, 255, 7, 1)
        old, new = prefix + positive_integer(0), prefix + positive_integer(41)
        extra = extra[len(link(encoding, "curve")) :]  # CURVE_DATA already exists.
    assert original.count(old) == 1
    data = original.replace(old, new)[:-4] + extra + original[-4:]
    parsed = read_brep(data)
    assert parsed.brep.complete and parsed.brep.topology.valid
    spline = next(c.definition for c in parsed.brep.curves if c.source.node_index == 4)
    assert isinstance(spline, NurbsCurve)
    assert spline.knots == (0.0, 1.0)
    assert tuple(v for p in spline.control_vertices for v in p) == (0.011, -0.017, 0.023, 0.031)
    descriptor = next(n for n in parsed.document.nodes if n.index == 41)
    assert descriptor.node_type == 184


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_helix_surface_form_keeps_the_owning_bspline_surface(encoding):
    original = spcurve(encoding, embedded=True, patch={}).replace(EMBEDDED.encode(), KEY.encode())
    extra = helix_records(encoding, "surface")[len(link(encoding, "surface")) :]
    if encoding == "x_t":
        old, new = b"BBBBBBBB????0 0 0 0 ", b"BBBBBBBBH???41 0 0 0 "
    else:
        old = b"BBBBBBBB????" + positive_integer(0) * 4
        new = b"BBBBBBBBH???" + positive_integer(41) + positive_integer(0) * 3
    assert original.count(old) == 1
    data = original.replace(old, new)[:-4] + extra + original[-4:]
    parsed = read_brep(data)
    assert parsed.brep.complete and parsed.brep.topology.valid
    surface = parsed.brep.surfaces[0].definition
    assert isinstance(surface, NurbsSurface)
    assert surface.u_knots == surface.v_knots == (0, 1)
    data_node = next(n for n in parsed.document.nodes if n.node_type == 125)
    kinds = [f for f in data_node.fields if f.definition.name == "analytic_kind"]
    assert kinds[0].values[0].value == ord("H")
    descriptor = next(n for n in parsed.document.nodes if n.index == 41)
    assert descriptor.node_type == 163
    assert [f.definition.name for f in descriptor.fields][2:] == [
        "hand",
        "turns",
        "pitch",
        "gap",
        "tolerance",
    ]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_issue13_assembly_check_keeps_helix_descriptors(tmp_path, capsys, encoding):
    # The descriptors are metadata beside the independently authored assembly.
    data = assembly_fixture(encoding, KEY)[:-4]
    data += helix_records(encoding, "surface")
    data += helix_records(encoding, "curve", first=50)
    data += b"1 0 " if encoding == "x_t" else b"\x00\x01\x00\x01"
    parsed = read_brep(data)
    assert parsed.brep.complete and parsed.brep.topology.valid
    assert (len(parsed.brep.assemblies), len(parsed.brep.instances)) == (2, 3)
    kinds = [node.node_type for node in parsed.document.nodes]
    assert (kinds.count(163), kinds.count(184)) == (2, 2)
    path = tmp_path / f"assembly.{encoding}"
    path.write_bytes(data)
    assert main(["check", str(path), "--json"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["status"] == "complete"
    assert report["summary"]["schema_resolution"]["profile_id"] == (
        "solidworks-3601228-36001-13006-r3"
    )


@pytest.mark.skipif(importlib.util.find_spec("OCP") is None, reason="requires OCCT")
def test_issue13_step_export_passes_the_schema_boundary(tmp_path, capsys):
    source = tmp_path / "wire.x_t"
    data = assembly_fixture("x_t", KEY)[:-4] + helix_records("x_t", "surface") + b"1 0 "
    source.write_bytes(data)
    output = tmp_path / "wire.step"
    assert main(["export-step", str(source), str(output), "--source-unit", "m"]) == 0
    assert output.is_file()
    report = json.loads(capsys.readouterr().out)
    assert report["status"] == "validated"
    assert report["conversion"]["occt_valid"]
