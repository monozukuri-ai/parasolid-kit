"""Distribution runtime guards and synthetic compiled-profile fuzz seed reachability."""

from __future__ import annotations

import subprocess
import sys

import pytest

from parasolid_kit import (
    LimitExceededError,
    ParseError,
    ParseLimits,
    compare_documents,
    map_brep,
    parse_xb,
    parse_xt,
)
from scripts.prepare_fuzz_corpus import build_seeds
from scripts.verify_isolated_install import RUNTIME_GUARD_CODE


@pytest.mark.parametrize(
    "name",
    [
        "integers",
        "unicode",
        "body",
        "base-integers",
        "embedded-unchanged",
        "embedded-copy",
        "embedded-insert-delete-append",
        "solidworks-world",
        "solidworks-cone",
        "base-cone",
        "embedded-cone",
        "base-intersection",
        "embedded-intersection",
        "embedded-full204",
        "embedded-trimmed",
        "base-trimmed",
        "base-spcurve",
    ],
)
def test_fuzz_seeds_reach_builtin_field_readers(name):
    seeds = build_seeds()
    text, binary = parse_xt(seeds[f"{name}-text"]), parse_xb(seeds[f"{name}-binary"])
    assert compare_documents(text, binary).equivalent
    assert text.schema_resolution.kind == "builtin"
    assert text.nodes[0].fields and binary.nodes[0].fields
    if name == "integers":
        assert [v.value for v in text.nodes[0].fields[0].values] == [7, None]
    if name == "unicode":
        assert [v.value for v in text.nodes[0].fields[0].values] == [0x6587, 0xD83D, 0xDE00]
    if name == "embedded-insert-delete-append":
        assert [field.definition.name for field in text.nodes[0].fields] == ["flag", "value"]
        assert [field.values[0].value for field in text.nodes[0].fields] == [7, 0.125]


@pytest.mark.parametrize("encoding,parse", [("text", parse_xt), ("binary", parse_xb)])
@pytest.mark.parametrize("profile", ["v30", "v13", "icad", "solidworks"])
def test_fuzz_seeds_reach_brep_topology_for_every_compiled_profile(encoding, parse, profile):
    document = parse(build_seeds()[f"brep-{profile}-{encoding}"])
    model = map_brep(document, limits=ParseLimits(max_diagnostics=256))
    assert model.complete
    assert (len(model.bodies), len(model.regions), len(model.shells)) == (1, 1, 1)


@pytest.mark.parametrize("encoding,parse", [("text", parse_xt), ("binary", parse_xb)])
@pytest.mark.parametrize("name", ["base-cone", "embedded-cone", "solidworks-cone", "base-trimmed"])
def test_fuzz_seeds_reach_brep_geometry_readers(encoding, parse, name):
    model = map_brep(parse(build_seeds()[f"brep-{name}-{encoding}"]))
    assert model.complete
    assert len(model.curves) == (2 if name == "base-trimmed" else 0)
    assert len(model.surfaces) == (0 if name == "base-trimmed" else 1)


@pytest.mark.parametrize("encoding,parse", [("text", parse_xt), ("binary", parse_xb)])
def test_fuzz_reference_cycle_reaches_semantic_rejection(encoding, parse):
    document = parse(build_seeds()[f"reference-cycle-{encoding}"])
    with pytest.raises(ParseError) as captured:
        map_brep(document)
    assert captured.value.diagnostic.code == "topology.invalid_relationship"
    assert "cycle" in str(captured.value)


@pytest.mark.parametrize("encoding,parse", [("text", parse_xt), ("binary", parse_xb)])
@pytest.mark.parametrize("node_type", [3, 4])
def test_fuzz_solidworks_delta_membership_fails_closed(encoding, parse, node_type):
    with pytest.raises(ParseError) as captured:
        parse(build_seeds()[f"solidworks-unknown-{node_type}-{encoding}"])
    assert captured.value.diagnostic.code == "schema.unknown_base_type"


@pytest.mark.parametrize("name", [name for name in build_seeds() if name.startswith("truncated-")])
def test_fuzz_invalid_wire_seeds_are_rejected(name):
    parse = parse_xt if name.endswith("text") else parse_xb
    with pytest.raises(ParseError):
        parse(build_seeds()[name])


@pytest.mark.parametrize("encoding,parse", [("text", parse_xt), ("binary", parse_xb)])
@pytest.mark.parametrize("name", ["nan", "infinity"])
def test_fuzz_nonfinite_values_reach_semantic_rejection(encoding, parse, name):
    document = parse(build_seeds()[f"{name}-{encoding}"])
    with pytest.raises(ParseError) as captured:
        map_brep(document)
    assert captured.value.diagnostic.code == "geometry.invalid_parameter"


@pytest.mark.parametrize("encoding,parse", [("text", parse_xt), ("binary", parse_xb)])
@pytest.mark.parametrize(
    "field,limit,resource",
    [
        ("max_schema_types", 1, "schema_types"),
        ("max_string_bytes", 8, "string_bytes"),
        ("max_file_size", 1, "file_size"),
    ],
)
def test_fuzz_inputs_respect_schema_string_and_file_limits(encoding, parse, field, limit, resource):
    with pytest.raises((ParseError, LimitExceededError)) as captured:
        parse(build_seeds()[f"brep-v30-{encoding}"], limits=ParseLimits(**{field: limit}))
    assert captured.value.diagnostic.code == "limits.exceeded"
    expected_resource = (
        "modeller_version" if field == "max_string_bytes" and encoding == "text" else resource
    )
    assert captured.value.diagnostic.details["resource"] == expected_resource


@pytest.mark.parametrize("encoding,parse", [("text", parse_xt), ("binary", parse_xb)])
def test_unknown_embedded_fuzz_seed_stops_before_definition_decoding(encoding, parse):
    with pytest.raises(ParseError) as captured:
        parse(build_seeds()[f"embedded-unknown-{encoding}"])
    assert captured.value.diagnostic.code == "schema.unknown_base_type"


@pytest.mark.parametrize("encoding,parse", [("text", parse_xt), ("binary", parse_xb)])
@pytest.mark.parametrize(
    "name,limits",
    [
        ("array-limit", ParseLimits(max_variable_elements=4096)),
        ("node-limit", ParseLimits(max_nodes=1024)),
        ("body", ParseLimits(max_fields_per_type=1)),
    ],
)
def test_builtin_fuzz_inputs_respect_node_field_and_array_limits(encoding, parse, name, limits):
    with pytest.raises(ParseError) as captured:
        parse(build_seeds()[f"{name}-{encoding}"], limits=limits)
    assert captured.value.diagnostic.code == "limits.exceeded"
    expected = {
        "array-limit": ("field_elements", 4097, 4096),
        "node-limit": ("nodes", 1025, 1024),
        "body": ("schema_fields_per_type", 33, 1),
    }[name]
    assert (
        tuple(captured.value.diagnostic.details[k] for k in ("resource", "actual", "limit"))
        == expected
    )


@pytest.mark.parametrize("blocked", ["network", "checkout", "catalog"])
def test_isolated_runtime_guards_block_undeclared_inputs(tmp_path, blocked):
    checkout = tmp_path / "checkout"
    checkout.mkdir()
    (checkout / "hidden.txt").write_text("hidden")
    (tmp_path / "sch_30000.sch_txt").write_text("catalog")
    expressions = {
        "network": "import socket; socket.socket()",
        "checkout": "Path(sys.argv[1], 'hidden.txt').read_text()",
        "catalog": "Path('sch_30000.sch_txt').read_text()",
    }
    result = subprocess.run(
        [
            sys.executable,
            "-I",
            "-c",
            RUNTIME_GUARD_CODE + "\n" + expressions[blocked],
            str(checkout),
            str(tmp_path / "environment"),
        ],
        cwd=tmp_path,
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0
    assert "during isolated parser runtime" in result.stderr


def test_isolated_guard_allows_only_separately_supplied_fixture(tmp_path):
    (tmp_path / "input.x_t").write_bytes(b"fixture")
    result = subprocess.run(
        [
            sys.executable,
            "-I",
            "-c",
            RUNTIME_GUARD_CODE + "\nassert Path('input.x_t').read_bytes() == b'fixture'",
            str(tmp_path / "checkout"),
            str(tmp_path / "environment"),
        ],
        cwd=tmp_path,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr
