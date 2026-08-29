from __future__ import annotations

import base64
import csv
import hashlib
import io
import json
import tarfile
import zipfile
from pathlib import Path

from scripts.verify_artifacts import (
    EXPECTED_LICENSE_BYTES,
    VIEWER_ASSET_LICENSE,
    VIEWER_ASSET_SHA256,
    VIEWER_ASSET_VERSION,
    verify_sdist,
    verify_wheel,
)

DIST_INFO = "parasolid_kit-0.1.0.dev0.dist-info"
RUST_SBOM = f"{DIST_INFO}/sboms/parasolid-python.cyclonedx.json"
ROOT = Path(__file__).resolve().parents[1]
VIEWER_ASSETS = {name: (ROOT / "src" / name).read_bytes() for name in VIEWER_ASSET_SHA256}


def _metadata(
    *,
    license_expression: str = "MIT",
    occt_requirement: str = ("cadquery-ocp-novtk<7.10,>=7.9.3.1; extra == 'occt'"),
    cadquery_requirement: str = "cadquery<2.9,>=2.8; extra == 'cadquery'",
    additional_headers: tuple[str, ...] = (),
) -> bytes:
    lines = [
        "Metadata-Version: 2.4",
        "Name: parasolid-kit",
        "Version: 0.1.0.dev0",
        "Requires-Python: >=3.10",
        f"License-Expression: {license_expression}",
        "License-File: LICENSE",
        "Provides-Extra: cadquery",
        "Provides-Extra: occt",
        f"Requires-Dist: {cadquery_requirement}",
        f"Requires-Dist: {occt_requirement}",
        *additional_headers,
        "",
        "",
    ]
    return "\n".join(lines).encode()


def _record(files: dict[str, bytes]) -> bytes:
    output = io.StringIO()
    writer = csv.writer(output, lineterminator="\n")
    for name, payload in files.items():
        digest = base64.urlsafe_b64encode(hashlib.sha256(payload).digest()).rstrip(b"=")
        writer.writerow((name, f"sha256={digest.decode('ascii')}", len(payload)))
    writer.writerow((f"{DIST_INFO}/RECORD", "", ""))
    return output.getvalue().encode("utf-8")


def _wheel(path: Path, *, extra: dict[str, bytes] | None = None) -> None:
    files = {
        "parasolid_kit/__init__.py": b'__version__ = "0.1.0.dev0"\n',
        "parasolid_kit/_core.abi3.so": b"native-placeholder",
        f"{DIST_INFO}/METADATA": _metadata(),
        f"{DIST_INFO}/WHEEL": b"Wheel-Version: 1.0\nRoot-Is-Purelib: false\n",
        f"{DIST_INFO}/entry_points.txt": (
            b"[console_scripts]\nparasolid-kit = parasolid_kit.cli:main\n"
        ),
        f"{DIST_INFO}/licenses/LICENSE": EXPECTED_LICENSE_BYTES,
        **VIEWER_ASSETS,
    }
    files.update(extra or {})
    files[f"{DIST_INFO}/RECORD"] = _record(files)
    with zipfile.ZipFile(path, "w") as archive:
        for name, payload in files.items():
            archive.writestr(name, payload)


def _sdist(path: Path, *, extra: dict[str, bytes] | None = None) -> None:
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
    files = {name: b"placeholder" for name in required}
    files["LICENSE"] = EXPECTED_LICENSE_BYTES
    files["PKG-INFO"] = _metadata()
    files.update({f"src/{name}": payload for name, payload in VIEWER_ASSETS.items()})
    files.update(extra or {})
    with tarfile.open(path, "w:gz") as archive:
        for relative, payload in files.items():
            name = f"parasolid_kit-0.1.0.dev0/{relative}"
            info = tarfile.TarInfo(name)
            info.size = len(payload)
            archive.addfile(info, io.BytesIO(payload))


def test_wheel_gate_accepts_expected_files_and_license(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(wheel)

    report = verify_wheel(wheel, require_license=True)

    assert report["status"] == "passed"
    assert report["license_expression"] == "MIT"
    assert report["metadata_license_file"] is True
    assert report["license_file"] is True
    assert report["provides_extra"] == ["cadquery", "occt"]
    assert len(report["requires_dist"]) == 2
    assert report["viewer_asset_version"] == VIEWER_ASSET_VERSION
    assert report["viewer_asset_license"] == VIEWER_ASSET_LICENSE
    assert report["viewer_assets"] == sorted(VIEWER_ASSET_SHA256)


def test_wheel_gate_rejects_native_cad_data(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(wheel, extra={"parasolid_kit/private-model.x_t": b"not distributable"})

    report = verify_wheel(wheel)

    assert report["status"] == "failed"
    assert any("native CAD fixture" in error for error in report["errors"])


def test_wheel_gate_accepts_maturin_rust_sbom(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "component": {
                "name": "parasolid-python",
                "version": "0.1.0-dev0",
                "licenses": [{"expression": "MIT"}],
            }
        },
        "components": [],
        "dependencies": [],
    }
    _wheel(wheel, extra={RUST_SBOM: json.dumps(sbom).encode("utf-8")})

    report = verify_wheel(wheel, require_license=True)

    assert report["status"] == "passed"
    assert report["rust_sbom"] is True


def test_wheel_gate_rejects_malformed_rust_sbom(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(wheel, extra={RUST_SBOM: b"not-json"})

    report = verify_wheel(wheel)

    assert report["status"] == "failed"
    assert any("SBOM is not valid UTF-8 JSON" in error for error in report["errors"])


def test_sdist_gate_accepts_the_declared_source_layout(tmp_path: Path) -> None:
    sdist = tmp_path / "package.tar.gz"
    _sdist(sdist)

    report = verify_sdist(sdist, require_license=True)

    assert report["status"] == "passed"
    assert report["license_expression"] == "MIT"
    assert report["metadata_license_file"] is True
    assert report["license_file"] is True
    assert report["provides_extra"] == ["cadquery", "occt"]
    assert len(report["requires_dist"]) == 2
    assert report["viewer_assets"] == sorted(VIEWER_ASSET_SHA256)


def test_wheel_gate_rejects_a_modified_viewer_asset(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(
        wheel,
        extra={"parasolid_kit/interop/preview/static/viewer.js": b"modified"},
    )

    report = verify_wheel(wheel)

    assert report["status"] == "failed"
    assert any("viewer asset SHA-256 mismatch" in error for error in report["errors"])


def test_sdist_gate_rejects_an_unapproved_viewer_asset(tmp_path: Path) -> None:
    sdist = tmp_path / "package.tar.gz"
    _sdist(
        sdist,
        extra={"src/parasolid_kit/interop/preview/static/logo.png": b"unapproved"},
    )

    report = verify_sdist(sdist)

    assert report["status"] == "failed"
    assert any("unapproved viewer asset" in error for error in report["errors"])


def test_sdist_gate_rejects_an_unapproved_maintainer_script(tmp_path: Path) -> None:
    sdist = tmp_path / "package.tar.gz"
    _sdist(sdist, extra={"scripts/private_validation.py": b"not distributable"})

    report = verify_sdist(sdist)

    assert report["status"] == "failed"
    assert any("unexpected maintainer script" in error for error in report["errors"])


def test_wheel_license_gate_rejects_a_different_expression(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(
        wheel,
        extra={f"{DIST_INFO}/METADATA": _metadata(license_expression="Apache-2.0")},
    )

    report = verify_wheel(wheel, require_license=True)

    assert report["status"] == "failed"
    assert any("License-Expression must be MIT" in error for error in report["errors"])


def test_wheel_license_gate_rejects_different_license_text(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(wheel, extra={f"{DIST_INFO}/licenses/LICENSE": b"different text"})

    report = verify_wheel(wheel, require_license=True)

    assert report["status"] == "failed"
    assert any("LICENSE content differs" in error for error in report["errors"])


def test_wheel_gate_rejects_an_unconditional_optional_runtime(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(
        wheel,
        extra={
            f"{DIST_INFO}/METADATA": _metadata(
                additional_headers=("Requires-Dist: cadquery-ocp-novtk>=7.9.3.1,<7.10",)
            )
        },
    )

    report = verify_wheel(wheel)

    assert report["status"] == "failed"
    assert any("unconditional Requires-Dist" in error for error in report["errors"])


def test_wheel_gate_rejects_unknown_or_duplicate_extras(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(
        wheel,
        extra={
            f"{DIST_INFO}/METADATA": _metadata(
                additional_headers=(
                    "Provides-Extra: viewer",
                    "Provides-Extra: occt",
                )
            )
        },
    )

    report = verify_wheel(wheel)

    assert report["status"] == "failed"
    assert any("duplicate Provides-Extra" in error for error in report["errors"])
    assert any("Provides-Extra must be exactly" in error for error in report["errors"])


def test_wheel_gate_rejects_a_broader_extra_version_range(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(
        wheel,
        extra={
            f"{DIST_INFO}/METADATA": _metadata(
                occt_requirement="cadquery-ocp-novtk>=7.9; extra == 'occt'"
            )
        },
    )

    report = verify_wheel(wheel)

    assert report["status"] == "failed"
    assert any("unapproved version range" in error for error in report["errors"])


def test_sdist_gate_rejects_a_non_exact_extra_marker(tmp_path: Path) -> None:
    sdist = tmp_path / "package.tar.gz"
    _sdist(
        sdist,
        extra={
            "PKG-INFO": _metadata(
                cadquery_requirement=(
                    "cadquery<2.9,>=2.8; extra == 'cadquery' and python_version >= '3.11'"
                )
            )
        },
    )

    report = verify_sdist(sdist)

    assert report["status"] == "failed"
    assert any("non-exact extra marker" in error for error in report["errors"])


def test_sdist_gate_rejects_a_bundled_siemens_catalog(tmp_path: Path) -> None:
    sdist = tmp_path / "package.tar.gz"
    _sdist(sdist, extra={"corpus/sch_30000.sch_txt": b"private catalog"})

    report = verify_sdist(sdist)

    assert report["status"] == "failed"
    assert any("Siemens schema catalog" in error for error in report["errors"])


def test_sdist_gate_rejects_internal_design_notes(tmp_path: Path) -> None:
    sdist = tmp_path / "package.tar.gz"
    _sdist(sdist, extra={".internal/README.md": b"maintainer-only notes"})

    report = verify_sdist(sdist)

    assert report["status"] == "failed"
    assert any("unexpected sdist top-level path" in error for error in report["errors"])


def test_sdist_gate_rejects_path_traversal(tmp_path: Path) -> None:
    sdist = tmp_path / "package.tar.gz"
    _sdist(sdist, extra={"../escape": b"unsafe"})

    report = verify_sdist(sdist)

    assert report["status"] == "failed"
    assert any("unsafe sdist path" in error for error in report["errors"])
