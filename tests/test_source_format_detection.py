"""``read_brep(source_format="auto")`` recognises the ``.xmt_*`` suffixes and the stream prefix.

The Siemens example transmit files are named ``.xmt_txt``; read by path they
failed with "cannot identify source encoding" because neither the suffix nor
the ``**ABCDEF...`` first line of the common header was recognised. The
fixtures are the authored general body in both encodings, bare and behind a
common header.
"""

from __future__ import annotations

import pytest

from parasolid_kit import read_brep
from tests.test_builtin_schema_runtime import general_body_payload

COMMON_HEADER = {
    "x_t": (
        b"**PART1;FORMAT=text;\n**PART2;SCH=SCH_9999999_99999;USFLD_SIZE=0;\n"
        b"**PART3;\n**END_OF_HEADER***\n"
    ),
    "x_b": (
        b"**PART1;FORMAT=binary;\n**PART2;SCH=SCH_9999999_99999;USFLD_SIZE=0;\n"
        b"**PART3;\n**END_OF_HEADER***\n"
    ),
}
FORMAT = {"x_t": "text", "x_b": "binary"}


def payload(encoding: str, *, common_header: bool) -> bytes:
    data = general_body_payload(encoding)
    return COMMON_HEADER[encoding] + data if common_header else data


@pytest.mark.parametrize(
    "encoding,suffix,common_header",
    [
        ("x_t", ".xmt_txt", False),
        ("x_t", ".xmt_txt", True),
        ("x_b", ".xmt_bin", False),
        ("x_b", ".xmt_bin", True),
        # An unknown suffix leaves the decision to the stream: the bare ``T``
        # line or the ``**`` common header is text, the ``PS`` marker binary.
        ("x_t", ".dat", False),
        ("x_t", ".dat", True),
        ("x_b", ".dat", False),
        ("x_b", ".dat", True),
    ],
)
def test_auto_detection_by_suffix_and_stream_prefix(tmp_path, encoding, suffix, common_header):
    data = payload(encoding, common_header=common_header)
    path = tmp_path / f"model{suffix}"
    path.write_bytes(data)
    parsed = read_brep(path)
    assert parsed.document.format == FORMAT[encoding]
    assert len(parsed.brep.bodies) == 1
    # In-memory bytes rely on the content alone.
    assert read_brep(data).document.format == FORMAT[encoding]


def test_the_suffix_decides_before_the_stream_is_inspected(tmp_path):
    path = tmp_path / "renamed.xmt_txt"
    path.write_bytes(payload("x_t", common_header=True))
    assert read_brep(path, source_format="x-t").document.format == "text"
    assert read_brep(path).document.format == "text"


def test_unrecognised_bytes_still_need_an_explicit_format():
    with pytest.raises(ValueError, match="cannot identify source encoding"):
        read_brep(b"not a transmit file at all")
