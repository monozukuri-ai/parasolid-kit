"""Structured failures for optional geometry-kernel interoperability."""

from __future__ import annotations

from typing import TYPE_CHECKING

from ..diagnostics import Diagnostic
from ..errors import ParasolidError
from ._typing import INTEROP_DIAGNOSTIC_PREFIXES

if TYPE_CHECKING:
    from .occt.model import ConversionReport, OcctConversionResult
    from .occt.step import StepExportReport
    from .preview.model import PreviewReport


class InteropError(ParasolidError):
    """Base failure carrying one structured interop diagnostic."""

    def __init__(self, diagnostic: Diagnostic) -> None:
        if not isinstance(diagnostic, Diagnostic):
            raise TypeError("diagnostic must be a Diagnostic")
        if not diagnostic.code.startswith(INTEROP_DIAGNOSTIC_PREFIXES):
            raise ValueError("interop diagnostic code must use an interop-owned prefix")
        self.diagnostic = diagnostic
        super().__init__(self._format_message())

    def _format_message(self) -> str:
        location = ""
        if self.diagnostic.location is not None:
            coordinates = ", ".join(
                f"{name}={value}" for name, value in self.diagnostic.location.to_dict().items()
            )
            location = f" ({coordinates})"
        return f"{self.diagnostic.code}{location}: {self.diagnostic.message}"


class InteropDependencyError(InteropError):
    """An optional runtime is missing, broken, or conflicts with another profile."""


class OcctConversionError(InteropError):
    """A B-Rep could not be converted to an OCCT shape without hidden loss."""

    def __init__(
        self,
        diagnostic: Diagnostic,
        *,
        report: ConversionReport | None = None,
    ) -> None:
        self.report = report
        super().__init__(diagnostic)


class CadQueryConversionError(InteropError):
    """An OCCT conversion could not be exposed through the CadQuery shape API."""

    def __init__(
        self,
        diagnostic: Diagnostic,
        *,
        result: OcctConversionResult | None = None,
    ) -> None:
        self.result = result
        super().__init__(diagnostic)


class StepExportError(InteropError):
    """An OCCT result could not be written or independently revalidated as STEP."""

    def __init__(
        self,
        diagnostic: Diagnostic,
        *,
        report: StepExportReport | None = None,
    ) -> None:
        self.report = report
        super().__init__(diagnostic)


class PreviewError(InteropError):
    """A bounded local preview could not be generated or served safely."""

    def __init__(
        self,
        diagnostic: Diagnostic,
        *,
        report: PreviewReport | None = None,
    ) -> None:
        self.report = report
        super().__init__(diagnostic)
