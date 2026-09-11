"""Deterministic public framing probes, not CAD-produced geometry or holdouts."""

from __future__ import annotations

import struct

from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_text import text_header


def release_fixtures() -> dict[str, bytes]:
    key = "SCH_3000000_30000"
    binary = SyntheticXbBuilder(schema_name=key, schema_max_type=None)
    binary.add_raw_node(82, struct.pack(">ihi", 3, 2, 0) + struct.pack(">ii", -32764, 2147483647))
    binary.add_raw_node(82, struct.pack(">ih", 0, 8))
    unknown = SyntheticXbBuilder(schema_name="SCH_9900000_99000", schema_max_type=None)
    unknown.add_raw_node(82, struct.pack(">ih", 0, 2))
    return {
        "values.x_t": text_header(key) + b"82 3 1 0 ?2147483647 82 0 91 1 0 ",
        "values.x_b": binary.build(),
        "unknown.x_b": unknown.build(),
    }
