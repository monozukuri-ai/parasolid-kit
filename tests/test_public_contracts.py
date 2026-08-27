from __future__ import annotations

import parasolid_kit
from parasolid_kit import _core, schema


def test_public_contracts_import_without_optional_cad_dependencies() -> None:
    limits = parasolid_kit.ParseLimits(max_nodes=100)

    assert parasolid_kit.__version__ == "0.1.0.dev0"
    assert limits.max_nodes == 100
    assert parasolid_kit.DEFAULT_PARSE_LIMITS.max_file_size > 0
    assert callable(parasolid_kit.inspect_xb)
    assert callable(parasolid_kit.inspect_xt)
    assert callable(parasolid_kit.parse_xb)
    assert callable(parasolid_kit.parse_xt)
    assert callable(parasolid_kit.compare_documents)
    assert callable(parasolid_kit.map_brep)
    assert callable(parasolid_kit.write_xb)
    assert callable(parasolid_kit.load_schema_catalog)
    assert parasolid_kit.XbBinaryFormat.NEUTRAL.value == "neutral"
    assert parasolid_kit.XbHeader.__module__ == "parasolid_kit.binary.header"
    assert parasolid_kit.XtHeader.__module__ == "parasolid_kit.text.header"
    assert parasolid_kit.BrepModel.__module__ == "parasolid_kit.brep.model"
    assert parasolid_kit.Body.__module__ == "parasolid_kit.brep.topology"
    assert parasolid_kit.CurveGeometry.__module__ == "parasolid_kit.brep.geometry"
    assert parasolid_kit.Sense.UNKNOWN.value == "unknown"
    assert callable(schema.resolve_schema_blob)
    assert callable(schema.schema_coverage)
    assert callable(schema.load_schema_catalog)
    assert schema.SchemaKey.parse("SCH_3000000_30000_13006").provider_schema == "13006"
    assert schema.FieldType.DOUBLE.value == "f"
    assert schema.SchemaSource.EMBEDDED_FULL.value == "embedded_full"
    assert _core.CORE_VERSION == "0.1.0-dev0"
    assert "_core" not in parasolid_kit.__all__
