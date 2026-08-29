from __future__ import annotations

from dataclasses import replace
from pathlib import Path

import pytest

from parasolid_kit import (
    Body,
    BodyKind,
    BoundingBox,
    BrepMetrics,
    BrepModel,
    BrepSummary,
    ByteRange,
    CurveGeometry,
    CurveKind,
    Diagnostic,
    DiagnosticKind,
    DiagnosticSeverity,
    DirectorySchemaProvider,
    InMemorySchemaProvider,
    LineCurve,
    ParasolidDocument,
    ParsedBrep,
    PlaneSurface,
    PointGeometry,
    SchemaCatalog,
    SchemaProvider,
    Sense,
    SourceNodeRef,
    SurfaceGeometry,
    SurfaceKind,
    TopologyValidation,
    Vector3,
    load_schema_catalog,
    parse_xb,
    read_brep,
)
from parasolid_kit import api as api_module
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.test_schema_catalog import _catalog_bytes


def _document() -> ParasolidDocument:
    catalog = load_schema_catalog(_catalog_bytes(), expected_schema_id="30000")
    return parse_xb(
        SyntheticXbBuilder(
            schema_name="SCH_3000000_30000",
            schema_max_type=None,
        ).build(),
        schema_provider=InMemorySchemaProvider((catalog,)),
    )


def _source() -> SourceNodeRef:
    return SourceNodeRef(
        node_index=1,
        node_type=12,
        type_name="SYNTHETIC",
        node_id=10,
        byte_range=ByteRange(0, 1),
    )


def _model(document: ParasolidDocument, *, complete: bool = False) -> BrepModel:
    source = _source()
    diagnostic = Diagnostic(
        code="geometry.unsupported_curve",
        severity=DiagnosticSeverity.WARNING,
        kind=DiagnosticKind.UNSUPPORTED,
        message="synthetic unsupported geometry",
        node_type=75,
        node_id=10,
    )
    return BrepModel(
        source_format=document.format,
        schema_key=document.schema_key.raw,
        complete=complete,
        bodies=(
            Body(
                id=10,
                kind=BodyKind.SOLID,
                size_resolution=1.0e-6,
                linear_resolution=1.0e-6,
                regions=(),
                edges=(),
                vertices=(),
                source=source,
            ),
        ),
        regions=(),
        shells=(),
        faces=(),
        loops=(),
        half_edges=(),
        edges=(),
        vertices=(),
        points=(
            PointGeometry(
                id=20,
                position=Vector3(1.0, 2.0, 3.0),
                owner=None,
                source=source,
            ),
        ),
        curves=(
            CurveGeometry(
                id=30,
                sense=Sense.POSITIVE,
                owner=None,
                kind=CurveKind.LINE,
                definition=LineCurve(
                    point=Vector3(0.0, 0.0, 0.0),
                    direction=Vector3(1.0, 0.0, 0.0),
                ),
                source=source,
            ),
        ),
        surfaces=(
            SurfaceGeometry(
                id=40,
                sense=Sense.POSITIVE,
                owner=None,
                kind=SurfaceKind.PLANE,
                definition=PlaneSurface(
                    point=Vector3(0.0, 0.0, 0.0),
                    normal=Vector3(0.0, 0.0, 1.0),
                    x_axis=Vector3(1.0, 0.0, 0.0),
                ),
                source=source,
            ),
        ),
        topology=TopologyValidation(
            valid=True,
            closed_loop_count=2,
            closed_edge_ring_count=3,
            euler_characteristic=1,
        ),
        metrics=BrepMetrics(
            bounding_box=BoundingBox(
                minimum=Vector3(1.0, 2.0, 3.0),
                maximum=Vector3(4.0, 6.0, 8.0),
            ),
            surface_area=12.5,
            volume=4.25,
        ),
        diagnostics=() if complete else (diagnostic,),
    )


def test_brep_summary_is_compact_deterministic_and_keeps_diagnostic_layers() -> None:
    document = _document()
    document_diagnostic = Diagnostic(
        code="document.incomplete_fixture",
        severity=DiagnosticSeverity.WARNING,
        kind=DiagnosticKind.INCOMPLETE,
        message="synthetic document diagnostic",
    )
    document = replace(document, diagnostics=(document_diagnostic,))
    model = _model(document)

    summary = BrepSummary.from_parsed(document, model)
    serialized = summary.to_dict()

    assert summary.complete is False
    assert summary.counts.bodies == 1
    assert summary.counts.points == 1
    assert summary.body_kind_counts == (("solid", 1),)
    assert summary.curve_kind_counts == (("line", 1),)
    assert summary.surface_kind_counts == (("plane", 1),)
    assert summary.diagnostic_count == 2
    assert serialized["schema_key"]["provider_schema"] == "30000"  # type: ignore[index]
    assert serialized["body_kind_counts"] == {"solid": 1}
    assert serialized["curve_kind_counts"] == {"line": 1}
    assert serialized["surface_kind_counts"] == {"plane": 1}
    assert serialized["metrics"] == {
        "unit_basis": "source_transmit_units",
        "bounding_box": {
            "minimum": [1.0, 2.0, 3.0],
            "maximum": [4.0, 6.0, 8.0],
            "extents": [3.0, 4.0, 5.0],
        },
        "surface_area": 12.5,
        "volume": 4.25,
    }
    assert serialized["diagnostics"]["document"][0]["code"] == (  # type: ignore[index]
        "document.incomplete_fixture"
    )
    assert serialized["diagnostics"]["brep"][0]["code"] == (  # type: ignore[index]
        "geometry.unsupported_curve"
    )


def test_read_brep_auto_detects_bytes_and_uses_the_exact_directory_catalog(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    schema_path = tmp_path / "sch_30000.sch_txt"
    schema_path.write_bytes(_catalog_bytes())
    source = SyntheticXbBuilder(
        schema_name="SCH_3000000_30000",
        schema_max_type=None,
    ).build()

    def map_synthetic(document: ParasolidDocument, **_kwargs: object) -> BrepModel:
        return _model(document)

    monkeypatch.setattr(api_module, "map_brep", map_synthetic)

    result = read_brep(source, schema_dir=tmp_path)

    assert isinstance(result, ParsedBrep)
    assert result.document.format == "binary"
    assert result.brep.source_format == "binary"
    assert result.summary.schema_key.provider_schema == "30000"
    assert result.summary.file_size == len(source)
    assert result.complete is False


def test_read_brep_rejects_ambiguous_configuration_and_reports_the_exact_missing_path(
    tmp_path: Path,
) -> None:
    source = SyntheticXbBuilder(
        schema_name="SCH_3000000_30000",
        schema_max_type=None,
    ).build()
    provider = InMemorySchemaProvider((SchemaCatalog("30000", ()),))

    with pytest.raises(ValueError, match="mutually exclusive"):
        read_brep(source, schema_provider=provider, schema_dir=tmp_path)

    with pytest.raises(FileNotFoundError) as captured:
        read_brep(source, schema_dir=tmp_path)
    assert str(captured.value).endswith("sch_30000.sch_txt")

    with pytest.raises(ValueError, match="source_format"):
        read_brep(source, schema_provider=provider, source_format="binary")  # type: ignore[arg-type]


def test_summary_rejects_a_document_model_identity_mismatch() -> None:
    document = _document()
    model = replace(_model(document), schema_key="SCH_3000000_13006")

    with pytest.raises(ValueError, match="schema keys"):
        BrepSummary.from_parsed(document, model)


def test_directory_provider_satisfies_the_runtime_protocol(tmp_path: Path) -> None:
    assert isinstance(DirectorySchemaProvider(tmp_path), SchemaProvider)
