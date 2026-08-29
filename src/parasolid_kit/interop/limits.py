"""Independent resource ceilings for conversion, export, and preview work."""

from __future__ import annotations

from dataclasses import dataclass, fields


@dataclass(frozen=True, slots=True)
class InteropLimits:
    """Bound work that can amplify one parsed B-Rep into kernel or mesh objects."""

    max_entities: int = 1_000_000
    max_occt_subshapes: int = 2_000_000
    max_curve_samples: int = 1_000_000
    max_triangles: int = 5_000_000
    max_vertices: int = 10_000_000
    max_output_bytes: int = 512 * 1024 * 1024
    max_diagnostics: int = 10_000

    def __post_init__(self) -> None:
        for item in fields(self):
            value = getattr(self, item.name)
            if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
                raise ValueError(f"{item.name} must be a positive integer")

    def limit_for(self, resource: str) -> int:
        """Return one named ceiling without conflating it with parse limits."""

        if not isinstance(resource, str):
            raise TypeError("resource must be a string")
        names = {item.name for item in fields(self)}
        if resource not in names:
            raise ValueError(f"unknown interop limit: {resource}")
        return getattr(self, resource)

    def to_dict(self) -> dict[str, int]:
        """Return deterministic JSON-compatible limit values."""

        return {item.name: getattr(self, item.name) for item in fields(self)}


DEFAULT_INTEROP_LIMITS = InteropLimits()
