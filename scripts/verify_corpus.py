#!/usr/bin/env python3
"""Verify provenance, checksums, and redistribution status of the public corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
from typing import Any

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CORPUS_DIR = ROOT / "corpus"
TEXT_SUFFIXES = {".x_t", ".xt"}
BINARY_SUFFIXES = {".x_b", ".xb"}


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus-dir", type=Path, default=DEFAULT_CORPUS_DIR)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--schema", type=Path)
    return parser.parse_args()


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _manifest_entries(path: Path, errors: list[str]) -> list[tuple[int, dict[str, Any]]]:
    if not path.exists():
        return []
    entries: list[tuple[int, dict[str, Any]]] = []
    for line_number, raw_line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not raw_line.strip():
            continue
        try:
            value = json.loads(raw_line)
        except json.JSONDecodeError as error:
            errors.append(f"manifest line {line_number}: invalid JSON: {error.msg}")
            continue
        if not isinstance(value, dict):
            errors.append(f"manifest line {line_number}: entry must be a JSON object")
            continue
        entries.append((line_number, value))
    return entries


def verify_corpus(
    corpus_dir: Path,
    *,
    manifest_path: Path | None = None,
    schema_path: Path | None = None,
) -> dict[str, object]:
    """Return a deterministic report for the distributable corpus."""

    corpus_dir = corpus_dir.resolve()
    manifest_path = (manifest_path or corpus_dir / "manifest.jsonl").resolve()
    schema_path = (schema_path or corpus_dir / "manifest.schema.json").resolve()
    errors: list[str] = []

    try:
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(schema)
    except (OSError, json.JSONDecodeError) as error:
        return {
            "status": "failed",
            "corpus_dir": str(corpus_dir),
            "manifest": str(manifest_path),
            "entries": 0,
            "files": 0,
            "errors": [f"cannot load manifest schema: {error}"],
        }

    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    entries = _manifest_entries(manifest_path, errors)
    identifiers: set[str] = set()
    declared_paths: set[str] = set()

    for line_number, entry in entries:
        schema_errors = sorted(validator.iter_errors(entry), key=lambda item: list(item.path))
        for error in schema_errors:
            location = ".".join(str(item) for item in error.path) or "entry"
            errors.append(f"manifest line {line_number} ({location}): {error.message}")
        if schema_errors:
            continue

        identifier = entry["id"]
        if identifier in identifiers:
            errors.append(f"manifest line {line_number}: duplicate id: {identifier}")
        identifiers.add(identifier)

        if entry["redistribution"] != "allowed":
            errors.append(
                f"manifest line {line_number}: public entry is not cleared for redistribution: "
                f"{entry['redistribution']}"
            )
        if not entry.get("license"):
            errors.append(
                f"manifest line {line_number}: public entry has no license/permission basis"
            )

        path_text = entry["path"]
        relative = PurePosixPath(path_text)
        if (
            relative.is_absolute()
            or ".." in relative.parts
            or "." in relative.parts
            or not relative.parts
            or relative.as_posix() != path_text
        ):
            errors.append(f"manifest line {line_number}: path is not normalized: {path_text}")
            continue
        if relative.parts[0] != "generated":
            errors.append(
                f"manifest line {line_number}: public path must be under generated/: {path_text}"
            )
            continue
        if path_text in declared_paths:
            errors.append(f"manifest line {line_number}: duplicate path: {path_text}")
        declared_paths.add(path_text)

        suffix = relative.suffix.lower()
        expected_suffixes = TEXT_SUFFIXES if entry["encoding"] == "text" else BINARY_SUFFIXES
        if suffix not in expected_suffixes:
            errors.append(
                f"manifest line {line_number}: encoding {entry['encoding']} does not match "
                f"{path_text}"
            )

        artifact = corpus_dir.joinpath(*relative.parts)
        if artifact.is_symlink():
            errors.append(
                f"manifest line {line_number}: symlinks are not distributable: {path_text}"
            )
        elif not artifact.is_file():
            errors.append(f"manifest line {line_number}: file does not exist: {path_text}")
        else:
            actual_sha256 = _sha256(artifact)
            if actual_sha256 != entry["sha256"]:
                errors.append(
                    f"manifest line {line_number}: SHA-256 mismatch for {path_text}: "
                    f"expected {entry['sha256']}, got {actual_sha256}"
                )

    generated_dir = corpus_dir / "generated"
    actual_paths = (
        {
            path.relative_to(corpus_dir).as_posix()
            for path in generated_dir.rglob("*")
            if path.is_file() or path.is_symlink()
        }
        if generated_dir.exists()
        else set()
    )
    for undeclared in sorted(actual_paths - declared_paths):
        errors.append(f"generated file has no manifest entry: {undeclared}")

    return {
        "status": "passed" if not errors else "failed",
        "corpus_dir": str(corpus_dir),
        "manifest": str(manifest_path),
        "entries": len(entries),
        "files": len(actual_paths),
        "errors": errors,
    }


def main() -> int:
    """Run the corpus gate and return a process status."""

    arguments = _arguments()
    report = verify_corpus(
        arguments.corpus_dir,
        manifest_path=arguments.manifest,
        schema_path=arguments.schema,
    )
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
