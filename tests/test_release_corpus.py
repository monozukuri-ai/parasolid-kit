from __future__ import annotations

import copy
import json
import shutil
import sys
from pathlib import Path

import pytest

from parasolid_kit import _core
from scripts import release_corpus_runtime as runtime
from scripts import verify_release_corpus as gate
from tests.support.release_fixture import release_fixtures

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def corpus(tmp_path: Path) -> Path:
    shutil.copytree(ROOT / "corpus/expected", tmp_path / "expected")
    generated = tmp_path / "generated/synthetic/release-v1"
    generated.mkdir(parents=True)
    for name, data in release_fixtures().items():
        (generated / name).write_bytes(data)
    for name in ("manifest.jsonl", "release-checks.json"):
        shutil.copyfile(ROOT / "corpus" / name, tmp_path / name)
    return tmp_path


def rules(root: Path) -> dict:
    return json.loads((root / "release-checks.json").read_text())


def save_rules(root: Path, value: dict) -> None:
    (root / "release-checks.json").write_text(json.dumps(value))


def entries(root: Path) -> list[dict]:
    return [json.loads(line) for line in (root / "manifest.jsonl").read_text().splitlines()]


def save_entries(root: Path, values: list[dict]) -> None:
    (root / "manifest.jsonl").write_text("".join(json.dumps(v) + "\n" for v in values))


@pytest.fixture
def rust_reply(corpus: Path, monkeypatch: pytest.MonkeyPatch) -> dict:
    """Protocol double only; CI runs the real Rust example against this same corpus."""
    probe = corpus / "rust-probe"
    probe.write_bytes(b"test protocol double")
    replies = {}
    for entry, case in zip(entries(corpus), rules(corpus)["cases"], strict=True):
        if case["stage"] == "diagnostic":
            replies[entry["path"]] = {"status": "diagnostic", **case["diagnostic"]}
        else:
            snapshot = json.loads((corpus / case["baseline"]["path"]).read_text())
            doc = snapshot["document"]
            replies[entry["path"]] = {
                "status": "parsed",
                "schema_key": doc["schema_key"]["raw"],
                "profile": doc["schema_resolution"],
                "brep": None,
                "termination": [doc["terminator"]["byte_range"][k] for k in ("start", "end")],
                "nodes": [
                    {
                        "node_type": n["node_type"],
                        "index": n["index"],
                        "variable_length": n["variable_length"],
                        "range": [n["byte_range"][k] for k in ("start", "end")],
                        "fields": [
                            {
                                "name": f["definition"]["name"],
                                "code": f["definition"]["field_type"],
                                "range": [f["byte_range"][k] for k in ("start", "end")],
                                "values": [v["value"] for v in f["values"]],
                            }
                            for f in n["fields"]
                        ],
                    }
                    for n in doc["nodes"]
                ],
                "value_reencoding": True,
                "schema_blobs_replayed": False,
            }
        replies[entry["path"]]["core_version"] = _core.CORE_VERSION
    original = gate.run_json

    def run(command, timeout):
        if command[0] == str(probe):
            result = replies[Path(command[1]).relative_to(corpus).as_posix()]
            return (0 if result["status"] == "parsed" else 1), copy.deepcopy(result)
        return original(command, timeout)

    monkeypatch.setattr(gate, "run_json", run)
    return replies


def verify(root: Path, **kwargs) -> dict:
    return gate.verify_release_corpus(
        root / "manifest.jsonl",
        root / "release-checks.json",
        root=root,
        rust_probe=root / "rust-probe",
        **kwargs,
    )


def test_public_probes_are_reproducible_and_diagnostics_are_counted_separately(corpus, rust_reply):
    for name, data in release_fixtures().items():
        assert (corpus / "generated/synthetic/release-v1" / name).read_bytes() == data
    report = verify(corpus)
    assert report["status"] == "passed", report
    assert report["counts"] == {"required": 3, "parsed": 2, "expected_diagnostics": 1, "failed": 0}
    assert report["cases"][0]["pair_equivalent"] is True
    assert report["cases"][2]["cli_exits"] == {"parse": 2, "check": 2}


@pytest.mark.parametrize("change", ["empty", "missing", "duplicate", "missing_pair"])
def test_required_cases_cannot_disappear_or_repeat(corpus, change):
    values = entries(corpus)
    spec = rules(corpus)
    if change == "empty":
        values = []
        spec["cases"] = []
    elif change == "missing":
        values.pop()
    elif change == "duplicate":
        spec["cases"].append(copy.deepcopy(spec["cases"][0]))
    else:
        spec["cases"][0]["pair_id"] = "absent"
    save_entries(corpus, values)
    save_rules(corpus, spec)
    report = verify(corpus)
    assert report["status"] == "failed" and report["errors"]
    assert report["counts"]["parsed"] == 0


@pytest.mark.parametrize("change", ["missing", "hash", "size", "traversal", "symlink"])
def test_input_integrity_is_checked_before_parsing(corpus, change):
    values = entries(corpus)
    path = corpus / values[0]["path"]
    if change == "missing":
        path.unlink()
    elif change == "hash":
        path.write_bytes(b"U" + path.read_bytes()[1:])
    elif change == "size":
        path.write_bytes(path.read_bytes() + b" ")
    elif change == "traversal":
        values[0]["path"] = "../" + values[0]["path"]
    else:
        target = path.with_name("actual.x_t")
        path.rename(target)
        path.symlink_to(target)
    save_entries(corpus, values)
    report = verify(corpus)
    assert report["status"] == "failed" and report["errors"]
    assert report["counts"]["parsed"] == 0


@pytest.mark.parametrize("change", ["profile", "baseline", "diagnostic", "unmeasured", "range"])
def test_result_regressions_fail_even_when_input_hashes_match(corpus, rust_reply, change):
    spec = rules(corpus)
    if change == "profile":
        spec["cases"][0]["profile"]["profile_sha256"] = "0" * 64
    elif change == "baseline":
        artifact = spec["cases"][0]["baseline"]
        path = corpus / artifact["path"]
        snapshot = json.loads(path.read_text())
        snapshot["document"]["nodes"][0]["fields"][0]["values"][0]["value"] = 42
        path.write_text(json.dumps(snapshot))
        artifact.update(sha256=gate.sha256(path), bytes=path.stat().st_size)
    elif change == "diagnostic":
        spec["cases"][2]["diagnostic"]["offset"] += 1
    else:
        reply = rust_reply[entries(corpus)[0]["path"]]
        if change == "unmeasured":
            reply["value_reencoding"] = None
        else:
            reply["nodes"][0]["range"][1] -= 1
    save_rules(corpus, spec)
    report = verify(corpus)
    assert report["status"] == "failed", report
    assert report["counts"]["failed"] >= 1


def test_unexpected_parse_failure_is_not_an_expected_diagnostic(corpus, rust_reply):
    values = entries(corpus)
    spec = rules(corpus)
    path = corpus / values[1]["path"]
    path.write_bytes(path.read_bytes()[:-1])
    values[1]["sha256"] = gate.sha256(path)
    spec["cases"][1]["bytes"] = path.stat().st_size
    save_entries(corpus, values)
    save_rules(corpus, spec)
    report = verify(corpus)
    assert report["status"] == "failed"
    assert report["counts"]["expected_diagnostics"] == 1


def test_missing_required_oracle_does_not_become_a_skip(corpus):
    spec = rules(corpus)
    case = spec["cases"][0]
    case["stage"] = "brep"
    case["oracle"] = {
        "kind": "onshape_analytic",
        "unit": "m",
        "microversion": "fixture-v1",
        **{k: case["baseline"] for k in ("step", "body_details", "mass_properties")},
        **{
            k: 1e-10
            for k in (
                "linear_tolerance",
                "direction_tolerance",
                "area_tolerance",
                "volume_tolerance",
                "relative_tolerance",
            )
        },
        "core_metrics": ["volume"],
    }
    save_rules(corpus, spec)
    (corpus / "rust-probe").write_bytes(b"unused")
    report = verify(corpus)
    assert report["status"] == "failed"
    assert any("oracle interpreter" in e for e in report["errors"])


def test_worker_checks_cli_exit_and_json(corpus, monkeypatch):
    monkeypatch.setattr(runtime, "cli", lambda *args: (2, {}))
    with pytest.raises(ValueError, match="CLI inspect differs"):
        runtime.probe(corpus / entries(corpus)[0]["path"], "text", "raw")


def test_subprocess_timeout_retains_stderr():
    with pytest.raises(ValueError, match=r"timeout.*timeout-marker"):
        gate.run_json(
            [
                sys.executable,
                "-I",
                "-c",
                "import sys,time; print('timeout-marker',file=sys.stderr,flush=True); "
                "time.sleep(10)",
            ],
            1,
        )


@pytest.mark.parametrize(
    "operation", ["open('forbidden.sch_txt')", "__import__('socket').socket()"]
)
def test_runtime_guard_rejects_catalog_and_network(operation):
    code = f"""
import runpy,sys
guard = runpy.run_path({str(ROOT / "scripts/release_corpus_runtime.py")!r})['runtime_guard']
sys.addaudithook(guard)
try:
    {operation}
except RuntimeError as error:
    import json
    print(json.dumps({{'error':str(error)}}))
else:
    raise AssertionError('runtime access was allowed')
"""
    status, result = gate.run_json([sys.executable, "-I", "-c", code], 10)
    assert status == 0 and "prohibits" in result["error"]


def test_release_schema_is_valid():
    gate.Draft202012Validator.check_schema(
        json.loads((ROOT / "corpus/release-checks.schema.json").read_text())
    )
