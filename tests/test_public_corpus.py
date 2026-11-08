"""Regression over a local corpus of real transmit files, when one is configured.

Set ``PARASOLID_KIT_CORPUS`` to a directory of X_T/X_B files (for example the
public files named in ``docs/format-support.md``, which are not redistributed
here). Every file must parse, map a complete B-Rep and convert strictly unless
``expected.json`` in that directory lists it, as ``{"<file name>": "<reason>"}``.
Without the variable the test is skipped, so CI never depends on files it
does not have.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
CORPUS = os.environ.get("PARASOLID_KIT_CORPUS")

pytestmark = pytest.mark.skipif(
    not CORPUS or not Path(CORPUS).is_dir(), reason="PARASOLID_KIT_CORPUS is not a directory"
)


def test_every_corpus_file_parses_maps_and_converts_unless_expected():
    sys.path.insert(0, str(ROOT / "scripts"))
    import corpus_matrix

    directory = Path(CORPUS or ".")
    expected_path = directory / "expected.json"
    expected = json.loads(expected_path.read_text()) if expected_path.is_file() else {}
    rows = corpus_matrix.run_matrix([directory], sys.executable, timeout=900.0)
    corpus_matrix.print_table(rows)
    assert rows, "the corpus directory holds no Parasolid files"
    failures = {
        str(row["path"]): row.get("fatal") or row.get("parse") or row.get("occt")
        for row in rows
        if not corpus_matrix.is_success(row)
    }
    unexpected = {name: reason for name, reason in failures.items() if name not in expected}
    recovered = sorted(name for name in expected if name not in failures)
    assert not unexpected, f"unexpected failures: {unexpected}"
    assert not recovered, f"files listed in expected.json now succeed: {recovered}"
