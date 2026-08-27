from __future__ import annotations

import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator, ValidationError

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "corpus" / "manifest.schema.json"


def _valid_entry() -> dict[str, object]:
    return {
        "id": "synthetic-box-v30-binary",
        "lineage_id": "synthetic-box-001",
        "source_kind": "synthetic",
        "generator": {
            "product": "parasolid-kit test builder",
            "product_version": "0.1.0.dev0",
            "document_version": "fixture-v1",
            "export_settings": {"purpose": "unit-test"},
        },
        "recipe": "tests/support/parasolid_binary.py",
        "parasolid_version": "30.0",
        "encoding": "binary",
        "path": "generated/synthetic/box/v30/model.x_b",
        "sha256": "0" * 64,
        "redistribution": "allowed",
        "expected": {
            "schema": "SCH_3000000_30000_13006",
            "required_node_types": [],
            "node_count": 0,
        },
    }


def test_manifest_schema_is_valid_draft_2020_12() -> None:
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))

    Draft202012Validator.check_schema(schema)
    Draft202012Validator(schema).validate(_valid_entry())


def test_manifest_rejects_unknown_redistribution_state() -> None:
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    entry = _valid_entry()
    entry["redistribution"] = "probably"

    with pytest.raises(ValidationError):
        Draft202012Validator(schema).validate(entry)
