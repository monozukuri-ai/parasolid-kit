"""Authored fixtures for the later standard-key revisions; no catalog is required.

Schema 31001 as a standard key (the edits of the 32001 standard key), the
assembly structure under schema 28101 and the types added to the 15003 and
20000 profiles; then schemas 37102 and 12103 as standard keys, the assembly
structure under schema 9008 and the embedded key of schema 35002. Codes and
ordinals are authored here independently of the runtime profile declarations;
only these synthetic fixtures choose the keys.
"""

import pytest

from parasolid_kit import (
    AssemblyKind,
    InstancePartKind,
    builtin_profiles,
    map_brep,
    parse_xb,
    parse_xt,
)
from parasolid_kit.brep import Vector3
from tests.support.parasolid_records import records_fixture
from tests.test_builtin_assembly_runtime import ROTATION
from tests.test_builtin_icad_standard_runtime import BODIES, CODES, wire_records

# BODY as the 32001 standard key writes it: 34 fields, the mesh offset
# reference last. REGION carries its owner and LIST its revised header.
BODY_31001 = "dppppppppffpppupuupppppppppdppppdp"
STANDARD_31001 = {
    **CODES,
    12: BODY_31001,
    19: "dpppppcp",
    70: "dulpppdddpp",
}
# BODY of schema 28101 (33 fields) and ASSEMBLY with the mesh and polyline
# construction lists after its seventh field (19 fields).
STANDARD_28101 = {
    **CODES,
    12: BODIES["SCH_2901000_28101"][0],
    19: "dpppppcp",
    70: "dulpppdddpp",
    10: "dpppppppppffpppupup",
    11: "dpuppppppp",
    100: "dppp" + "f" * 9 + "vfdv",
}


# BODY of the 37102 standard key: the lattice, mesh and polyline references
# of the 33103 layout and the boundary lattice after the legacy shell (36
# fields). REGION carries a frame reference before its kind.
BODY_37102 = "dpppppppppffpppupuuppppppppppdppppdp"
STANDARD_37102 = {**CODES, 12: BODY_37102, 19: "dppppppcp"}
# Schema 12103 keeps the 13006 layouts: BODY of 23 fields, REGION of seven.
STANDARD_12103 = {**CODES, 12: BODIES["SCH_1300218_13006"][0], 19: "dpppppc"}
# Schema 9008: BODY without its geometry state (22 fields), FIN without its
# attribute pointer (nine), and the assembly records in their 13006 layouts.
STANDARD_9008 = {
    **CODES,
    12: "dppppppffpppupuppppppp",
    17: "ppppppppc",
    19: "dpppppc",
    10: "dpppppppffpppupup",
    11: "dpuppppppp",
    100: "dppp" + "f" * 9 + "vfdv",
}


def parse(encoding, data):
    return (parse_xt if encoding == "x_t" else parse_xb)(data)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    ("key", "profile"),
    [
        ("SCH_3100000_31001", "parasolid-3100000-31001-r1"),
        # Another build of the schema resolves through the family.
        ("SCH_3100203_31001", "family-31001"),
    ],
)
def test_schema_31001_reads_an_authored_wire_body(encoding, key, profile):
    # The wire of the 32001 standard key: the same BODY ordinals.
    records = wire_records("SCH_3200000_32001")
    doc = parse(encoding, records_fixture(encoding, key, records, STANDARD_31001, embedded=False))
    assert doc.schema_resolution.schema_key == key
    assert doc.schema_resolution.profile_id == profile
    assert all(s.definition.source.value == "base" and not s.edits for s in doc.schemas)
    body = next(s for s in doc.schemas if s.definition.node_type == 12)
    assert len(body.definition.fields) == len(BODY_31001)
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert (model.bodies[0].size_resolution, model.bodies[0].linear_resolution) == (1e-6, 1e-8)
    assert [p.position for p in model.points] == [Vector3(2.0, -1.0, 3.0), Vector3(5.0, 3.0, 3.0)]
    assert model.curves[0].definition.direction == Vector3(0.6, 0.8, 0.0)


def assembly_28101():
    """The wire body placed by one instance of a root assembly, with every
    ASSEMBLY ordinal after the two inserted lists shifted by two."""
    body = wire_records("SCH_2901000_28101")
    # The body's ref_instance (base ordinal 9) follows the two inserted lists.
    body[0] = (12, 1, {**body[0][2], 11: 13})
    return [
        *body,
        (10, 12, {10: 1e3, 11: 1e-8, 15: 1, 17: 1, 18: 13}),
        (11, 13, {2: 1, 3: 1, 4: 14, 5: 12}),
        (
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
    ]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    ("key", "profile"),
    [
        ("SCH_2901000_28101", "icad-2901000-28101-r3"),
        ("SCH_2900000_28101", "family-28101"),
    ],
)
def test_schema_28101_maps_an_authored_assembly(encoding, key, profile):
    data = records_fixture(encoding, key, assembly_28101(), STANDARD_28101, embedded=False)
    doc = parse(encoding, data)
    assert doc.schema_resolution.profile_id == profile
    assembly = next(s for s in doc.schemas if s.definition.node_type == 10)
    assert len(assembly.definition.fields) == 19
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert len(model.bodies) == 1
    (root,) = model.assemblies
    assert (root.kind, root.size_resolution, root.linear_resolution) == (
        AssemblyKind.COLLECTIVE,
        1e3,
        1e-8,
    )
    assert root.instances == (0,)
    (placed,) = model.instances
    assert (placed.part_kind, placed.part, placed.assembly, placed.negative) == (
        InstancePartKind.BODY,
        0,
        0,
        False,
    )
    transform = placed.transform
    assert transform is not None
    assert transform.rotation == ((0, -1, 0), (1, 0, 0), (0, 0, 1))
    assert transform.translation == Vector3(1.0, 2.0, 3.0)
    assert transform.apply(Vector3(1.0, 0.0, 0.0)) == Vector3(1.0, 3.0, 3.0)


def wire_37102():
    """The authored wire body in the 37102 layouts."""
    return [
        # Precisions, kind and the three heads follow the inserted references.
        (12, 1, {10: 1e-6, 11: 1e-8, 17: 2, 26: 2, 27: 6, 28: 4}),
        # The kind follows the frame reference.
        (19, 2, {2: 1, 5: 3, 7: "V"}),
        *wire_records("SCH_3301000_33103")[2:],
    ]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    ("key", "profile"),
    [
        ("SCH_3701153_37102", "feko-3701153-37102-r1"),
        # Another build of the schema resolves through the family.
        ("SCH_3700000_37102", "family-37102"),
    ],
)
def test_schema_37102_reads_an_authored_wire_body(encoding, key, profile):
    doc = parse(
        encoding, records_fixture(encoding, key, wire_37102(), STANDARD_37102, embedded=False)
    )
    assert doc.schema_resolution.schema_key == key
    assert doc.schema_resolution.profile_id == profile
    assert all(s.definition.source.value == "base" and not s.edits for s in doc.schemas)
    body = next(s for s in doc.schemas if s.definition.node_type == 12)
    assert len(body.definition.fields) == len(BODY_37102) == 36
    classes = [f.pointer_class for f in body.definition.fields]
    assert (classes[3], classes[20], classes[35]) == (222, 222, 206)
    region = next(s for s in doc.schemas if s.definition.node_type == 19)
    assert [f.pointer_class for f in region.definition.fields][6] == 230
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert (model.bodies[0].size_resolution, model.bodies[0].linear_resolution) == (1e-6, 1e-8)
    assert [p.position for p in model.points] == [Vector3(2.0, -1.0, 3.0), Vector3(5.0, 3.0, 3.0)]
    assert model.curves[0].definition.direction == Vector3(0.6, 0.8, 0.0)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    ("key", "profile"),
    [
        ("SCH_1200000_12103", "cadexchanger-1200000-12103-r1"),
        ("SCH_1200113_12103", "family-12103"),
    ],
)
def test_schema_12103_reads_an_authored_wire_body(encoding, key, profile):
    records = wire_records("SCH_1300218_13006")
    doc = parse(encoding, records_fixture(encoding, key, records, STANDARD_12103, embedded=False))
    assert doc.schema_resolution.profile_id == profile
    assert all(s.definition.source.value == "base" and not s.edits for s in doc.schemas)
    body = next(s for s in doc.schemas if s.definition.node_type == 12)
    assert len(body.definition.fields) == 23
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert [p.position for p in model.points] == [Vector3(2.0, -1.0, 3.0), Vector3(5.0, 3.0, 3.0)]


def assembly_9008():
    """The wire body in the 9008 layouts, placed by one instance of a root
    assembly whose records keep their 13006 ordinals."""
    return [
        # BODY without its geometry state: the heads move up by one.
        (12, 1, {7: 1e-6, 8: 1e-8, 9: 13, 14: 2, 19: 2, 20: 6, 21: 4}),
        (19, 2, {2: 1, 5: 3, 6: "V"}),
        (13, 3, {7: 2}),
        (18, 4, {4: 9, 5: 5, 7: 1}),
        (29, 5, {2: 4, 5: (2, -1, 3)}),
        (16, 6, {3: 7, 6: 11, 9: 1}),
        # FIN without its attribute pointer: every ordinal moves up by one.
        (17, 7, {3: 4, 4: 8, 5: 6, 8: "+"}),
        (17, 8, {3: 9, 4: 7, 5: 6, 8: "-"}),
        (18, 9, {5: 10, 7: 1}),
        (29, 10, {2: 9, 5: (5, 3, 3)}),
        (30, 11, {2: 6, 7: (2, -1, 3), 8: (0.6, 0.8, 0)}),
        (10, 12, {8: 1e3, 9: 1e-8, 13: 1, 15: 1, 16: 13}),
        (11, 13, {2: 1, 3: 1, 4: 14, 5: 12}),
        (
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
    ]


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    ("key", "profile"),
    [
        ("SCH_901297_9008", "microstation-901297-9008-r2"),
        ("SCH_901000_9008", "family-9008"),
    ],
)
def test_schema_9008_maps_an_authored_assembly(encoding, key, profile):
    data = records_fixture(encoding, key, assembly_9008(), STANDARD_9008, embedded=False)
    doc = parse(encoding, data)
    assert doc.schema_resolution.profile_id == profile
    assembly = next(s for s in doc.schemas if s.definition.node_type == 10)
    assert len(assembly.definition.fields) == 17
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert len(model.bodies) == 1
    (root,) = model.assemblies
    assert (root.kind, root.size_resolution, root.linear_resolution) == (
        AssemblyKind.COLLECTIVE,
        1e3,
        1e-8,
    )
    assert root.instances == (0,)
    (placed,) = model.instances
    assert (placed.part_kind, placed.part, placed.assembly, placed.negative) == (
        InstancePartKind.BODY,
        0,
        0,
        False,
    )
    transform = placed.transform
    assert transform is not None
    assert transform.rotation == ((0, -1, 0), (1, 0, 0), (0, 0, 1))
    assert transform.translation == Vector3(1.0, 2.0, 3.0)


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
@pytest.mark.parametrize(
    ("key", "profile"),
    [
        ("SCH_3500184_35002_13006", "nx-3500184-35002-13006-r1"),
        ("SCH_3500200_35002_13006", "family-35002-13006"),
    ],
)
def test_the_embedded_key_of_schema_35002_reads_an_authored_wire_body(encoding, key, profile):
    # An embedded key carries its edits; the wire body keeps the base layouts.
    records = wire_records("SCH_1300218_13006")
    doc = parse(encoding, records_fixture(encoding, key, records, STANDARD_12103, embedded=True))
    assert doc.schema_resolution.profile_id == profile
    assert all(s.definition.source.value == "embedded_unchanged" for s in doc.schemas)
    model = map_brep(doc)
    assert model.complete and model.topology.valid
    assert [p.position for p in model.points] == [Vector3(2.0, -1.0, 3.0), Vector3(5.0, 3.0, 3.0)]


def test_revised_profiles_list_their_added_types():
    profiles = {p.profile_id: p for p in builtin_profiles()}
    # 15003 +UNICODE_VALUES, then +SWEPT_SURF, TRIMMED_CURVE and the SP_CURVE
    # dependencies; 20000 +SPUN_SURF; 28101 +ASSEMBLY/INSTANCE/TRANSFORM.
    assert profiles["icad-1500000-15003-r4"].type_count == 44
    assert profiles["icad-2401000-20000-r5"].type_count == 50
    assert profiles["icad-2901000-28101-r3"].type_count == 29
    new = profiles["parasolid-3100000-31001-r1"]
    assert (new.provider_schema, new.schema_keys) == ("31001", ("SCH_3100000_31001",))
    family = profiles["family-31001"]
    assert [m.profile_id for m in family.members] == ["parasolid-3100000-31001-r1"]
    assert family.type_count == new.type_count
    for retired in (
        "icad-1500000-15003-r2",
        "icad-1500000-15003-r3",
        "icad-2401000-20000-r4",
        "icad-2901000-28101-r2",
        "microstation-901297-9008-r1",
        "inventor-2600169-25001-r1",
    ):
        assert retired not in profiles
    # The standard keys of schemas 37102 and 12103 and the embedded key of
    # schema 35002 each anchor a family of one member.
    for profile_id, schema, key, types, family_id in (
        ("feko-3701153-37102-r1", "37102", "SCH_3701153_37102", 27, "family-37102"),
        ("cadexchanger-1200000-12103-r1", "12103", "SCH_1200000_12103", 37, "family-12103"),
        (
            "nx-3500184-35002-13006-r1",
            "13006",
            "SCH_3500184_35002_13006",
            None,
            "family-35002-13006",
        ),
    ):
        profile = profiles[profile_id]
        assert (profile.provider_schema, profile.schema_keys) == (schema, (key,))
        if types is not None:
            assert profile.type_count == types
        assert [m.profile_id for m in profiles[family_id].members] == [profile_id]
    # 9008 +the assembly structure and four more types; 25001 +six types, the
    # multi-part transmit block among them.
    assert profiles["microstation-901297-9008-r2"].type_count == 51
    assert profiles["inventor-2600169-25001-r2"].type_count == 45
