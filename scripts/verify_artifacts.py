#!/usr/bin/env python3
"""Verify that built wheel and sdist archives contain only intended files."""

from __future__ import annotations

import argparse
import base64
import configparser
import csv
import hashlib
import io
import json
import re
import stat
import tarfile
import zipfile
from email.parser import BytesParser
from pathlib import Path, PurePosixPath
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
PACKAGE_NAME = "parasolid-kit"
IMPORT_NAME = "parasolid_kit"
VERSION = "0.1.0.dev0"
LICENSE_EXPRESSION = "MIT"
RUST_PACKAGE_NAME = "parasolid-python"
RUST_PACKAGE_VERSION = "0.1.0-dev0"
RUST_SBOM_FILENAME = f"{RUST_PACKAGE_NAME}.cyclonedx.json"
APPROVED_EXTRAS = frozenset({"cadquery", "occt"})
APPROVED_EXTRA_REQUIREMENTS = {
    ("cadquery", "cadquery"): frozenset({">=2.8", "<2.9"}),
    ("cadquery-ocp-novtk", "occt"): frozenset({">=7.9.3.1", "<7.10"}),
}
EXPECTED_LICENSE_BYTES = (ROOT / "LICENSE").read_bytes()
EXPECTED_LICENSE_SHA256 = hashlib.sha256(EXPECTED_LICENSE_BYTES).hexdigest()
VIEWER_ASSET_VERSION = "1.0.0"
VIEWER_ASSET_LICENSE = "MIT"
VIEWER_ASSET_MARKER = b'content="1.0.0; license=MIT"'
VIEWER_ASSET_SHA256 = {
    "parasolid_kit/interop/preview/static/index.html": (
        "0a80d9176e27433c009cebba64671d75f1bc405b979d2ed43bea248b6e184ca4"
    ),
    "parasolid_kit/interop/preview/static/viewer.css": (
        "c6b6f8778e83ef6c16f6a42d03b864f05f3ee99fe10b272d046717e9290de296"
    ),
    "parasolid_kit/interop/preview/static/viewer.js": (
        "81094c0fbb08865157d6091fd60be9f85ffbc3cdcc60153325ee27a313351d6e"
    ),
}
NATIVE_CAD_SUFFIXES = {
    ".asm",
    ".icd",
    ".iges",
    ".igs",
    ".prt",
    ".sat",
    ".step",
    ".stp",
    ".x_b",
    ".x_t",
    ".xb",
    ".xt",
}
SDIST_ROOT_FILES = {
    ".gitignore",
    "Cargo.lock",
    "Cargo.toml",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "COPYING",
    "PKG-INFO",
    "README.md",
    "pyproject.toml",
    "uv.lock",
}
SDIST_ROOT_DIRECTORIES = {
    "corpus",
    "crates",
    "docs",
    "fuzz",
    "scripts",
    "src",
    "tests",
}
SDIST_SCRIPT_FILES = frozenset(
    {
        "scripts/inspect_xb_headers.py",
        "scripts/verify_artifacts.py",
        "scripts/verify_corpus.py",
        "scripts/verify_isolated_install.py",
        "scripts/verify_optional_install.py",
        "scripts/verify_optional_interop_i0.py",
        "scripts/verify_optional_interop_i3.py",
        "scripts/verify_optional_interop_i4.py",
        "scripts/verify_optional_interop_i5.py",
        "scripts/verify_optional_interop_i6.py",
        "scripts/verify_optional_interop_i7.py",
    }
)
_REQUIREMENT = re.compile(
    r"^\s*(?P<name>[A-Za-z0-9][A-Za-z0-9._-]*)\s*"
    r"(?P<specifier>\([^)]*\)|[^;]*?)\s*(?:;\s*(?P<marker>.+))?$"
)
_EXTRA_MARKER = re.compile(r"^extra\s*==\s*(['\"])([a-z0-9][a-z0-9._-]*)\1$")


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--sdist", type=Path, required=True)
    parser.add_argument(
        "--require-license",
        action="store_true",
        help="require MIT metadata and the matching installed/source license file",
    )
    return parser.parse_args()


def _unsafe_archive_path(name: str) -> bool:
    path = PurePosixPath(name)
    comparable = name[:-1] if name.endswith("/") else name
    return (
        not name
        or "\\" in name
        or path.is_absolute()
        or ".." in path.parts
        or "." in path.parts
        or path.as_posix() != comparable
    )


def _forbidden_path(path: PurePosixPath) -> str | None:
    lowered_parts = tuple(part.lower() for part in path.parts)
    if "__pycache__" in lowered_parts or any(part.endswith(".egg-info") for part in lowered_parts):
        return "cache/build metadata"
    if any(part in {"data", "downloads", "local", "target", "artifacts"} for part in lowered_parts):
        return "local or generated build data"
    if path.suffix.lower() in NATIVE_CAD_SUFFIXES:
        return "native CAD fixture"
    if re.fullmatch(r"sch_[0-9]+.*\.sch_txt", path.name, flags=re.IGNORECASE):
        return "Siemens schema catalog"
    return None


def _license_expression(metadata: Any) -> str | None:
    expression = metadata.get("License-Expression")
    return expression.strip() if expression and expression.strip() else None


def _verify_dependency_metadata(
    metadata: Any,
    *,
    artifact: str,
    errors: list[str],
) -> tuple[list[str], list[str]]:
    """Require the exact approved optional-dependency metadata and no base dependency."""

    provides_extra = list(metadata.get_all("Provides-Extra", []) or [])
    requires_dist = list(metadata.get_all("Requires-Dist", []) or [])
    if len(provides_extra) != len(set(provides_extra)):
        errors.append(f"{artifact} metadata contains duplicate Provides-Extra values")
    if set(provides_extra) != APPROVED_EXTRAS:
        errors.append(
            f"{artifact} Provides-Extra must be exactly {sorted(APPROVED_EXTRAS)}, "
            f"got {sorted(provides_extra)}"
        )

    observed: set[tuple[str, str]] = set()
    for requirement in requires_dist:
        match = _REQUIREMENT.fullmatch(requirement)
        if match is None:
            errors.append(f"{artifact} has an unparseable Requires-Dist: {requirement!r}")
            continue
        name = re.sub(r"[-_.]+", "-", match.group("name")).lower()
        marker = match.group("marker")
        if marker is None:
            errors.append(f"{artifact} has an unconditional Requires-Dist: {requirement!r}")
            continue
        marker_match = _EXTRA_MARKER.fullmatch(marker.strip())
        if marker_match is None:
            errors.append(f"{artifact} has a non-exact extra marker: {requirement!r}")
            continue
        extra = marker_match.group(2)
        key = (name, extra)
        if key in observed:
            errors.append(f"{artifact} has a duplicate extra requirement: {requirement!r}")
            continue
        observed.add(key)

        specifier = match.group("specifier").strip()
        if specifier.startswith("(") and specifier.endswith(")"):
            specifier = specifier[1:-1].strip()
        specifiers = frozenset(item.strip() for item in specifier.split(",") if item.strip())
        expected = APPROVED_EXTRA_REQUIREMENTS.get(key)
        if expected is None:
            errors.append(f"{artifact} has an unapproved extra requirement: {requirement!r}")
        elif specifiers != expected:
            errors.append(
                f"{artifact} has an unapproved version range for {name}[{extra}]: "
                f"{sorted(specifiers)}, expected {sorted(expected)}"
            )

    missing = set(APPROVED_EXTRA_REQUIREMENTS) - observed
    if missing:
        errors.append(f"{artifact} is missing approved extra requirements: {sorted(missing)}")
    return sorted(provides_extra), requires_dist


def _verify_rust_sbom(payload: bytes, errors: list[str]) -> None:
    """Validate maturin's optional PEP 770 Rust CycloneDX document."""

    try:
        document = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        errors.append(f"wheel Rust SBOM is not valid UTF-8 JSON: {error}")
        return
    if not isinstance(document, dict):
        errors.append("wheel Rust SBOM root must be a JSON object")
        return
    if document.get("bomFormat") != "CycloneDX":
        errors.append("wheel Rust SBOM must use the CycloneDX format")
    if document.get("specVersion") != "1.5":
        errors.append("wheel Rust SBOM must use CycloneDX 1.5")
    if document.get("version") != 1:
        errors.append("wheel Rust SBOM document version must be 1")

    metadata = document.get("metadata")
    component = metadata.get("component") if isinstance(metadata, dict) else None
    if not isinstance(component, dict):
        errors.append("wheel Rust SBOM is missing its metadata component")
        return
    if component.get("name") != RUST_PACKAGE_NAME:
        errors.append(f"wheel Rust SBOM component must be {RUST_PACKAGE_NAME}")
    if component.get("version") != RUST_PACKAGE_VERSION:
        errors.append(f"wheel Rust SBOM component version must be {RUST_PACKAGE_VERSION}")
    licenses = component.get("licenses")
    if not (
        isinstance(licenses, list)
        and any(
            isinstance(item, dict) and item.get("expression") == LICENSE_EXPRESSION
            for item in licenses
        )
    ):
        errors.append(f"wheel Rust SBOM must declare the {LICENSE_EXPRESSION} license")
    if not isinstance(document.get("components"), list):
        errors.append("wheel Rust SBOM components must be an array")
    if not isinstance(document.get("dependencies"), list):
        errors.append("wheel Rust SBOM dependencies must be an array")


def _verify_record(archive: zipfile.ZipFile, record_name: str, errors: list[str]) -> None:
    rows = list(csv.reader(io.StringIO(archive.read(record_name).decode("utf-8"))))
    recorded_names = {row[0] for row in rows if len(row) == 3}
    archive_names = {item.filename for item in archive.infolist() if not item.is_dir()}
    if recorded_names != archive_names:
        errors.append("wheel RECORD file set does not match archive members")
        return
    for row in rows:
        if len(row) != 3:
            errors.append("wheel RECORD contains a row with fields other than three")
            continue
        name, encoded_digest, encoded_size = row
        if name == record_name:
            if encoded_digest or encoded_size:
                errors.append("wheel RECORD self-entry must have empty hash and size")
            continue
        data = archive.read(name)
        expected_digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=")
        if encoded_digest != f"sha256={expected_digest.decode('ascii')}":
            errors.append(f"wheel RECORD SHA-256 mismatch: {name}")
        if encoded_size != str(len(data)):
            errors.append(f"wheel RECORD size mismatch: {name}")


def _verify_viewer_asset(
    name: str,
    payload: bytes,
    *,
    artifact: str,
    errors: list[str],
) -> None:
    expected = VIEWER_ASSET_SHA256.get(name)
    if expected is None:
        errors.append(f"{artifact} contains an unapproved viewer asset: {name}")
        return
    digest = hashlib.sha256(payload).hexdigest()
    if digest != expected:
        errors.append(f"{artifact} viewer asset SHA-256 mismatch: {name}")
    if name.endswith("/index.html") and VIEWER_ASSET_MARKER not in payload:
        errors.append(
            f"{artifact} viewer asset marker must declare "
            f"version {VIEWER_ASSET_VERSION} and license {VIEWER_ASSET_LICENSE}"
        )


def verify_wheel(path: Path, *, require_license: bool = False) -> dict[str, object]:
    """Inspect one wheel without extracting or importing it."""

    path = path.resolve()
    errors: list[str] = []
    license_expression: str | None = None
    metadata_license_file = False
    license_file = False
    license_file_sha256: str | None = None
    rust_sbom = False
    provides_extra: list[str] = []
    requires_dist: list[str] = []
    viewer_assets: set[str] = set()
    try:
        with zipfile.ZipFile(path) as archive:
            members = [item for item in archive.infolist() if not item.is_dir()]
            names = [item.filename for item in members]
            if len(names) != len(set(names)):
                errors.append("wheel contains duplicate member paths")
            for name in names:
                if _unsafe_archive_path(name):
                    errors.append(f"unsafe wheel path: {name}")
                    continue
                pure_path = PurePosixPath(name)
                forbidden = _forbidden_path(pure_path)
                if forbidden:
                    errors.append(f"wheel contains {forbidden}: {name}")
                top = pure_path.parts[0]
                if top == IMPORT_NAME:
                    if name in VIEWER_ASSET_SHA256:
                        viewer_assets.add(name)
                        _verify_viewer_asset(
                            name,
                            archive.read(name),
                            artifact="wheel",
                            errors=errors,
                        )
                    elif pure_path.name != "py.typed" and pure_path.suffix not in {
                        ".py",
                        ".pyi",
                        ".pyd",
                        ".so",
                    }:
                        errors.append(f"unexpected package file in wheel: {name}")
                elif top.startswith(f"{IMPORT_NAME}-") and top.endswith(".dist-info"):
                    dist_info_relative = pure_path.parts[1:]
                    if dist_info_relative == ("licenses", "LICENSE"):
                        license_file = True
                        license_file_sha256 = hashlib.sha256(archive.read(name)).hexdigest()
                    if not (
                        (
                            len(dist_info_relative) == 1
                            and dist_info_relative[0]
                            in {"METADATA", "RECORD", "WHEEL", "entry_points.txt"}
                        )
                        or (
                            len(dist_info_relative) >= 2
                            and dist_info_relative[0].lower() == "licenses"
                        )
                        or dist_info_relative == ("sboms", RUST_SBOM_FILENAME)
                    ):
                        errors.append(f"unexpected dist-info file in wheel: {name}")
                else:
                    errors.append(f"unexpected wheel top-level path: {name}")

            missing_viewer_assets = set(VIEWER_ASSET_SHA256) - viewer_assets
            for missing in sorted(missing_viewer_assets):
                errors.append(f"required wheel viewer asset is missing: {missing}")

            native_extensions = [
                name
                for name in names
                if name.startswith(f"{IMPORT_NAME}/_core")
                and PurePosixPath(name).suffix in {".pyd", ".so"}
            ]
            if len(native_extensions) != 1:
                errors.append(
                    "wheel must contain exactly one private native extension, "
                    f"got {native_extensions}"
                )

            metadata_names = [name for name in names if name.endswith(".dist-info/METADATA")]
            if len(metadata_names) != 1:
                errors.append("wheel must contain exactly one METADATA file")
            else:
                metadata = BytesParser().parsebytes(archive.read(metadata_names[0]))
                if metadata.get("Name") != PACKAGE_NAME:
                    errors.append(f"unexpected project name: {metadata.get('Name')}")
                if metadata.get("Version") != VERSION:
                    errors.append(f"unexpected project version: {metadata.get('Version')}")
                if metadata.get("Requires-Python") != ">=3.10":
                    errors.append(f"unexpected Requires-Python: {metadata.get('Requires-Python')}")
                provides_extra, requires_dist = _verify_dependency_metadata(
                    metadata,
                    artifact="wheel",
                    errors=errors,
                )
                license_expression = _license_expression(metadata)
                metadata_license_file = "LICENSE" in metadata.get_all("License-File", [])

            entry_point_names = [
                name for name in names if name.endswith(".dist-info/entry_points.txt")
            ]
            if len(entry_point_names) != 1:
                errors.append("wheel must contain exactly one entry_points.txt")
            else:
                entry_points = configparser.ConfigParser()
                entry_points.read_string(archive.read(entry_point_names[0]).decode("utf-8"))
                if entry_points.get("console_scripts", "parasolid-kit", fallback=None) != (
                    "parasolid_kit.cli:main"
                ):
                    errors.append("wheel does not expose the expected parasolid-kit console script")

            record_names = [name for name in names if name.endswith(".dist-info/RECORD")]
            if len(record_names) != 1:
                errors.append("wheel must contain exactly one RECORD file")
            else:
                _verify_record(archive, record_names[0], errors)

            sbom_names = [
                name for name in names if name.endswith(f".dist-info/sboms/{RUST_SBOM_FILENAME}")
            ]
            if len(sbom_names) > 1:
                errors.append("wheel contains more than one Rust SBOM")
            elif sbom_names:
                rust_sbom = True
                _verify_rust_sbom(archive.read(sbom_names[0]), errors)
    except (OSError, zipfile.BadZipFile) as error:
        errors.append(f"cannot read wheel: {error}")

    if require_license:
        if license_expression != LICENSE_EXPRESSION:
            errors.append(
                f"wheel License-Expression must be {LICENSE_EXPRESSION}, got {license_expression!r}"
            )
        if not metadata_license_file:
            errors.append("wheel METADATA does not declare License-File: LICENSE")
        if not license_file:
            errors.append("wheel does not install its LICENSE file")
        elif license_file_sha256 != EXPECTED_LICENSE_SHA256:
            errors.append("wheel LICENSE content differs from the repository LICENSE")
    return {
        "path": str(path),
        "status": "passed" if not errors else "failed",
        "license_expression": license_expression,
        "metadata_license_file": metadata_license_file,
        "license_file": license_file,
        "license_file_sha256": license_file_sha256,
        "rust_sbom": rust_sbom,
        "provides_extra": provides_extra,
        "requires_dist": requires_dist,
        "viewer_asset_version": VIEWER_ASSET_VERSION,
        "viewer_asset_license": VIEWER_ASSET_LICENSE,
        "viewer_assets": sorted(viewer_assets),
        "errors": errors,
    }


def _sdist_members(path: Path) -> tuple[list[tuple[str, bool, bool, bool]], str | None]:
    if path.name.endswith(".tar.gz") or path.suffix in {".gz", ".tar"}:
        with tarfile.open(path, "r:*") as archive:
            return [
                (
                    item.name,
                    item.issym() or item.islnk(),
                    not (item.isfile() or item.isdir() or item.issym() or item.islnk()),
                    item.isdir(),
                )
                for item in archive.getmembers()
            ], None
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            return [
                (
                    item.filename,
                    stat.S_ISLNK(item.external_attr >> 16),
                    False,
                    item.is_dir(),
                )
                for item in archive.infolist()
            ], None
    return [], f"unsupported sdist archive: {path.name}"


def _read_sdist_file(path: Path, relative_name: str) -> bytes | None:
    expected_parts = PurePosixPath(relative_name).parts
    if path.name.endswith(".tar.gz") or path.suffix in {".gz", ".tar"}:
        with tarfile.open(path, "r:*") as archive:
            matches = [
                item
                for item in archive.getmembers()
                if item.isfile() and PurePosixPath(item.name).parts[1:] == expected_parts
            ]
            if len(matches) != 1:
                return None
            stream = archive.extractfile(matches[0])
            return None if stream is None else stream.read()
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            matches = [
                item
                for item in archive.infolist()
                if not item.is_dir() and PurePosixPath(item.filename).parts[1:] == expected_parts
            ]
            return archive.read(matches[0]) if len(matches) == 1 else None
    return None


def verify_sdist(path: Path, *, require_license: bool = False) -> dict[str, object]:
    """Inspect one source distribution without extracting it."""

    path = path.resolve()
    errors: list[str] = []
    license_expression: str | None = None
    metadata_license_file = False
    license_file = False
    license_file_sha256: str | None = None
    provides_extra: list[str] = []
    requires_dist: list[str] = []
    viewer_assets: set[str] = set()
    try:
        members, read_error = _sdist_members(path)
    except (OSError, tarfile.TarError, zipfile.BadZipFile) as error:
        members, read_error = [], f"cannot read sdist: {error}"
    if read_error:
        errors.append(read_error)

    file_names: set[str] = set()
    member_names: set[str] = set()
    prefixes: set[str] = set()
    for name, is_link, is_special, is_directory in members:
        if _unsafe_archive_path(name):
            errors.append(f"unsafe sdist path: {name}")
            continue
        if name in member_names:
            errors.append(f"sdist contains a duplicate member path: {name}")
        member_names.add(name)
        parts = PurePosixPath(name).parts
        prefixes.add(parts[0])
        if len(parts) == 1:
            continue
        relative = PurePosixPath(*parts[1:])
        if is_link:
            errors.append(f"sdist contains a link: {relative.as_posix()}")
        if is_special:
            errors.append(f"sdist contains a special file: {relative.as_posix()}")
        if is_directory:
            continue
        file_names.add(relative.as_posix())
        forbidden = _forbidden_path(relative)
        if forbidden:
            errors.append(f"sdist contains {forbidden}: {relative.as_posix()}")
        top = relative.parts[0]
        if top not in SDIST_ROOT_FILES and top not in SDIST_ROOT_DIRECTORIES:
            errors.append(f"unexpected sdist top-level path: {relative.as_posix()}")
        if top == "corpus" and relative.as_posix() not in {
            "corpus/README.md",
            "corpus/manifest.jsonl",
            "corpus/manifest.schema.json",
        }:
            errors.append(f"unexpected public corpus file in sdist: {relative.as_posix()}")
        if (
            top == "fuzz"
            and len(relative.parts) > 1
            and relative.parts[1]
            not in {
                "Cargo.lock",
                "Cargo.toml",
                "fuzz_targets",
            }
        ):
            errors.append(f"unexpected fuzz runtime file in sdist: {relative.as_posix()}")
        if top == "scripts" and relative.as_posix() not in SDIST_SCRIPT_FILES:
            errors.append(f"unexpected maintainer script in sdist: {relative.as_posix()}")
        if relative.name in {"LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"}:
            license_file = True
        viewer_prefix = ("src", "parasolid_kit", "interop", "preview", "static")
        if relative.parts[: len(viewer_prefix)] == viewer_prefix:
            wheel_name = PurePosixPath(*relative.parts[1:]).as_posix()
            payload = _read_sdist_file(path, relative.as_posix())
            if payload is None:
                errors.append(f"cannot read sdist viewer asset: {relative.as_posix()}")
            else:
                viewer_assets.add(wheel_name)
                _verify_viewer_asset(
                    wheel_name,
                    payload,
                    artifact="sdist",
                    errors=errors,
                )

    if len(prefixes) != 1:
        errors.append(f"sdist must have exactly one archive root, got {sorted(prefixes)}")
    required = {
        "Cargo.lock",
        "Cargo.toml",
        "LICENSE",
        "PKG-INFO",
        "README.md",
        "corpus/README.md",
        "corpus/manifest.schema.json",
        "crates/parasolid-core/Cargo.toml",
        "crates/parasolid-python/Cargo.toml",
        "docs/api.md",
        "docs/format-support.md",
        "fuzz/Cargo.lock",
        "fuzz/Cargo.toml",
        "fuzz/fuzz_targets/inspect.rs",
        "fuzz/fuzz_targets/parse.rs",
        "fuzz/fuzz_targets/schema_catalog.rs",
        "pyproject.toml",
        "scripts/verify_optional_install.py",
        "scripts/verify_optional_interop_i0.py",
        "scripts/verify_optional_interop_i3.py",
        "scripts/verify_optional_interop_i4.py",
        "scripts/verify_optional_interop_i5.py",
        "scripts/verify_optional_interop_i6.py",
        "scripts/verify_optional_interop_i7.py",
        "src/parasolid_kit/__init__.py",
        "src/parasolid_kit/interop/__init__.py",
        "src/parasolid_kit/interop/_typing.py",
        "src/parasolid_kit/interop/cadquery.py",
        "src/parasolid_kit/interop/dependency.py",
        "src/parasolid_kit/interop/errors.py",
        "src/parasolid_kit/interop/limits.py",
        "src/parasolid_kit/interop/occt/__init__.py",
        "src/parasolid_kit/interop/occt/conversion.py",
        "src/parasolid_kit/interop/occt/coverage.py",
        "src/parasolid_kit/interop/occt/geometry.py",
        "src/parasolid_kit/interop/occt/model.py",
        "src/parasolid_kit/interop/occt/options.py",
        "src/parasolid_kit/interop/occt/step.py",
        "src/parasolid_kit/interop/occt/topology.py",
        "src/parasolid_kit/interop/occt/validation.py",
        "src/parasolid_kit/interop/preview/__init__.py",
        "src/parasolid_kit/interop/preview/glb.py",
        "src/parasolid_kit/interop/preview/model.py",
        "src/parasolid_kit/interop/preview/server.py",
        "src/parasolid_kit/interop/preview/static/index.html",
        "src/parasolid_kit/interop/preview/static/viewer.css",
        "src/parasolid_kit/interop/preview/static/viewer.js",
        "src/parasolid_kit/interop/preview/tessellation.py",
        "src/parasolid_kit/interop/preview/writer.py",
        "tests/_occt_fixtures.py",
        "tests/test_cadquery_adapter.py",
        "tests/test_geometry_coverage.py",
        "tests/test_occt_conversion.py",
        "tests/test_preview.py",
        "tests/test_step_export.py",
    }
    for missing in sorted(required - file_names):
        errors.append(f"required sdist file is missing: {missing}")
    for missing in sorted(set(VIEWER_ASSET_SHA256) - viewer_assets):
        errors.append(f"required sdist viewer asset is missing: src/{missing}")

    try:
        pkg_info = _read_sdist_file(path, "PKG-INFO")
        if pkg_info is not None:
            metadata = BytesParser().parsebytes(pkg_info)
            license_expression = _license_expression(metadata)
            metadata_license_file = "LICENSE" in metadata.get_all("License-File", [])
            provides_extra, requires_dist = _verify_dependency_metadata(
                metadata,
                artifact="sdist",
                errors=errors,
            )
        license_bytes = _read_sdist_file(path, "LICENSE")
        if license_bytes is not None:
            license_file_sha256 = hashlib.sha256(license_bytes).hexdigest()
    except (OSError, tarfile.TarError, zipfile.BadZipFile) as error:
        errors.append(f"cannot read sdist PKG-INFO: {error}")

    if require_license:
        if license_expression != LICENSE_EXPRESSION:
            errors.append(
                f"sdist License-Expression must be {LICENSE_EXPRESSION}, got {license_expression!r}"
            )
        if not metadata_license_file:
            errors.append("sdist PKG-INFO does not declare License-File: LICENSE")
        if not license_file:
            errors.append("sdist is missing its LICENSE file")
        elif license_file_sha256 != EXPECTED_LICENSE_SHA256:
            errors.append("sdist LICENSE content differs from the repository LICENSE")
    return {
        "path": str(path),
        "status": "passed" if not errors else "failed",
        "license_expression": license_expression,
        "metadata_license_file": metadata_license_file,
        "license_file": license_file,
        "license_file_sha256": license_file_sha256,
        "provides_extra": provides_extra,
        "requires_dist": requires_dist,
        "viewer_asset_version": VIEWER_ASSET_VERSION,
        "viewer_asset_license": VIEWER_ASSET_LICENSE,
        "viewer_assets": sorted(viewer_assets),
        "errors": errors,
    }


def main() -> int:
    """Verify both distribution artifacts."""

    arguments = _arguments()
    wheel = verify_wheel(arguments.wheel, require_license=arguments.require_license)
    sdist = verify_sdist(arguments.sdist, require_license=arguments.require_license)
    report = {
        "status": "passed"
        if wheel["status"] == "passed" and sdist["status"] == "passed"
        else "failed",
        "wheel": wheel,
        "sdist": sdist,
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
