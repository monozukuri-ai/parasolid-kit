# parasolid-kit

`parasolid-kit` is an experimental, schema-aware parser for Parasolid X_T and
X_B transmit files. Parsing and geometry mapping run in a safe Rust core, while
Python users work with immutable typed models.

The project is read-focused and currently pre-alpha. It is intended for file
inspection, validation, research, and conversion pipelines where preserving
the transmitted structure matters more than silently approximating unsupported
data.

## Features

- Inspect X_T and X_B headers without a schema catalog.
- Parse complete X_T and X_B node streams with the exact caller-provided schema.
- Reconstruct an unmodified parsed X_B document byte-for-byte.
- Compare X_T and X_B documents after pointer-index remapping.
- Map supported topology, analytic geometry, and NURBS records to a typed B-Rep
  source model.
- Return structured diagnostics and enforce configurable resource limits.
- Use the same functionality from Python or a JSON command-line interface.

See [format support and limitations](docs/format-support.md) before relying on
the parser for production data.

## Installation

Python 3.10 or newer is required. Install a downloaded wheel with `pip`:

```bash
python -m pip install /path/to/parasolid_kit-0.1.0.dev0-cp310-abi3-PLATFORM.whl
```

To install from a source checkout, Rust 1.88 or newer is also required:

```bash
git clone https://github.com/monozukuri-ai/parasolid-kit.git
cd parasolid-kit
python -m pip install .
```

The project is not yet claiming a published PyPI release. Once a release is
available there, the installation command will be `python -m pip install
parasolid-kit`.

## Schema catalogs

Header inspection works without external data. Complete parsing requires the
exact `sch_*.sch_txt` catalog identified by the file's internal schema key.
There is no fallback to a nearby version and no inferred replacement for a
missing catalog.

Siemens schema catalogs are not included in this repository or its packages.
You must provide a catalog from software or an SDK that you are licensed to use
and comply with its storage and redistribution terms. The parser reads the
catalog in place and does not copy it into the package.

## Python example

```python
from pathlib import Path

from parasolid_kit import (
    InMemorySchemaProvider,
    SchemaKey,
    inspect_xb,
    load_schema_catalog,
    map_brep,
    parse_xb,
    write_xb,
)

source = Path("model.x_b")
header = inspect_xb(source)
schema_id = SchemaKey.parse(header.schema_key).provider_schema

catalog = load_schema_catalog(
    Path("/path/to/schema") / f"sch_{schema_id}.sch_txt",
    expected_schema_id=schema_id,
)
provider = InMemorySchemaProvider((catalog,))

document = parse_xb(source, schema_provider=provider)
brep = map_brep(document)

print(header.modeller_version)
print(len(document.nodes), len(brep.bodies), brep.complete)
assert write_xb(document) == document.raw_bytes
```

`map_brep()` can return a valid but incomplete model when a well-formed geometry
record has no typed mapping yet. Inspect `brep.complete` and `brep.diagnostics`
instead of assuming that every parsed record has been interpreted.

## Command line

```bash
# Header inspection does not need a schema catalog.
parasolid-kit inspect model.x_b

# Complete parsing and comparison require the exact catalog in --schema-dir.
parasolid-kit parse model.x_t --schema-dir /path/to/schema
parasolid-kit parse model.x_b --schema-dir /path/to/schema --brep
parasolid-kit compare model.x_t model.x_b --schema-dir /path/to/schema
```

Reports are JSON. Exit status is `0` for success or equivalent documents, `1`
for a valid comparison that found differences, and `2` for input, schema, or
parse errors. `python -m parasolid_kit` provides the same interface.

## Documentation

- [Python API](docs/api.md)
- [Format support and limitations](docs/format-support.md)
- [Corpus provenance and redistribution policy](corpus/README.md)

## Project boundaries

`parasolid-kit` does not parse native CAD containers such as iCAD `.icd`, CAD
assembly structures, occurrence transforms, visibility, or appearance. A
format-specific adapter can extract a bounded X_T/X_B payload and pass it to
this package without making the parser depend on that CAD product.

The package has no runtime dependency on a CAD product, geometry kernel, or
OpenCascade binding. It does not bundle Siemens schema data or proprietary CAD
files.

## License

This project is licensed under the [MIT License](LICENSE).
