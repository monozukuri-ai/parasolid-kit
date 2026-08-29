"""Atomic writer for a self-contained, bounded local preview directory."""

from __future__ import annotations

import hashlib
import json
import shutil
import tempfile
from importlib import resources
from pathlib import Path
from typing import Final

from ...brep.model import BrepModel
from ...diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity
from ..errors import PreviewError
from ..limits import DEFAULT_INTEROP_LIMITS, InteropLimits
from ..occt.model import OcctConversionResult
from .glb import validate_glb_bytes
from .model import PreviewArtifact, PreviewOptions, PreviewReport, PreviewResult
from .tessellation import tessellate_preview

ASSET_BUNDLE_VERSION: Final = "1.0.0"
ASSET_LICENSE: Final = "MIT"
STATIC_ASSET_NAMES: Final = ("index.html", "viewer.css", "viewer.js")

# Updated deliberately whenever the reviewed, package-owned UI changes. Keeping
# this allowlist here makes unexpected wheel resources fail before being served.
STATIC_ASSET_SHA256: Final[dict[str, str]] = {
    "index.html": "0a80d9176e27433c009cebba64671d75f1bc405b979d2ed43bea248b6e184ca4",
    "viewer.css": "c6b6f8778e83ef6c16f6a42d03b864f05f3ee99fe10b272d046717e9290de296",
    "viewer.js": "81094c0fbb08865157d6091fd60be9f85ffbc3cdcc60153325ee27a313351d6e",
}


def write_preview(
    converted: OcctConversionResult,
    brep: BrepModel,
    destination: str | Path,
    *,
    options: PreviewOptions | None = None,
    limits: InteropLimits = DEFAULT_INTEROP_LIMITS,
    overwrite: bool = False,
) -> PreviewResult:
    """Write GLB, manifest, and bundled UI without exposing source bytes or paths."""

    if not isinstance(destination, (str, Path)):
        raise TypeError("destination must be a path")
    if not isinstance(overwrite, bool):
        raise TypeError("overwrite must be a boolean")
    output = Path(destination)
    if output.name in {"", ".", ".."}:
        raise ValueError("destination must name one preview directory")
    if output.is_symlink():
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.unsafe_destination",
                kind=DiagnosticKind.INVALID,
                message="preview destination must not be a symbolic link",
            )
        )
    if output.exists() and not overwrite:
        raise FileExistsError(f"preview destination already exists: {output}")
    if output.exists() and not output.is_dir():
        raise NotADirectoryError(f"preview destination is not a directory: {output}")
    if not isinstance(limits, InteropLimits):
        raise TypeError("limits must be InteropLimits")

    resolved_options = PreviewOptions() if options is None else options
    tessellated = tessellate_preview(
        converted,
        brep,
        options=resolved_options,
        limits=limits,
    )
    validation = validate_glb_bytes(tessellated.glb)
    if not validation.valid:
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.invalid_glb",
                kind=DiagnosticKind.INTERNAL,
                message="generated preview GLB did not pass structural validation",
                details={"error_count": len(validation.errors)},
            )
        )

    static_assets = _static_assets(brep)
    manifest = dict(tessellated.manifest)
    manifest["asset_bundle"] = {
        "version": ASSET_BUNDLE_VERSION,
        "license": ASSET_LICENSE,
        "assets": [
            _artifact(name, payload).to_dict() for name, payload in sorted(static_assets.items())
        ],
    }
    manifest["glb"] = _artifact("preview.glb", tessellated.glb).to_dict()
    manifest_bytes = (
        json.dumps(manifest, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("ascii")
    payloads = {
        **static_assets,
        "preview.glb": tessellated.glb,
        "preview.manifest.json": manifest_bytes,
    }
    total_bytes = sum(len(payload) for payload in payloads.values())
    if total_bytes > limits.max_output_bytes:
        raise PreviewError(
            _diagnostic(
                brep,
                code="preview.limit_exceeded",
                kind=DiagnosticKind.LIMIT,
                message="complete preview output exceeds the configured byte limit",
                details={
                    "resource": "max_output_bytes",
                    "observed": total_bytes,
                    "limit": limits.max_output_bytes,
                },
            )
        )

    artifacts = tuple(_artifact(name, payloads[name]) for name in sorted(payloads))
    partial = not brep.complete or bool(
        tessellated.missing_face_count or tessellated.missing_edge_count
    )
    report = PreviewReport(
        schema_version=1,
        status="partial" if partial else "complete",
        source_complete=brep.complete,
        conversion_complete=converted.report.conversion_complete,
        occt_valid=converted.report.occt_valid,
        partial=partial,
        options=resolved_options,
        target_unit=converted.report.options.target_unit,
        face_primitive_count=tessellated.face_primitive_count,
        edge_primitive_count=tessellated.edge_primitive_count,
        triangle_count=tessellated.triangle_count,
        vertex_count=tessellated.vertex_count,
        curve_sample_count=tessellated.curve_sample_count,
        missing_face_count=tessellated.missing_face_count,
        missing_edge_count=tessellated.missing_edge_count,
        output_bytes=total_bytes,
        glb_validation=validation,
        artifacts=artifacts,
        asset_bundle_version=ASSET_BUNDLE_VERSION,
        asset_license=ASSET_LICENSE,
        limits=limits,
    )
    _write_directory(output, payloads, overwrite=overwrite)
    return PreviewResult(
        directory=output,
        index_path=output / "index.html",
        glb_path=output / "preview.glb",
        manifest_path=output / "preview.manifest.json",
        report=report,
    )


def _static_assets(brep: BrepModel) -> dict[str, bytes]:
    directory = resources.files("parasolid_kit.interop.preview").joinpath("static")
    result: dict[str, bytes] = {}
    for name in STATIC_ASSET_NAMES:
        candidate = directory.joinpath(name)
        if not candidate.is_file():
            raise PreviewError(
                _diagnostic(
                    brep,
                    code="preview.asset_missing",
                    kind=DiagnosticKind.INTERNAL,
                    message=f"bundled preview asset is missing: {name}",
                    details={"asset": name},
                )
            )
        payload = candidate.read_bytes()
        digest = hashlib.sha256(payload).hexdigest()
        if digest != STATIC_ASSET_SHA256[name]:
            raise PreviewError(
                _diagnostic(
                    brep,
                    code="preview.asset_mismatch",
                    kind=DiagnosticKind.INTERNAL,
                    message=f"bundled preview asset hash differs from the reviewed bundle: {name}",
                    details={"asset": name, "sha256": digest},
                )
            )
        result[name] = payload
    return result


def _write_directory(output: Path, payloads: dict[str, bytes], *, overwrite: bool) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=f".{output.name}.staging-", dir=output.parent))
    backup: Path | None = None
    try:
        for name, payload in sorted(payloads.items()):
            (staging / name).write_bytes(payload)
        if output.exists():
            if not overwrite:
                raise FileExistsError(f"preview destination already exists: {output}")
            backup = Path(tempfile.mkdtemp(prefix=f".{output.name}.backup-", dir=output.parent))
            backup.rmdir()
            output.replace(backup)
        staging.replace(output)
    except BaseException:
        if backup is not None and backup.exists() and not output.exists():
            backup.replace(output)
        raise
    finally:
        if staging.exists():
            shutil.rmtree(staging)
        if backup is not None and backup.exists():
            shutil.rmtree(backup)


def _artifact(filename: str, payload: bytes) -> PreviewArtifact:
    return PreviewArtifact(
        filename=filename,
        byte_size=len(payload),
        sha256=hashlib.sha256(payload).hexdigest(),
    )


def _diagnostic(
    brep: BrepModel,
    *,
    code: str,
    kind: DiagnosticKind,
    message: str,
    details: dict[str, str | int | float | bool | None] | None = None,
) -> Diagnostic:
    return Diagnostic(
        code=code,
        severity=DiagnosticSeverity.ERROR,
        kind=kind,
        message=message,
        schema_key=brep.schema_key,
        fatal=True,
        details={} if details is None else details,
    )


__all__ = [
    "ASSET_BUNDLE_VERSION",
    "ASSET_LICENSE",
    "STATIC_ASSET_NAMES",
    "STATIC_ASSET_SHA256",
    "write_preview",
]
