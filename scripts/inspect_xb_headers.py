#!/usr/bin/env python3
"""Print machine-readable M1 header inspection for one or more X_B files."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from parasolid_kit import ParseError, inspect_xb


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path, help="raw .x_b files to inspect")
    return parser


def main() -> int:
    """Inspect each path and return nonzero if any input is invalid."""

    reports: list[dict[str, object]] = []
    failed = False
    for path in _parser().parse_args().paths:
        try:
            reports.append(
                {
                    "path": path.as_posix(),
                    "status": "header_valid",
                    "header": inspect_xb(path).to_dict(),
                }
            )
        except ParseError as error:
            failed = True
            reports.append(
                {
                    "path": path.as_posix(),
                    "status": "invalid",
                    "diagnostic": error.diagnostic.to_dict(),
                }
            )
    print(json.dumps(reports, ensure_ascii=False, indent=2))
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
