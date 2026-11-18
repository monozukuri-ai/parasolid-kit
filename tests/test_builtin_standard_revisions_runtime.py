"""Authored fixtures for the later standard-key revisions; no catalog is required.

Schema 31001 as a standard key (the edits of the 32001 standard key), the
assembly structure under schema 28101 and the types added to the 15003 and
20000 profiles. Codes and ordinals are authored here independently of the
runtime profile declarations; only these synthetic fixtures choose the keys.
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


def test_revised_profiles_list_their_added_types():
    profiles = {p.profile_id: p for p in builtin_profiles()}
    # 15003 +UNICODE_VALUES, 20000 +SPUN_SURF, 28101 +ASSEMBLY/INSTANCE/TRANSFORM.
    assert profiles["icad-1500000-15003-r3"].type_count == 38
    assert profiles["icad-2401000-20000-r5"].type_count == 50
    assert profiles["icad-2901000-28101-r3"].type_count == 29
    new = profiles["parasolid-3100000-31001-r1"]
    assert (new.provider_schema, new.schema_keys) == ("31001", ("SCH_3100000_31001",))
    family = profiles["family-31001"]
    assert [m.profile_id for m in family.members] == ["parasolid-3100000-31001-r1"]
    assert family.type_count == new.type_count
    for retired in ("icad-1500000-15003-r2", "icad-2401000-20000-r4", "icad-2901000-28101-r2"):
        assert retired not in profiles
