"""Generate public B-Rep cases with the installed writer and serve them for E2E."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import runpy
import sys
from contextlib import ExitStack
from dataclasses import replace
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]


def serve_previews(directory: Path, stack: ExitStack) -> dict[str, Any]:
    # Imports deliberately follow the optional cold-runtime audit guard.
    import parasolid_kit
    from parasolid_kit.interop.occt import (
        SourceEntityKind,
        SourceEntityRef,
        SourceShapeMap,
        to_occt,
    )
    from parasolid_kit.interop.preview import (
        ASSET_BUNDLE_VERSION,
        STATIC_ASSET_SHA256,
        PreviewOptions,
        create_preview_server,
        write_preview,
    )

    factories = runpy.run_path(str(ROOT / "tests/_occt_fixtures.py"))
    cases = {
        "box": ("make_box_model", "mm", True),
        "cylinder-hole": ("make_cylinder_hole_model", "mm", True),
        "two-boxes": ("make_two_box_model", "mm", True),
        "sheet": ("make_nurbs_surface_model", "mm", True),
        "box-cm-no-edges": ("make_box_model", "cm", False),
        "box-partial": ("make_box_model", "mm", True),
    }
    for name in (
        "box-unknown-edges",
        "box-diagnostics",
        "bad-glb",
        "bad-manifest",
        "bad-json",
        "missing",
        "incomplete",
    ):
        cases[name] = cases["box"]
    config: dict[str, Any] = {
        "input": "fresh public synthetic BrepModel -> installed OCCT/writer/Python server",
        "package": str(Path(parasolid_kit.__file__).resolve()),
        "python": sys.version,
        "assets": {
            "version": ASSET_BUNDLE_VERSION,
            "assets": {name: {"sha256": value} for name, value in STATIC_ASSET_SHA256.items()},
        },
        "urls": {},
        "oracle": {},
        "files": {},
    }
    for name, (factory, unit, edges) in cases.items():
        model = factories[factory]()
        sources = {}
        for kind in SourceEntityKind:
            collection = {"body": "bodies", "vertex": "vertices"}.get(kind.value, f"{kind.value}s")
            for entity in getattr(model, collection):
                ref = SourceEntityRef(kind, entity.id, entity.source)
                sources[ref.key] = ref.to_dict()
        config["oracle"][name] = {"sources": sources}
        converted = to_occt(model, source_unit="mm", target_unit=unit)
        if name == "box-partial":
            converted = replace(
                converted,
                source_map=SourceShapeMap(
                    tuple(
                        r
                        for r in converted.source_map.relations
                        if not (r.source.kind is SourceEntityKind.FACE and r.source.entity_id == 1)
                    )
                ),
            )
        result = write_preview(
            converted,
            model,
            directory / name,
            options=PreviewOptions(
                include_edges=edges,
                allow_partial=name == "box-partial",
            ),
        )
        config["files"][name] = {
            p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in result.directory.iterdir()
        }
        # Fault and injection cases modify generated metadata only after its baseline hashes.
        manifest = json.loads(result.manifest_path.read_text(encoding="ascii"))
        if name == "box-unknown-edges":
            for p in manifest["primitives"]:
                if p["kind"] == "edge":
                    p["body_ids"] = []
        if name == "box-diagnostics":
            manifest["primitives"][1]["diagnostic_codes"] = ["fixture.top"]
            manifest["primitives"][1]["source_entities"][0]["relation_note"] = (
                '<img src="https://example.invalid/source" onerror="window.injected=true">'
            )
            manifest["diagnostics"] = [
                {
                    "code": "fixture.top",
                    "severity": "warning",
                    "message": '<img src="https://example.invalid/diagnostic" '
                    'onerror="window.injected=true">',
                }
            ]
        if name == "bad-manifest":
            manifest["primitives"][0]["pick_id"] = 99
        if name == "incomplete":
            manifest["source"]["complete"] = False
            manifest["preview"]["partial"] = True
            manifest["preview"]["options"]["allow_partial"] = True
        if name in {"box-unknown-edges", "box-diagnostics", "bad-manifest", "incomplete"}:
            result.manifest_path.write_text(json.dumps(manifest), encoding="ascii")
        if name == "bad-json":
            result.manifest_path.write_text("{invalid JSON", encoding="ascii")
        if name == "bad-glb":
            result.glb_path.write_bytes(result.glb_path.read_bytes()[:20])
        server = stack.enter_context(create_preview_server(result.directory))
        server.start()
        if name == "missing":
            result.glb_path.unlink()
        config["urls"][name] = server.url
    return config


def install_runtime_guard(forbidden: list[Path], environment: Path) -> None:
    """Deny source-tree reads, process launches and non-loopback Python networking."""
    forbidden = [p.resolve() for p in forbidden]

    def guard(event: str, args: tuple[Any, ...]) -> None:
        if event == "open" and isinstance(args[0], (str, bytes)):
            path = Path(os.fsdecode(args[0])).resolve()
            if any(path.is_relative_to(p) for p in forbidden) or path.suffix == ".sch_txt":
                raise RuntimeError("source checkout/catalog read forbidden during viewer runtime")
        if event in {"subprocess.Popen", "os.system", "os.posix_spawn", "os.exec", "os.fork"}:
            raise RuntimeError("process launch forbidden during viewer runtime")
        if (
            event in {"socket.connect", "socket.bind", "socket.sendto"}
            and args[-1][0] != "127.0.0.1"
        ):
            raise RuntimeError("non-loopback network forbidden during viewer runtime")
        if event in {"socket.getaddrinfo", "socket.gethostbyname", "socket.gethostbyaddr"} and args[
            0
        ] not in {"127.0.0.1", "localhost"}:
            raise RuntimeError("external DNS forbidden during viewer runtime")

    sys.addaudithook(guard)
    import parasolid_kit
    from parasolid_kit import cli
    from parasolid_kit.interop import preview

    assert Path(parasolid_kit.__file__).resolve().is_relative_to(environment.resolve())
    assert "OCP" not in sys.modules and "cadquery" not in sys.modules
    assert callable(cli.main) and callable(preview.write_preview)
    assert os.environ["PATH"] == ""
    for root in forbidden:
        try:
            (root / "pyproject.toml").read_bytes()
        except RuntimeError:
            pass
        else:
            raise AssertionError("source guard did not reject a probe")
    import socket
    import subprocess

    try:
        subprocess.run(["node", "--version"], check=True)
    except RuntimeError:
        pass
    else:
        raise AssertionError("process guard did not reject a probe")
    with socket.socket() as connection:
        try:
            connection.connect(("203.0.113.1", 443))
        except RuntimeError:
            pass
        else:
            raise AssertionError("network guard did not reject a probe")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--forbid-root", type=Path, action="append", default=[])
    parser.add_argument("--environment", type=Path)
    args = parser.parse_args()
    if args.forbid_root:
        if args.environment is None:
            parser.error("--forbid-root requires --environment")
        install_runtime_guard(args.forbid_root, args.environment)
    with ExitStack() as stack:
        config = serve_previews(args.directory, stack)
        config["runtime_guard"] = bool(args.forbid_root)
        print(json.dumps(config), flush=True)
        sys.stdin.read()  # The harness closes stdin after browser checks; close all servers.


if __name__ == "__main__":
    main()
