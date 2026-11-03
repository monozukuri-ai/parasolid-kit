"""``read_brep(source_format="auto")`` recognises the common header and the ``.xmt_*`` suffixes.

The Siemens example transmit files are named ``.xmt_txt``; read by path they
failed with "cannot identify source encoding" because neither the suffix nor
the ``**ABCDEF...`` first line of the common header was recognised.
"""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest

from parasolid_kit import read_brep

DATA = Path(__file__).resolve().parents[1] / "data"


@pytest.mark.parametrize(
    "name,suffix,encoding",
    [
        ("onshape-box-v30.x_t", ".xmt_txt", "text"),
        ("onshape-box-v30.x_b", ".xmt_bin", "binary"),
        ("onshape-box-v30.x_t", ".dat", "text"),
        ("onshape-box-v30.x_b", ".dat", "binary"),
    ],
)
def test_auto_detection_by_suffix_and_common_header(tmp_path, name, suffix, encoding):
    path = tmp_path / f"model{suffix}"
    shutil.copyfile(DATA / name, path)
    parsed = read_brep(path)
    assert parsed.document.format == encoding
    assert len(parsed.brep.bodies) == 1
    # In-memory bytes rely on the content alone.
    assert read_brep(path.read_bytes()).document.format == encoding


def test_unrecognised_bytes_still_need_an_explicit_format():
    with pytest.raises(ValueError, match="cannot identify source encoding"):
        read_brep(b"not a transmit file at all")
