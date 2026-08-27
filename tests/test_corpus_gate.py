from __future__ import annotations

import hashlib
import json
from pathlib import Path

from scripts.verify_corpus import verify_corpus

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "corpus" / "manifest.schema.json"


def _entry(path: str, payload: bytes) -> dict[str, object]:
    return {
        "id": "synthetic-box-v30-text",
        "lineage_id": "synthetic-box-001",
        "source_kind": "synthetic",
        "generator": {
            "product": "parasolid-kit test builder",
            "product_version": "0.1.0.dev0",
            "document_version": "fixture-v1",
        },
        "recipe": "tests/support/parasolid_text.py",
        "parasolid_version": "30.0",
        "encoding": "text",
        "path": path,
        "sha256": hashlib.sha256(payload).hexdigest(),
        "redistribution": "allowed",
        "license": "project test-fixture permission",
        "expected": {
            "schema": "SCH_3000000_30000",
            "required_node_types": [],
            "node_count": 0,
        },
    }


def _write_manifest(corpus_dir: Path, entries: list[dict[str, object]]) -> None:
    lines = "".join(f"{json.dumps(entry, sort_keys=True)}\n" for entry in entries)
    (corpus_dir / "manifest.jsonl").write_text(lines, encoding="utf-8")


def test_empty_distributable_corpus_passes(tmp_path: Path) -> None:
    corpus_dir = tmp_path / "corpus"
    corpus_dir.mkdir()

    report = verify_corpus(corpus_dir, schema_path=SCHEMA_PATH)

    assert report["status"] == "passed"
    assert report["entries"] == 0
    assert report["files"] == 0


def test_allowed_manifest_entry_must_match_the_file_checksum(tmp_path: Path) -> None:
    corpus_dir = tmp_path / "corpus"
    artifact = corpus_dir / "generated" / "synthetic" / "model.x_t"
    artifact.parent.mkdir(parents=True)
    payload = b"synthetic X_T fixture"
    artifact.write_bytes(payload)
    _write_manifest(corpus_dir, [_entry("generated/synthetic/model.x_t", payload)])

    report = verify_corpus(corpus_dir, schema_path=SCHEMA_PATH)

    assert report["status"] == "passed"
    assert report["entries"] == 1
    assert report["files"] == 1


def test_undeclared_generated_file_fails(tmp_path: Path) -> None:
    corpus_dir = tmp_path / "corpus"
    artifact = corpus_dir / "generated" / "orphan.x_b"
    artifact.parent.mkdir(parents=True)
    artifact.write_bytes(b"orphan")

    report = verify_corpus(corpus_dir, schema_path=SCHEMA_PATH)

    assert report["status"] == "failed"
    assert report["errors"] == ["generated file has no manifest entry: generated/orphan.x_b"]


def test_public_entry_requires_normalized_path_rights_and_license(tmp_path: Path) -> None:
    corpus_dir = tmp_path / "corpus"
    corpus_dir.mkdir()
    entry = _entry("local/../generated/model.x_t", b"payload")
    entry["redistribution"] = "unknown"
    entry.pop("license")
    _write_manifest(corpus_dir, [entry])

    report = verify_corpus(corpus_dir, schema_path=SCHEMA_PATH)

    assert report["status"] == "failed"
    assert any("path is not normalized" in error for error in report["errors"])
    assert any("not cleared for redistribution" in error for error in report["errors"])
    assert any("has no license/permission basis" in error for error in report["errors"])
