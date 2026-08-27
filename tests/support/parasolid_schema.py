"""Deterministic embedded-schema bytes built from documented field framing."""

from __future__ import annotations

import struct


def positive_integer(value: int) -> bytes:
    """Encode a non-negative integer using the pointer-index representation."""

    if not 0 <= value <= 1_073_709_055:
        raise ValueError("value is outside the compact positive-integer range")
    if value < 32_767:
        return struct.pack(">h", value + 1)
    quotient, remainder = divmod(value, 32_767)
    if quotient > 32_767:
        raise ValueError("positive-integer quotient does not fit a signed short")
    return struct.pack(">hh", -(remainder + 1), quotient)


def short_string(value: str) -> bytes:
    """Encode an ASCII string with one unsigned-byte length."""

    encoded = value.encode("ascii")
    if len(encoded) > 255:
        raise ValueError("short string exceeds 255 bytes")
    return bytes((len(encoded),)) + encoded


def embedded_field(
    name: str,
    *,
    field_type: str = "d",
    pointer_class: int = 0,
    element_count: int = 0,
    transmitted: bool = True,
) -> bytes:
    """Encode one full or inserted effective field."""

    output = bytearray(short_string(name))
    output.extend(struct.pack(">H", pointer_class))
    output.extend(positive_integer(element_count))
    if pointer_class == 0:
        output.extend(short_string(field_type))
    if element_count == 1:
        output.append(int(transmitted))
    return bytes(output)


def full_schema(name: str, description: str, fields: tuple[bytes, ...]) -> bytes:
    """Encode one complete definition for a type absent from the base schema."""

    if len(fields) > 255:
        raise ValueError("full schema field count exceeds one byte")
    return b"".join((bytes((len(fields),)), short_string(name), short_string(description), *fields))
