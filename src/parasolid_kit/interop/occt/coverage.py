"""Machine-readable I7 parser, OCCT, and STEP geometry coverage contract."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal, TypeAlias

from ...brep.geometry import CurveKind, SurfaceKind

GeometryCategory: TypeAlias = Literal["curve", "surface"]
CoverageStatus: TypeAlias = Literal["exact", "conditional", "unsupported"]


@dataclass(frozen=True, slots=True)
class GeometryCoverage:
    """One source geometry kind and its independently stated pipeline gates."""

    category: GeometryCategory
    kind: str
    parser: CoverageStatus
    occt: CoverageStatus
    step: CoverageStatus
    constraints: str

    def to_dict(self) -> dict[str, str]:
        return {
            "category": self.category,
            "kind": self.kind,
            "parser": self.parser,
            "occt": self.occt,
            "step": self.step,
            "constraints": self.constraints,
        }


GEOMETRY_COVERAGE: tuple[GeometryCoverage, ...] = (
    GeometryCoverage("curve", CurveKind.LINE.value, "exact", "exact", "exact", "two vertices"),
    GeometryCoverage(
        "curve",
        CurveKind.CIRCLE.value,
        "exact",
        "exact",
        "exact",
        "vertex-free full period; use trimmed for an arc",
    ),
    GeometryCoverage(
        "curve",
        CurveKind.ELLIPSE.value,
        "exact",
        "exact",
        "exact",
        "vertex-free full period; major radius >= minor radius",
    ),
    GeometryCoverage(
        "curve",
        CurveKind.PARABOLA.value,
        "exact",
        "exact",
        "exact",
        "two vertices on one exact branch",
    ),
    GeometryCoverage(
        "curve",
        CurveKind.HYPERBOLA.value,
        "exact",
        "exact",
        "exact",
        "two vertices on one exact branch",
    ),
    GeometryCoverage(
        "curve",
        CurveKind.TRIMMED.value,
        "exact",
        "exact",
        "exact",
        "explicit basis, parameters, endpoint positions, and two vertices",
    ),
    GeometryCoverage(
        "curve",
        CurveKind.NURBS.value,
        "exact",
        "conditional",
        "conditional",
        "nonrational or rational 3D curves, open or in the stored periodic form, "
        "or 2D curves used by SP_CURVE",
    ),
    GeometryCoverage(
        "curve",
        CurveKind.SURFACE_PARAMETRIC.value,
        "exact",
        "conditional",
        "conditional",
        "nonrational or rational 2D NURBS on a supported surface, as FIN or edge curve; "
        "bounded 3D approximation and source FIN checks",
    ),
    GeometryCoverage(
        "curve",
        CurveKind.INTERSECTION.value,
        "exact",
        "conditional",
        "conditional",
        "source-identified CHART/LIMIT branch on two supports, open or closed; "
        "bounded numerical fitting",
    ),
    GeometryCoverage(
        "curve",
        CurveKind.UNSUPPORTED.value,
        "unsupported",
        "unsupported",
        "unsupported",
        "unknown source semantics are retained without inference",
    ),
    GeometryCoverage(
        "surface", SurfaceKind.PLANE.value, "exact", "exact", "exact", "explicit trim loops"
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.CYLINDER.value,
        "exact",
        "exact",
        "exact",
        "two vertex-free circular boundary loops",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.CONE.value,
        "exact",
        "exact",
        "exact",
        "frustum with two positive-radius circular boundary loops",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.SPHERE.value,
        "exact",
        "exact",
        "exact",
        "untrimmed closed face, or explicitly trimmed face in parametric topology; "
        "OCCT seam and pole topology is generated",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.TORUS.value,
        "exact",
        "exact",
        "exact",
        "closed ring torus, or explicitly trimmed ring, horn, apple or lemon torus "
        "in parametric topology",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.SPUN.value,
        "exact",
        "conditional",
        "conditional",
        "revolution of a mapped profile curve about the spin axis; explicitly "
        "trimmed faces in parametric topology",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.NURBS.value,
        "exact",
        "conditional",
        "conditional",
        "nonrational or rational 3D row-major grid, open or in the stored periodic form; "
        "multiple loops in parametric topology",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.OFFSET.value,
        "exact",
        "conditional",
        "conditional",
        "supported exact basis surface; I7 verifies a non-periodic NURBS basis",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.BLENDED_EDGE.value,
        "exact",
        "conditional",
        "conditional",
        "rolling-ball blends of equal offsets as the B-spline pipe of the blend radius "
        "around the spine, within the validation tolerance; cliff-edge blends unsupported",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.BLEND_BOUNDARY.value,
        "exact",
        "conditional",
        "conditional",
        "construction surface only: its intersection with a support is the blend's "
        "contact curve, interpolated from the spine within the validation tolerance",
    ),
    GeometryCoverage(
        "surface",
        SurfaceKind.UNSUPPORTED.value,
        "unsupported",
        "unsupported",
        "unsupported",
        "unknown source semantics are retained without inference",
    ),
)


def geometry_coverage() -> tuple[GeometryCoverage, ...]:
    """Return the immutable I7 coverage rows without importing OCCT."""

    return GEOMETRY_COVERAGE


def render_geometry_coverage_markdown() -> str:
    """Render the canonical table embedded verbatim in ``format-support.md``."""

    lines = [
        "| Category | Geometry kind | Parser | OCCT | STEP | Exact constraints |",
        "|---|---|---:|---:|---:|---|",
    ]
    lines.extend(
        "| {category} | `{kind}` | {parser} | {occt} | {step} | {constraints} |".format(
            **item.to_dict()
        )
        for item in GEOMETRY_COVERAGE
    )
    return "\n".join(lines)


__all__ = [
    "GEOMETRY_COVERAGE",
    "CoverageStatus",
    "GeometryCategory",
    "GeometryCoverage",
    "geometry_coverage",
    "render_geometry_coverage_markdown",
]
