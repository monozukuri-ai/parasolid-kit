"""Validated unit and tolerance options for OCCT conversion."""

from __future__ import annotations

from dataclasses import dataclass, field
from math import isfinite
from typing import Final, Literal, TypeAlias, cast

LengthUnit: TypeAlias = Literal["m", "cm", "mm", "in", "ft"]

UNIT_TO_METRES: Final[dict[LengthUnit, float]] = {
    "m": 1.0,
    "cm": 0.01,
    "mm": 0.001,
    "in": 0.0254,
    "ft": 0.3048,
}


@dataclass(frozen=True, slots=True)
class ValidationTolerances:
    """Target-unit tolerances used to compare source and OCCT metrics."""

    linear_absolute: float = 1.0e-6
    relative: float = 1.0e-9

    def __post_init__(self) -> None:
        for name in ("linear_absolute", "relative"):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                raise ValueError(f"{name} must be a finite non-negative number")
            normalized = float(value)
            if not isfinite(normalized) or normalized < 0.0:
                raise ValueError(f"{name} must be a finite non-negative number")
            object.__setattr__(self, name, normalized)
        if self.linear_absolute == 0.0 and self.relative == 0.0:
            raise ValueError("at least one validation tolerance must be positive")

    def linear_threshold(self, reference: float) -> float:
        return max(self.linear_absolute, self.relative * abs(reference))

    def area_threshold(self, reference: float) -> float:
        return max(self.linear_absolute**2, self.relative * abs(reference))

    def volume_threshold(self, reference: float) -> float:
        return max(self.linear_absolute**3, self.relative * abs(reference))

    def to_dict(self) -> dict[str, float]:
        return {
            "linear_absolute": self.linear_absolute,
            "relative": self.relative,
        }


@dataclass(frozen=True, slots=True)
class OcctConversionOptions:
    """Immutable conversion choices; source units are never inferred."""

    source_unit: LengthUnit
    target_unit: LengthUnit = "mm"
    require_complete: bool = True
    heal: bool = False
    validation: ValidationTolerances = field(default_factory=ValidationTolerances)
    source_identity: str | None = None

    def __post_init__(self) -> None:
        for name in ("source_unit", "target_unit"):
            value = getattr(self, name)
            if value not in UNIT_TO_METRES:
                supported = ", ".join(UNIT_TO_METRES)
                raise ValueError(f"{name} must be one of: {supported}")
            object.__setattr__(self, name, cast(LengthUnit, value))
        for name in ("require_complete", "heal"):
            if not isinstance(getattr(self, name), bool):
                raise TypeError(f"{name} must be a boolean")
        if not isinstance(self.validation, ValidationTolerances):
            raise TypeError("validation must be ValidationTolerances")
        if self.source_identity is not None and (
            not isinstance(self.source_identity, str) or not self.source_identity.strip()
        ):
            raise ValueError("source_identity must be a non-empty string or None")

    @property
    def source_to_metres(self) -> float:
        return UNIT_TO_METRES[self.source_unit]

    @property
    def target_to_metres(self) -> float:
        return UNIT_TO_METRES[self.target_unit]

    @property
    def applied_scale(self) -> float:
        return self.source_to_metres / self.target_to_metres

    def to_dict(self) -> dict[str, object]:
        return {
            "source_unit": self.source_unit,
            "target_unit": self.target_unit,
            "source_to_metres": self.source_to_metres,
            "target_to_metres": self.target_to_metres,
            "applied_scale": self.applied_scale,
            "require_complete": self.require_complete,
            "heal": self.heal,
            "validation": self.validation.to_dict(),
            "source_identity": self.source_identity,
        }
