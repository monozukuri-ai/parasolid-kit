"""Distribution runtime guards and synthetic compiled-profile fuzz seed reachability."""

from __future__ import annotations

import subprocess
import sys

import pytest

from parasolid_kit import ParseError, ParseLimits, compare_documents, parse_xb, parse_xt
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
