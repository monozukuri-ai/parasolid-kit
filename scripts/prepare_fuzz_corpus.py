#!/usr/bin/env python3
"""Generate synthetic compiled-profile seeds; no CAD input or catalog is read."""

from __future__ import annotations

import argparse
import struct
from pathlib import Path

KEY = "SCH_3000000_30000"
MODELLER = ": TRANSMIT FILE created by modeller version 3000000"


def _headers() -> tuple[bytes, bytes]:
    text = f"T{len(MODELLER)} {MODELLER}{len(KEY)} {KEY}0 ".encode()
    binary = b"PS\0\0" + struct.pack(">H", len(MODELLER)) + MODELLER.encode()
    binary += struct.pack(">i", len(KEY)) + KEY.encode() + struct.pack(">i", 0)
    return text, binary


def build_seeds() -> dict[str, bytes]:
    """Return valid arrays/BODY and payloads that exceed documented fuzz limits."""

    text_header, binary_header = _headers()
    seeds = {}

    def pair(name: str, text: bytes, binary: bytes) -> None:
        seeds[f"{name}-text"] = text_header + text + b"1 0 "
        seeds[f"{name}-binary"] = binary_header + binary + bytes([0, 1, 0, 1])

    pair("integers", b"82 2 1 7 ?", struct.pack(">hihii", 82, 2, 2, 7, -32764))
    pair(
        "unicode",
        b"98 3 1 25991 -10179 -8704 ",
        struct.pack(">hihHHH", 98, 3, 2, 0x6587, 0xD83D, 0xDE00),
    )
    # Synthetic empty general BODY, for raw fields and dispatch rather than solid evidence.
    values = [0] * 33
    values[9], values[10], values[16] = 1e-6, 1e-8, 6
    binary_body = struct.pack(">hh", 12, 2)
    for ordinal, value in enumerate(values):
        if ordinal in (0, 27, 32):
            binary_body += struct.pack(">i", value)
        elif ordinal in (9, 10):
            binary_body += struct.pack(">d", value)
        elif ordinal in (14, 16, 17):
            binary_body += bytes([value])
        else:
            binary_body += struct.pack(">h", value + 1)
    pair("body", b"12 1 " + "".join(f"{v} " for v in values).encode(), binary_body)
    pair("array-limit", b"82 4097 1 ", struct.pack(">hih", 82, 4097, 2))
    pair(
        "node-limit",
        "".join(f"82 0 {i} " for i in range(1, 1026)).encode(),
        b"".join(struct.pack(">hih", 82, 0, i + 1) for i in range(1, 1026)),
    )
    return seeds


def main() -> int:
    """Write seeds to a runtime corpus directory, excluded from source archives."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("fuzz/corpus/parse"))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for name, data in build_seeds().items():
        (args.output / name).write_bytes(data)
    print(f"Wrote {len(build_seeds())} synthetic seeds to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
