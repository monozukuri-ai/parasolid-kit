# Installation

## Parser

Python 3.10+ is required. The parser supports Linux, macOS and Windows and has
no runtime dependency on a CAD product or geometry kernel.

```bash
python -m pip install parasolid-kit
parasolid-kit --help
```

Header inspection works immediately. Parsing and source B-Rep checking need no
schema file for the exact-key [supported profiles](format-support.md#supported-profiles).
For other keys or uncovered types, follow the [schema catalog guide](schema-catalogs.md).
Native CAD containers require their own adapters.

To install a downloaded wheel, pass its filename:

```bash
python -m pip install /path/to/parasolid_kit-0.2.0-cp310-abi3-PLATFORM.whl
```

Replace the filename with the actual wheel for your platform. See the
[changelog](../CHANGELOG.md) for version-specific changes and migration notes.

## Optional runtimes

Choose one optional profile for conversion, STEP export and preview:

| Profile | Platforms | Python | Runtime |
|---|---|---|---|
| `occt` | Linux, macOS, Windows | 3.10+ | Headless OCP without CadQuery or VTK |
| `cadquery` | Linux, macOS | 3.11+; Intel macOS: 3.11–3.13 | CadQuery and its full OCP runtime |

```bash
python -m pip install "parasolid-kit[occt]"
```

Or, in a separate environment:

```bash
python -m pip install "parasolid-kit[cadquery]"
```

Do not install both profiles in one environment: their distributions provide
the same `OCP` namespace. The package detects conflicts before native imports
and reports recovery commands. There is no `[all]` extra. Normal parsing does
not import the optional runtime.

On Windows, the tested CadQuery runtime crashes during process shutdown.
CadQuery adapter calls therefore raise `interop.unsupported_platform` before
the native import. Use `[occt]` for Windows conversion, STEP export and preview.

On Intel macOS, the CadQuery extra pins Numba to 0.62.x for prebuilt wheels.
Use Python 3.11–3.13; see
[Numba's platform notice](https://numba.readthedocs.io/en/stable/release/0.63.0-notes.html#deprecation-of-macos-x86-64-intel-support).

Both profiles use the same strict OCCT converter and supported geometry subset.
The CadQuery adapter wraps that result as `Shape` objects. The viewer uses the
same OCCT result and bundled assets; VTK, Node.js and a CDN are unnecessary for
viewing. Optional-runtime installation does not expand parser or conversion
coverage. See [geometry support](format-support.md#parse-occt-and-step-geometry-coverage).

To check the selected OCCT runtime:

```python
from parasolid_kit.interop import require_occt

OCP = require_occt()
```

See the API examples for [OCCT conversion](api.md#strict-occt-conversion),
[STEP export](api.md#direct-ap242-export), [CadQuery shapes](api.md#cadquery-shape-adapter)
and [local previews](api.md#bounded-local-preview).

## Source checkout

Source builds also require Rust 1.88+. Create a fresh environment and install
from the repository root:

```bash
git clone https://github.com/monozukuri-ai/parasolid-kit.git
cd parasolid-kit
python -m venv .venv
source .venv/bin/activate
python -m pip install .
```

On Windows, activate with `.venv\Scripts\Activate.ps1` in PowerShell instead of
`source`. For the source-tree viewer and OCCT conversion, replace the last
command with `python -m pip install ".[occt]"`. Choose `".[cadquery]"` instead
for the CadQuery profile on its supported platforms.

Building the viewer frontend itself requires Node/npm; ordinary source
installation uses the bundled assets. See [viewer development](../viewer/README.md)
for frontend build and test commands.
