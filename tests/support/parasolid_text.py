"""Small X_T fixture helpers with the physical delimiter rules kept explicit."""

from __future__ import annotations


def text_header(
    schema_name: str = "SCH_3000000_30000",
    *,
    modeller_version: str = ": TRANSMIT FILE created by modeller version 3000000",
    schema_max_type: int | None = None,
    user_field_size: int = 0,
    common_header: bool = False,
) -> bytes:
    """Build the internal T header, optionally preceded by a common header."""

    payload = f"T{len(modeller_version)} {modeller_version}{len(schema_name)} {schema_name}"
    if schema_max_type is not None:
        payload += f"{schema_max_type} "
    payload += f"{user_field_size} "
    prefix = (
        b"**PART1;FORMAT=text;\n**PART2;SCH=SCH_9999999_99999;USFLD_SIZE=0;\n"
        b"**PART3;\n**END_OF_HEADER***\n"
        if common_header
        else b""
    )
    return prefix + payload.encode("ascii")
