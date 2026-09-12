"""Regenerate public, synthetic OCCT preview fixtures and an independent source oracle.

Run from the repository with its optional OCCT environment. The Node tests use the
checked-in output and do not need Python/OCP. These are not parsed private CAD files.
"""

from __future__ import annotations

import hashlib
import json
import runpy
from dataclasses import replace
from pathlib import Path

from parasolid_kit.interop.occt import SourceEntityKind, SourceEntityRef, SourceShapeMap, to_occt
from parasolid_kit.interop.preview import PreviewOptions
from parasolid_kit.interop.preview.tessellation import tessellate_preview

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def main() -> None:
    factories = runpy.run_path(str(ROOT / "tests/_occt_fixtures.py"))
    cases = {
        "box": ("make_box_model", "mm", True),
        "box-partial": ("make_box_model", "mm", True),
        "cylinder-hole": ("make_cylinder_hole_model", "mm", True),
        "two-boxes": ("make_two_box_model", "mm", True),
        "sheet": ("make_nurbs_surface_model", "mm", True),
        "box-cm-no-edges": ("make_box_model", "cm", False),
    }
    oracle = {}
    for name, (factory, unit, edges) in cases.items():
        model = factories[factory]()
        converted = to_occt(model, source_unit="mm", target_unit=unit)
        if name == "box-partial":
            converted = replace(
                converted,
                source_map=SourceShapeMap(
                    tuple(
                        relation
                        for relation in converted.source_map.relations
                        if not (
                            relation.source.kind is SourceEntityKind.FACE
                            and relation.source.entity_id == 1
                        )
                    )
                ),
            )
        preview = tessellate_preview(
            converted,
            model,
            options=PreviewOptions(include_edges=edges, allow_partial=name == "box-partial"),
        )
        directory = HERE / "fixtures" / name
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "preview.glb").write_bytes(preview.glb)
        (directory / "preview.manifest.json").write_text(
            json.dumps(preview.manifest, indent=2, sort_keys=True) + "\n", encoding="ascii"
        )
        sources = {}
        for kind in SourceEntityKind:
            collection = {"body": "bodies", "vertex": "vertices"}.get(kind.value, f"{kind.value}s")
            for entity in getattr(model, collection):
                ref = SourceEntityRef(kind, entity.id, entity.source)
                sources[ref.key] = ref.to_dict()
        oracle[name] = {
            "sources": sources,
            "bodies": [{"id": b.id, "kind": b.kind.value} for b in model.bodies],
            "inputs": {
                p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                for p in [directory / "preview.glb", directory / "preview.manifest.json"]
            },
        }
    (HERE / "fixtures/oracle.json").write_text(
        json.dumps(oracle, indent=2, sort_keys=True) + "\n", encoding="ascii"
    )


if __name__ == "__main__":
    main()
