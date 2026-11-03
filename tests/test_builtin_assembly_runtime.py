"""Authored assemblies: ASSEMBLY, INSTANCE and TRANSFORM records beside a wire body.

The layouts follow the public XT reference, pp. 78-79 and 83-86; the body is the
legacy wire fixture. Only these synthetic fixtures substitute keys.
"""

import json

import pytest

from parasolid_kit import (
    AssemblyKind,
    InstancePartKind,
    ParseError,
    SchemaError,
    Transform,
    compare_documents,
    map_brep,
    parse_xb,
    parse_xt,
    read_brep,
    write_xb,
)
from parasolid_kit.brep import Vector3
from parasolid_kit.cli import main
from tests.support.parasolid_records import records_fixture
from tests.test_builtin_icad_legacy_runtime import CODES as BODY_CODES
from tests.test_builtin_icad_legacy_runtime import wire_records

STANDARD_KEY = "SCH_2401000_20000"
# The observed SolidWorks 2026 / 2020 and NX keys, another build of the
# SolidWorks 2020 schema (its family), an iCAD key completed by its family and
# the standard 20000 key.
KEYS = {
    "SCH_3701229_37102_13006": "solidworks-sch37102-13006-r2",
    "SCH_3101290_31100_13006": "solidworks-3101290-31100-13006-r1",
    "SCH_2600169_25001_13006": "nx-2600169-25001-13006-r1",
    "SCH_3101299_31100_13006": "family-31100-13006",
    "SCH_1901315_19008_13006": "icad-1901315-19008-13006-r1",
    STANDARD_KEY: "icad-2401000-20000-r3",
}
# A quarter turn about z, stored row by row.
ROTATION = (0, -1, 0, 1, 0, 0, 0, 0, 1)
CODES = {
    **BODY_CODES,
    10: "dpppppppffpppupup",
    11: "dpuppppppp",
    100: "dppp" + "f" * 9 + "vfdv",
}


def codes_for(key):
    if key != STANDARD_KEY:
        return CODES
    # Schema 20000 appends the index-map fields to BODY.
    return {**CODES, 12: "dppppppffpppupuupppppppdppp"}


def assembly_records(overrides=None):
    """Root assembly 12 holds instance 13 (body 1 through transform 14) and the
    negative instance 15 of sub-assembly 16, which holds instance 17 (body 1)."""
    records = {
        12: (10, 12, {8: 1e3, 9: 1e-8, 13: 1, 15: 1, 16: 13}),
        13: (11, 13, {2: 1, 3: 1, 4: 14, 5: 12, 6: 15, 8: 17}),
        14: (
            100,
            14,
            {
                1: 13,
                **{4 + i: v for i, v in enumerate(ROTATION)},
                13: (1, 2, 3),
                14: 1,
                15: 3,
                16: (0, 0, 0),
            },
        ),
        15: (11, 15, {2: 2, 3: 16, 5: 12}),
        16: (10, 16, {8: 1e3, 9: 1e-8, 10: 15, 13: 1, 15: 3, 16: 17}),
        17: (11, 17, {2: 1, 3: 1, 5: 16}),
    }
    for index, changes in (overrides or {}).items():
        kind, _, values = records[index]
        records[index] = (kind, index, {**values, **changes})
    return list(records.values())


def fixture(encoding, key, overrides=None):
    body = wire_records()
    # The body's ref_instance chain starts at the placing instance.
    body[0] = (12, 1, {**body[0][2], 9: 13})
    return records_fixture(
        encoding,
        key,
        [*body, *assembly_records(overrides)],
        codes_for(key),
        embedded=key != STANDARD_KEY,
    )


def parse(encoding, data):
    return (parse_xt if encoding == "x_t" else parse_xb)(data)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize("key", list(KEYS))
def test_assembly_structure_is_mapped_beside_the_body(encoding, key):
    doc = parse(encoding, fixture(encoding, key))
    assert doc.schema_resolution.profile_id == KEYS[key]
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert len(model.bodies) == 1
    root, sub = model.assemblies
    assert (root.kind, root.size_resolution, root.linear_resolution) == (
        AssemblyKind.COLLECTIVE,
        1e3,
        1e-8,
    )
    assert (root.instances, root.referencing_instances) == ((0, 1), ())
    assert (sub.kind, sub.instances, sub.referencing_instances) == (
        AssemblyKind.DISJUNCTIVE,
        (2,),
        (1,),
    )
    assert (root.source.node_type, root.source.node_index, root.source.node_id) == (10, 12, None)
    placed, negative, nested = model.instances
    assert (placed.part_kind, placed.part, placed.assembly, placed.negative) == (
        InstancePartKind.BODY,
        0,
        0,
        False,
    )
    assert placed.source.node_id == 113
    transform = placed.transform
    assert transform is not None
    assert transform.rotation == ((0, -1, 0), (1, 0, 0), (0, 0, 1))
    assert (transform.translation, transform.scale, transform.flags) == (
        Vector3(1.0, 2.0, 3.0),
        1.0,
        3,
    )
    assert transform.has(Transform.TRANSLATION) and transform.has(Transform.ROTATION)
    assert not transform.has(Transform.SCALING)
    assert transform.apply(Vector3(1.0, 0.0, 0.0)) == Vector3(1.0, 3.0, 3.0)
    assert (transform.source.node_type, transform.source.node_id) == (100, 114)
    assert (negative.part_kind, negative.part, negative.negative, negative.transform) == (
        InstancePartKind.ASSEMBLY,
        1,
        True,
        None,
    )
    assert (nested.part_kind, nested.part, nested.assembly) == (InstancePartKind.BODY, 0, 1)
    report = json.loads(json.dumps(model.to_dict()))
    assert report["instances"][0]["transform"]["flags"] == 3
    assert report["assemblies"][1]["kind"] == "disjunctive"


@pytest.mark.parametrize("key", list(KEYS))
def test_text_and_binary_assemblies_are_equivalent(key):
    text, binary = fixture("x_t", key), fixture("x_b", key)
    xt, xb = parse_xt(text), parse_xb(binary)
    assert compare_documents(xt, xb).equivalent
    assert write_xb(xb) == binary
    transform = xb.nodes[-4]
    assert (transform.node_type, len(transform.fields)) == (100, 9)
    # The rotation matrix is one nine-element field.
    assert len(transform.fields[4].values) == 9


def test_summary_and_check_report_assembly_counts(tmp_path, capsys):
    path = tmp_path / "assembly.x_t"
    path.write_bytes(fixture("x_t", "SCH_3701229_37102_13006"))
    parsed = read_brep(path)
    assert (parsed.summary.counts.assemblies, parsed.summary.counts.instances) == (2, 3)
    assert main(["check", str(path)]) == 0
    out = capsys.readouterr().out
    assert "  assemblies: 2" in out and "  instances: 3" in out


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    "overrides,code",
    [
        ({13: {3: 5}}, "brep.invalid_reference"),  # the part is a POINT
        ({13: {2: 3}}, "brep.invalid_field"),  # unknown instance type
        ({13: {4: 1}}, "brep.invalid_reference"),  # the transform pointer targets the body
        ({14: {14: 0}}, "geometry.invalid_parameter"),  # zero scale
        ({13: {6: 13}}, "topology.invalid_relationship"),  # the sub-instance chain cycles
        ({12: {15: 4}}, "brep.invalid_field"),  # unknown assembly type
        ({15: {5: 0}}, "brep.invalid_reference"),  # an instance without its assembly
    ],
)
def test_malformed_assemblies_fail_closed(encoding, overrides, code):
    doc = parse(encoding, fixture(encoding, "SCH_3701229_37102_13006", overrides))
    with pytest.raises(ParseError) as exc:
        map_brep(doc)
    assert exc.value.diagnostic.code == code


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_assembly_types_stay_uncovered_under_other_standard_schemas(encoding):
    # No public 16100 file carries an assembly, so its profile keeps the types undeclared.
    data = records_fixture(
        encoding, "SCH_1700000_16100", assembly_records()[:1], CODES, embedded=False
    )
    with pytest.raises(SchemaError) as exc:
        parse(encoding, data)
    assert exc.value.diagnostic.code == "schema.builtin_profile_uncovered_type"
