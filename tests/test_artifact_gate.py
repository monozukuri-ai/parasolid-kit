from __future__ import annotations

import base64
import csv
import hashlib
import io
import json
import tarfile
import zipfile
from pathlib import Path

from scripts.verify_artifacts import EXPECTED_LICENSE_BYTES, verify_sdist, verify_wheel

DIST_INFO = "parasolid_kit-0.1.0.dev0.dist-info"
RUST_SBOM = f"{DIST_INFO}/sboms/parasolid-python.cyclonedx.json"


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
        f"{DIST_INFO}/METADATA": (
            b"Metadata-Version: 2.4\n"
            b"Name: parasolid-kit\n"
            b"Version: 0.1.0.dev0\n"
            b"Requires-Python: >=3.10\n"
            b"License-Expression: MIT\n"
            b"License-File: LICENSE\n\n"
        ),
        f"{DIST_INFO}/WHEEL": b"Wheel-Version: 1.0\nRoot-Is-Purelib: false\n",
        f"{DIST_INFO}/entry_points.txt": (
            b"[console_scripts]\nparasolid-kit = parasolid_kit.cli:main\n"
        ),
        f"{DIST_INFO}/licenses/LICENSE": EXPECTED_LICENSE_BYTES,
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
        "src/parasolid_kit/__init__.py",
    }
    files = {name: b"placeholder" for name in required}
    files["LICENSE"] = EXPECTED_LICENSE_BYTES
    files["PKG-INFO"] = (
        b"Metadata-Version: 2.4\n"
        b"Name: parasolid-kit\n"
        b"Version: 0.1.0.dev0\n"
        b"License-Expression: MIT\n"
        b"License-File: LICENSE\n\n"
    )
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


def test_wheel_license_gate_rejects_a_different_expression(tmp_path: Path) -> None:
    wheel = tmp_path / "package.whl"
    _wheel(
        wheel,
        extra={
            f"{DIST_INFO}/METADATA": (
                b"Metadata-Version: 2.4\n"
                b"Name: parasolid-kit\n"
                b"Version: 0.1.0.dev0\n"
                b"Requires-Python: >=3.10\n"
                b"License-Expression: Apache-2.0\n"
                b"License-File: LICENSE\n\n"
            )
        },
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
