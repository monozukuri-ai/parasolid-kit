"""Check public imports in a fresh instance of the test interpreter."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path
from textwrap import dedent

_CHECK_OPTIONAL_IMPORTS = """
import sys
loaded = sorted({
    name.split(".", 1)[0]
    for name in sys.modules
    if name.split(".", 1)[0] in {"OCP", "cadquery"}
})
assert not loaded, f"unexpected optional CAD imports: {loaded}"
"""


def assert_dependency_free_imports(source: str, *, timeout: float = 30) -> None:
    """Run imports and API assertions without inheriting the parent's module state."""
    command = [
        sys.executable,
        "-I",
        "-c",
        _CHECK_OPTIONAL_IMPORTS + dedent(source) + "\n" + _CHECK_OPTIONAL_IMPORTS,
    ]
    try:
        result = subprocess.run(
            command,
            cwd=Path(__file__).resolve().parents[2],
            capture_output=True,
            text=True,
            check=False,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as error:
        raise AssertionError(
            f"import contract timed out after {timeout}s\n"
            f"stdout:\n{error.stdout!r}\nstderr:\n{error.stderr!r}"
        ) from error
    assert result.returncode == 0, (
        f"import contract failed (exit {result.returncode}, timeout {timeout}s)\n"
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
