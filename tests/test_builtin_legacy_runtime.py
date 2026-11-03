"""The older standard keys: the reference's own transmit example and the compiled listing.

The public XT Format Reference prints one complete transmit file (printed page
10): a disc under ``SCH_1200000_12006`` whose face carries a colour attribute.
It is authored here verbatim and must decode under the 12006 profile, which
declares exactly its node types; the other keys of Parasolid 9 to 16 are
checked through the compiled listing. No catalog is required and the public
files are not part of the tests.
"""

from __future__ import annotations

import pytest

from parasolid_kit import SchemaError, builtin_profiles, map_brep, parse_xt
from tests.support.parasolid_text import text_header

REFERENCE_KEY = "SCH_1200000_12006"
REFERENCE_MODELLER = ": TRANSMIT FILE created by modeller version 1200000"
# The reference's example, line by line (the annotations are not part of it).
REFERENCE_EXAMPLE = (
    "12 1 12 0 2 0 0 0 0 1e3 1e-8 0 0 0 1 0 3 1 3 4 5 0 6 7 0 "
    "70 2 0 1 0 0 4 1 20 8 8 8 1 T"
    "13 3 3 0 1 0 9 0 0 6 9 "
    "50 4 11 0 9 0 0 0 +0 0 0 0 0 1 1 0 0 "
    "31 5 10 0 7 0 0 0 +0 0 0 0 0 1 1 0 0 1 "
    "19 6 5 0 1 0 0 3 V"
    "16 7 6 0 ?10 0 0 5 0 0 1 "
    "17 10 0 11 10 10 0 12 7 0 0 +"
    "15 11 7 0 10 9 0 "
    "17 12 0 0 0 0 0 10 7 0 0 -"
    "14 9 2 13 ?0 0 11 3 4 +0 0 0 0 3 "
    "81 1 13 12 14 9 0 0 0 0 15 "
    "80 1 14 0 16 8001 0 0 0 0 3 5 0 0 FFFFTFTFFFFFF2 "
    "83 3 15 1 2 3 "
    "79 15 16 SDL/TYSA_COLOUR"
    "74 20 8 1 0 13 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 "
    "1 0 "
)

LEGACY = {
    "SCH_900141_9003": ("nx-900141-9003-r1", "9003"),
    "SCH_901297_9008": ("microstation-901297-9008-r1", "9008"),
    "SCH_1000252_10004": ("nx-1000252-10004-r1", "10004"),
    "SCH_1100129_11004": ("ironcad-1100129-11004-r1", "11004"),
    REFERENCE_KEY: ("solidworks-1200000-12006-r1", "12006"),
    "SCH_1600000_16004": ("nx-1600000-16004-r1", "16004"),
}


def test_the_reference_transmit_example_decodes_and_maps_under_its_own_key():
    data = text_header(REFERENCE_KEY, modeller_version=REFERENCE_MODELLER) + (
        REFERENCE_EXAMPLE.encode()
    )
    doc = parse_xt(data)
    assert doc.schema_resolution.profile_id == "solidworks-1200000-12006-r1"
    assert doc.schema_resolution.coverage == "verified_subset"
    assert len(doc.nodes) == 16
    assert all(s.definition.source.value == "base" and not s.edits for s in doc.schemas)
    body = doc.nodes[0]
    assert len(body.fields) == 23
    assert [f.definition.name for f in body.fields][14:17] == [
        "body_kind",
        "geometry_state",
        "legacy_shell",
    ]
    fin = doc.nodes[7]
    assert (fin.node_type, fin.index, len(fin.fields)) == (17, 10, 10)
    definition = doc.nodes[12]
    assert [f.definition.name for f in definition.fields] == [
        "next_definition",
        "identifier_ref",
        "kind_id",
        "event_actions",
        "allowed_owners",
        "value_kinds",
    ]
    assert len(definition.fields[4].values) == 13
    model = map_brep(doc)
    assert model.complete
    assert len(model.faces) == 1 and len(model.edges) == 1 and not model.vertices
    assert model.colour_of("face", model.faces[0].id) == (1.0, 2.0, 3.0)


def test_the_12006_profile_declares_only_the_example_types():
    data = text_header(REFERENCE_KEY, modeller_version=REFERENCE_MODELLER) + (
        REFERENCE_EXAMPLE.replace("83 3 15 1 2 3 ", "82 3 15 1 2 3 ").encode()
    )
    with pytest.raises(SchemaError, match=r"schema\.builtin_profile_uncovered_type"):
        parse_xt(data)


def test_the_older_keys_are_listed_with_their_families():
    by_id = {p.profile_id: p for p in builtin_profiles()}
    for key, (profile_id, schema) in LEGACY.items():
        profile = by_id[profile_id]
        assert profile.schema_keys == (key,)
        assert profile.provider_schema == schema
        assert profile.coverage == "verified_subset"
        family = by_id[f"family-{schema}"]
        assert [m.profile_id for m in family.members] == [profile_id]
        assert family.base_members == ()
