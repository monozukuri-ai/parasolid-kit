"""Optional interoperability contracts with dependency-free imports."""

from .dependency import (
    InteropProfile,
    installed_interop_distributions,
    require_cadquery,
    require_occt,
)
from .errors import (
    CadQueryConversionError,
    InteropDependencyError,
    InteropError,
    OcctConversionError,
    PreviewError,
    StepExportError,
)
from .limits import DEFAULT_INTEROP_LIMITS, InteropLimits

__all__ = [
    "DEFAULT_INTEROP_LIMITS",
    "CadQueryConversionError",
    "InteropDependencyError",
    "InteropError",
    "InteropLimits",
    "InteropProfile",
    "OcctConversionError",
    "PreviewError",
    "StepExportError",
    "installed_interop_distributions",
    "require_cadquery",
    "require_occt",
]
