from __future__ import annotations

import importlib
import importlib.util
import sys

import pytest

from tests.support.import_contract import assert_dependency_free_imports


@pytest.mark.parametrize("module", ["OCP", "cadquery"])
def test_import_contract_isolates_parent_but_rejects_child_native_import(module: str) -> None:
    if module == "cadquery" and sys.platform == "win32":
        pytest.skip("the CadQuery native runtime is unsupported on Windows")
    if importlib.util.find_spec(module) is None:
        pytest.skip(f"{module} is not installed in this profile")

    native_module = importlib.import_module(module)
    assert_dependency_free_imports("from parasolid_kit.interop import occt")
    assert sys.modules[module] is native_module

    with pytest.raises(AssertionError, match=f"unexpected optional CAD imports:.*{module}"):
        assert_dependency_free_imports(f"import {module}")
