"""Run the real CLI with a public synthetic B-Rep in place of the parser."""

from __future__ import annotations

import runpy
import sys
import webbrowser
from pathlib import Path
from types import SimpleNamespace

from parasolid_kit import cli

ROOT = Path(__file__).resolve().parents[2]
fixtures = runpy.run_path(str(ROOT / "tests" / "_occt_fixtures.py"))


def read_fixture(*_args: object, **_kwargs: object) -> SimpleNamespace:
    return SimpleNamespace(
        brep=fixtures["make_two_box_model"](),
        document=SimpleNamespace(raw_bytes=b"public synthetic two-box B-Rep"),
    )


def unexpected_browser(*_args: object, **_kwargs: object) -> bool:
    raise AssertionError("--no-open must not launch a browser")


if __name__ == "__main__":
    # Only parsing is replaced. Exercise normal argument parsing, OCCT, writer,
    # bundled resources, HTTP server, JSON flushing and interrupt cleanup.
    cli.read_brep = read_fixture
    webbrowser.open = unexpected_browser
    raise SystemExit(cli.main(sys.argv[1:]))
