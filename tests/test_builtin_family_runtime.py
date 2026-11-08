"""Schema-family selection: other builds of a reviewed schema parse without an exact profile.

A transmit key is ``SCH_<modeller>_<effective>[_<base>]``. The effective schema
and the base fix the node layouts; the modeller component is the producer's
Parasolid build number. The registry derives one family profile per
``<effective>[-<base>]`` from the exact profiles that share it. These fixtures
substitute builds that no exact profile accepts.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

from parasolid_kit import SchemaError, builtin_profiles, parse_xb, parse_xt, read_brep
from parasolid_kit.cli import main
from parasolid_kit.schema import SchemaKey
from tests.test_builtin_embedded_runtime import EMBEDDED, general_body
from tests.test_builtin_schema_runtime import KEY as V30
from tests.test_builtin_schema_runtime import general_body_payload

ROOT = Path(__file__).resolve().parents[1]
OTHER_V30_BUILD = "SCH_3000999_30000"
OTHER_EMBEDDED_BUILD = "SCH_3000399_30000_13006"
NO_FAMILY = "SCH_3000310_30900_13006"


def parser(encoding):
    return parse_xt if encoding == "x_t" else parse_xb


def test_family_listing_follows_the_exact_profiles():
    profiles = builtin_profiles()
    exact = [p for p in profiles if p.coverage == "verified_subset"]
    families = [p for p in profiles if p.coverage == "family_inferred"]
    assert len(exact) == 62
    assert len(families) == 36
    by_id = {p.profile_id: p for p in profiles}
    assert len(by_id) == len(profiles)
    for profile in exact:
        assert profile.family_id is None
        assert profile.members == ()
        assert profile.schema_keys
        if profile.producer_scope.startswith("TopSolid"):
            continue
        for key in profile.schema_keys:
            family = by_id[f"family-{SchemaKey.parse(key).family_id}"]
            assert profile.profile_id in {m.profile_id for m in family.members}
            assert family.provider_schema == profile.provider_schema
    for family in families:
        assert family.profile_id == f"family-{family.family_id}"
        assert family.profile_revision == 1
        assert family.schema_keys == ()
        assert family.conflicting_types == ()
        assert re.fullmatch(r"[0-9a-f]{64}", family.profile_sha256)
        members = [m.profile_id for m in family.members]
        assert members == sorted(members)
        for member in family.members:
            assert by_id[member.profile_id].profile_sha256 == member.profile_sha256
            assert by_id[member.profile_id].profile_revision == member.profile_revision
        assert family.type_count >= max(by_id[m].type_count for m in members)
    assert {m.profile_id for m in by_id["family-35102-13006"].members} == {
        "solidworks-3501210-35102-13006-r2",
        "solidworks-3501256-35102-13006-r2",
    }
    assert by_id["family-37102-13006"].absent_base_types == (176, 204)
    assert by_id["family-30000"].members[0].profile_id == "onshape-sch30000-r3"
    assert by_id["family-30000"].to_dict()["members"][0]["profile_id"] == "onshape-sch30000-r3"


def test_schema_key_family_drops_the_modeller_component():
    assert SchemaKey.parse("SCH_3501251_35102_13006").family_id == "35102-13006"
    assert SchemaKey.parse("SCH_3700000_36001").family_id == "36001"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_other_v30_build_maps_a_general_body_through_the_family(encoding):
    data = general_body_payload(encoding).replace(V30.encode(), OTHER_V30_BUILD.encode())
    result = read_brep(data)
    resolution = result.document.schema_resolution.to_dict()
    assert resolution == {
        "kind": "builtin",
        "profile_id": "family-30000",
        "profile_revision": 1,
        "schema_key": OTHER_V30_BUILD,
        "coverage": "family_inferred",
        "profile_sha256": resolution["profile_sha256"],
    }
    assert result.brep.complete and result.brep.topology.valid
    assert len(result.brep.bodies) == 1
    assert result.summary.schema_resolution == result.document.schema_resolution
    assert result.summary.to_dict()["schema_resolution"]["coverage"] == "family_inferred"


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_other_embedded_build_maps_copied_body_roles_through_the_family(encoding):
    data = general_body(encoding, EMBEDDED)
    result = read_brep(data.replace(EMBEDDED.encode(), OTHER_EMBEDDED_BUILD.encode()))
    resolution = result.document.schema_resolution
    assert resolution.profile_id == "family-30000-13006"
    assert resolution.coverage == "family_inferred"
    assert resolution.schema_key == OTHER_EMBEDDED_BUILD
    assert result.brep.complete and result.brep.topology.valid
    assert len(result.brep.bodies) == 1


@pytest.mark.parametrize("encoding", ["x_t", "x_b"])
def test_keys_outside_every_family_still_fail_closed(encoding):
    data = general_body(encoding, EMBEDDED).replace(EMBEDDED.encode(), NO_FAMILY.encode())
    with pytest.raises(SchemaError) as captured:
        parser(encoding)(data)
    assert captured.value.diagnostic.code == "schema.missing_base_schema"
    assert "schema family" in captured.value.diagnostic.message


def test_cli_check_reports_the_family(tmp_path, capsys):
    path = tmp_path / "other-build.x_t"
    path.write_bytes(general_body_payload("x_t").replace(V30.encode(), OTHER_V30_BUILD.encode()))
    assert main(["check", str(path)]) == 0
    out = capsys.readouterr().out
    assert "Profile: family-30000 (revision 1, family_inferred)" in out
    assert "Schema family: 30000" in out
    assert "onshape-sch30000-r3" in out
    assert main(["check", str(path), "--json"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["summary"]["schema_resolution"]["coverage"] == "family_inferred"
    assert report["summary"]["schema_resolution"]["schema_key"] == OTHER_V30_BUILD


def test_family_documentation_matches_runtime():
    support = (ROOT / "docs/format-support.md").read_text(encoding="utf-8")
    for family in (p for p in builtin_profiles() if p.coverage == "family_inferred"):
        assert f"| `{family.family_id}` |" in support, family.family_id
        for member in family.members:
            assert member.profile_id in support
    for name in ("README.md", "docs/api.md", "docs/schema-catalogs.md"):
        assert "family_inferred" in (ROOT / name).read_text(encoding="utf-8"), name


def test_topsolid_documentation_lists_exact_profile_metadata():
    support = (ROOT / "docs/format-support.md").read_text(encoding="utf-8")
    profiles = [p for p in builtin_profiles() if p.profile_id.startswith("topsolid-v6-")]
    assert len(profiles) == 5
    for profile in profiles:
        key = profile.schema_keys[0]
        row = next(line for line in support.splitlines() if line.startswith(f"| `{key}` |"))
        assert f"`{profile.profile_id}` / {profile.profile_revision}" in row
        assert f"{profile.type_count} / {profile.field_count}" in row
        assert profile.profile_sha256 in row
