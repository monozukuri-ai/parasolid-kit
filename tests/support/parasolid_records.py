"""Authored X_T/X_B fixtures from per-type scalar codes and record values."""

from __future__ import annotations

import struct
from collections.abc import Mapping, Sequence

from .parasolid_binary import SyntheticXbBuilder
from .parasolid_schema import positive_integer
from .parasolid_text import text_header

Record = tuple[int, int, Mapping[int, object]]


def records_fixture(
    encoding: str,
    key: str,
    records: Sequence[Record],
    codes: Mapping[int, str],
    *,
    embedded: bool,
    user_fields: int = 0,
) -> bytes:
    """Encode ``(node_type, index, {ordinal: value})`` records under ``key``.

    ``codes`` maps a node type to its scalar codes in field order (``d`` int,
    ``p`` pointer, ``f`` double, ``u`` byte, ``c`` char, ``v`` vector, ``n``
    short, ``l`` logical). Unset values take a per-code default. Under an
    embedded key the first record of every type carries the unchanged base
    marker; a standard key transmits no schema.
    """

    schema_max_type = 205 if embedded else None
    binary = SyntheticXbBuilder(
        schema_name=key, schema_max_type=schema_max_type, user_field_size=user_fields
    )
    text = bytearray(text_header(key, schema_max_type=schema_max_type, user_field_size=user_fields))
    seen: set[int] = set()
    for kind, index, overrides in records:
        raw = bytearray()
        text.extend(f"{kind} ".encode())
        if embedded and kind not in seen:
            raw.append(0xFF)
            text.extend(b"255 ")
        seen.add(kind)
        raw.extend(positive_integer(index))
        text.extend(f"{index} ".encode())
        defaults: dict[str, object] = {
            "d": index + 100,
            "p": 0,
            "f": None,
            "u": 0,
            "c": "+",
            "v": (0, 0, 0),
            "n": 0,
            "l": 0,
        }
        for ordinal, code in enumerate(codes[kind]):
            value = overrides.get(ordinal, defaults[code])
            if code == "v":
                assert isinstance(value, tuple)
                raw.extend(struct.pack(">ddd", *value))
                text.extend((" ".join(str(v) for v in value) + " ").encode())
            elif code == "p":
                assert isinstance(value, int)
                raw.extend(positive_integer(value))
                text.extend(f"{value} ".encode())
            elif code == "c":
                assert isinstance(value, str)
                raw.extend(value.encode())
                text.extend(value.encode())
            elif code == "l":
                assert isinstance(value, int)
                raw.append(value)
                text.extend(b"T" if value else b"F")
            else:
                raw.extend(
                    struct.pack(
                        ">" + {"d": "i", "n": "h", "u": "B", "f": "d"}[code],
                        -3.14158e13 if value is None else value,
                    )
                )
                text.extend(b"?" if value is None else f"{value} ".encode())
        binary.add_raw_node(kind, raw)
    text.extend(b"1 0 ")
    return bytes(text) if encoding == "x_t" else binary.build()
