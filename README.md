# parasolid-kit

A schema-aware parser for Parasolid X_T and X_B transmit files. A safe Rust
core handles parsing and geometry mapping; Python exposes immutable typed
models and a command-line interface. The project focuses on reading and
preserving the transmitted structure for inspection and conversion pipelines.

Rust applications can use [parasolid-core](crates/parasolid-core/README.md)
directly, without Python.

## Features

- Inspect headers and map supported topology, analytic geometry and NURBS to
  a typed source B-Rep.
- Compare X_T/X_B documents after pointer remapping and reconstruct an
  unmodified parsed X_B document byte-for-byte.
- Report structured diagnostics and enforce configurable resource limits.
- Use optional OCCT conversion, validated STEP export, CadQuery shapes and a
  local viewer with face/edge source picking, body visibility and section views.

## Installation

Python 3.10+ is required. The base package has no runtime dependencies:

```bash
python -m pip install parasolid-kit
```

For STEP export and the viewer, install the OCCT profile:

```bash
python -m pip install "parasolid-kit[occt]"
```

See [installation](docs/installation.md) for platform requirements, the separate
CadQuery profile, downloaded wheels and source builds.

## Quick start

For an input covered by a built-in profile:

```bash
parasolid-kit inspect model.x_b          # Header only; no schema catalog needed.
parasolid-kit check model.x_b            # Parse and check the source B-Rep.
parasolid-kit check model.x_t --json     # Structured report.
parasolid-kit compare model.x_t model.x_b
```

The same workflow is available in Python:

```python
from parasolid_kit import read_brep

parsed = read_brep("model.x_b")
print(parsed.summary.to_dict())
print(parsed.complete)
print(parsed.brep.diagnostics)
```

`complete` describes source B-Rep mapping. Geometry conversion and a native CAD
file's saved configuration have separate requirements. Use `check` when
completeness must affect the CLI exit code. See the [Python API](docs/api.md)
and [CLI reference](docs/api.md#cli) for lower-level operations and diagnostics.

## Local viewer

To try the viewer from a source checkout with Rust 1.88+ installed:

```bash
python -m pip install ".[occt]"
parasolid-kit viewer model.x_t --source-unit mm
```

Choose the input's actual unit; it is never inferred. `view` is an alias.
The command opens a local URL; Ctrl-C stops serving and keeps the generated
files. Node.js and a CDN are unnecessary at runtime.

![Viewer showing two boxes and the selected face's source ID, node and byte range](docs/images/viewer.png)

Project-authored synthetic two-body fixture, rendered with Linux/SwiftShader.
See [preview controls and saving](docs/api.md#bounded-local-preview),
[display limits](docs/format-support.md#viewer-display-boundary) and
[viewer development](viewer/README.md).

## Supported inputs

Built-in profiles cover exact Onshape V30/V13/current, extracted iCAD and
SolidWorks partition keys. Each has `verified_subset` coverage and requires
zero user fields. Consult the [support matrix](docs/format-support.md#supported-profiles)
for exact keys and geometry limits; matching a key does not guarantee that
all records are supported. Other keys or uncovered types need an explicit
[schema catalog](docs/schema-catalogs.md).

Native CAD containers, assemblies and SolidWorks delta application are outside
the complete parser contract. Parsing, OCCT conversion, STEP export and preview
have separate coverage. See [format support and limitations](docs/format-support.md).

## Documentation

| Topic | Guide |
|---|---|
| Setup and optional runtimes | [Installation](docs/installation.md) |
| Python models, conversion and CLI | [API reference](docs/api.md) |
| External schema providers | [Schema catalogs](docs/schema-catalogs.md) |
| Supported inputs and geometry | [Support matrix](docs/format-support.md) |
| Profile sources and validation | [Built-in profiles](docs/builtin-profiles.md) |
| Release changes | [Changelog](CHANGELOG.md) |
| Maintainer verification | [Release gates](docs/releasing.md), [corpus policy](corpus/README.md) |

## License

The distribution is licensed under `MIT AND Apache-2.0`: the original
implementation uses [MIT](LICENSE), and adopted partial readers use
[Apache-2.0](LICENSES/Apache-2.0.txt). See [reader provenance](crates/parasolid-core/PARTIAL_READERS.md)
and [viewer notices](viewer/README.md#dependencies-and-licenses).
