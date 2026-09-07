#!/usr/bin/env python3
"""Generate synthetic compiled-profile seeds; no CAD input or catalog is read."""

from __future__ import annotations

import argparse
import struct
from pathlib import Path

KEY = "SCH_3000000_30000"
MODELLER = ": TRANSMIT FILE created by modeller version 3000000"


def _headers(key: str = KEY) -> tuple[bytes, bytes]:
    embedded = key.count("_") == 3
    text = f"T{len(MODELLER)} {MODELLER}{len(key)} {key}{'205 ' if embedded else ''}0 ".encode()
    binary = b"PS\0\0" + struct.pack(">H", len(MODELLER)) + MODELLER.encode()
    binary += struct.pack(">i", len(key)) + key.encode()
    binary += (struct.pack(">H", 205) if embedded else b"") + struct.pack(">i", 0)
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
    text_header, binary_header = _headers("SCH_1300000_13006")
    pair("base-integers", b"82 2 1 7 ?", struct.pack(">hihii", 82, 2, 2, 7, -32764))
    cone_text = b"1 73 0 0 0 0 0 --0.011 0.017 -0.023 0.6 0.8 0 0.007013 -0.6 0.8 0 0 -1 "
    cone_binary = struct.pack(">hi5h", 2, 73, 1, 1, 1, 1, 1) + b"-"
    cone_binary += struct.pack(
        ">12d", -0.011, 0.017, -0.023, 0.6, 0.8, 0, 0.007013, -0.6, 0.8, 0, 0, -1
    )
    pair("base-cone", b"52 " + cone_text, struct.pack(">h", 52) + cone_binary)
    text_header, binary_header = _headers("SCH_3000310_30000_13006")
    pair("embedded-cone", b"52 255 " + cone_text, struct.pack(">hB", 52, 255) + cone_binary)
    pair(
        "embedded-unchanged",
        b"82 255 2 1 7 ?",
        struct.pack(">hBihii", 82, 255, 2, 2, 7, -32764),
    )
    pair(
        "embedded-copy",
        b"82 1 CZ2 1 7 ?",
        struct.pack(">h", 82) + b"\x01CZ" + struct.pack(">ihii", 2, 2, 7, -32764),
    )
    pair("embedded-unknown", b"110 0 1 ", struct.pack(">hBh", 110, 0, 2))
    # Complete unchanged trimmed-curve declaration and a forward line basis.
    trim_binary = struct.pack(">HBhi5h", 133, 255, 2, 73, 1, 1, 1, 1, 1)
    trim_binary += b"+" + struct.pack(">h8d", 3, 0.011, 0, 0, 0.019, 0, 0, 0.011, 0.019)
    trim_binary += struct.pack(">HBhi5h", 30, 255, 3, 74, 1, 1, 1, 1, 1)
    trim_binary += b"+" + struct.pack(">6d", 0, 0, 0, 1, 0, 0)
    pair(
        "embedded-trimmed",
        b"133 255 1 73 0 0 0 0 0 +2 0.011 0 0 0.019 0 0 0.011 0.019 "
        b"30 255 2 74 0 0 0 0 0 +0 0 0 1 0 0 ",
        trim_binary,
    )
    text_header, binary_header = _headers("SCH_1300000_13006")
    pair(
        "base-trimmed",
        b"133 1 73 0 0 0 0 0 +2 0.011 0 0 0.019 0 0 0.011 0.019 30 2 74 0 0 0 0 0 +0 0 0 1 0 0 ",
        trim_binary.replace(struct.pack(">HB", 133, 255), struct.pack(">H", 133), 1).replace(
            struct.pack(">HB", 30, 255), struct.pack(">H", 30), 1
        ),
    )
    # V13 SP_CURVE with all seven dependency layouts, including short arrays.
    text_header, binary_header = _headers("SCH_1300000_13006")
    sp_binary = struct.pack(">Hhi5h", 137, 2, 73, 1, 1, 1, 1, 1) + b"+"
    sp_binary += struct.pack(">3hd", 1, 3, 1, -3.14158e13)
    sp_binary += struct.pack(">Hhi5h", 134, 3, 74, 1, 1, 1, 1, 1) + b"+"
    sp_binary += struct.pack(">2h", 4, 5)
    sp_binary += struct.pack(">Hhhihi5B3h", 136, 4, 1, 2, 2, 2, 5, 0, 0, 0, 1, 6, 7, 8)
    sp_binary += struct.pack(">HhBh", 135, 5, 1, 1)
    sp_binary += struct.pack(">Hih4d", 45, 4, 6, 0.011, -0.017, 0.023, 0.031)
    sp_binary += struct.pack(">Hih2h", 127, 2, 7, 2, 2)
    sp_binary += struct.pack(">Hih2d", 128, 2, 8, 0, 1)
    pair(
        "base-spcurve",
        b"137 1 73 0 0 0 0 0 +0 2 0 ?134 2 74 0 0 0 0 0 +3 4 "
        b"136 3 1 2 2 2 5 FFF1 5 6 7 135 4 1 0 "
        b"45 4 5 0.011 -0.017 0.023 0.031 127 2 6 2 2 128 2 7 0 1 ",
        sp_binary,
    )
    text_header, binary_header = _headers("SCH_3000310_30000_13006")
    # Full declaration followed by a cached instance with an empty value array.
    full_text = b"2 17 INTERSECTION_DATA17 Intersection data7 uv_type0 0 1 u6 values0 1 1 fT"
    full_binary = (
        b"\x02\x11INTERSECTION_DATA\x11Intersection data"
        b"\x07uv_type\x00\x00\x00\x01\x01u"
        b"\x06values\x00\x00\x00\x02\x01f\x01"
    )
    pair(
        "embedded-full204",
        b"204 " + full_text + b"2 1 4 0.125 -0.75 204 0 2 4 ",
        struct.pack(">H", 204)
        + full_binary
        + struct.pack(">ihB2dHihB", 2, 2, 4, 0.125, -0.75, 204, 0, 3, 4),
    )
    for prefix, key in [("base", "SCH_1300000_13006"), ("embedded", "SCH_3000310_30000_13006")]:
        text_header, binary_header = _headers(key)
        marker, binary_marker = (b"255 ", b"\xff") if prefix == "embedded" else (b"", b"")
        text = b"38 " + marker + b"1 73 0 0 0 0 0 +0 0 2 3 3 "
        binary = struct.pack(">H", 38) + binary_marker + struct.pack(">hi5h", 2, 73, 1, 1, 1, 1, 1)
        binary += b"+" + struct.pack(">5h", 1, 1, 3, 4, 4)
        text += b"40 " + marker + b"2 2 0 1 2 ????0.011 0 0 0.019 0 0 "
        binary += struct.pack(">H", 40) + binary_marker + struct.pack(">ihddi", 2, 3, 0, 1, 2)
        binary += struct.pack(">10d", *([-3.14158e13] * 4), 0.011, 0, 0, 0.019, 0, 0)
        text += b"41 " + marker + b"1 3 L0.011 0 0 "
        binary += (
            struct.pack(">H", 41)
            + binary_marker
            + struct.pack(">ih", 1, 4)
            + b"L"
            + struct.pack(">3d", 0.011, 0, 0)
        )
        text += b"141 " + marker + b"4 1 4 4 0 "
        binary += struct.pack(">H", 141) + binary_marker + struct.pack(">5h", 5, 2, 5, 5, 1)
        pair(prefix + "-intersection", text, binary)
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
