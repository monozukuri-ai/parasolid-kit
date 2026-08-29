"""Self-contained OCCT preview output and local-only serving helpers."""

from .glb import GlbBuilder, validate_glb_bytes
from .model import (
    GlbValidationReport,
    PreviewArtifact,
    PreviewOptions,
    PreviewReport,
    PreviewResult,
)
from .server import PreviewServer, create_preview_server
from .tessellation import TessellationResult, tessellate_preview
from .writer import (
    ASSET_BUNDLE_VERSION,
    ASSET_LICENSE,
    STATIC_ASSET_NAMES,
    STATIC_ASSET_SHA256,
    write_preview,
)

__all__ = [
    "ASSET_BUNDLE_VERSION",
    "ASSET_LICENSE",
    "STATIC_ASSET_NAMES",
    "STATIC_ASSET_SHA256",
    "GlbBuilder",
    "GlbValidationReport",
    "PreviewArtifact",
    "PreviewOptions",
    "PreviewReport",
    "PreviewResult",
    "PreviewServer",
    "TessellationResult",
    "create_preview_server",
    "tessellate_preview",
    "validate_glb_bytes",
    "write_preview",
]
