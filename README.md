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
- Parse, map, and summarize one file with `read_brep()` or the human-readable
  `check` command.
- Convert the exact optional OCCT subset and export validated AP242 plus a
  provenance sidecar without routing through CadQuery.
- Wrap the same strict conversion as CadQuery `Shape` values for immediate
  inspection and downstream CadQuery operations.
- Write a bounded GLB/source manifest and inspect faces or edges in a bundled,
  offline local WebGL viewer.
- Return structured diagnostics and enforce configurable resource limits.
- Use the same functionality from Python or a command-line interface with
  deterministic JSON output where required.

See [format support and limitations](docs/format-support.md) before relying on
the parser for production data.

## Installation

Python 3.10 or newer is required. Install the current development release from
[PyPI](https://pypi.org/project/parasolid-kit/):

```bash
python -m pip install --pre parasolid-kit
```

The `--pre` option is required while only development releases are available.
To install the current release by exact version instead:

```bash
python -m pip install "parasolid-kit==0.1.0.dev0"
```

Stable releases, once available, can be installed without `--pre`:

```bash
python -m pip install parasolid-kit
```

Alternatively, install a downloaded wheel directly:

```bash
python -m pip install /path/to/parasolid_kit-0.1.0.dev0-cp310-abi3-PLATFORM.whl
```

Header inspection works immediately after installation. Complete parsing,
B-Rep checking, STEP export, CadQuery conversion, and the viewer also require
the exact Parasolid schema catalog named by the input file. See
[Schema catalogs](#schema-catalogs) before trying those commands.

### Optional interoperability profiles

The base install remains parser-only and has no runtime dependency on a
geometry kernel. Two mutually exclusive profiles establish the optional OCCT
boundary:

```bash
# Headless OCP runtime without CadQuery or VTK; Python 3.10+
python -m pip install --pre "parasolid-kit[occt]"

# CadQuery and its full OCP runtime; Python 3.11+
python -m pip install --pre "parasolid-kit[cadquery]"
```

Do not install both profiles in one environment. Their OCP distributions can
provide the same Python import namespace; `parasolid-kit` detects that state
before importing OCP and reports commands for returning to one profile. There
is intentionally no `[all]` extra.

The `[occt]` profile converts the documented exact I7 subset from `BrepModel` into a
validated OCCT shape and can export that result directly as AP242. The
`[cadquery]` profile runs the same strict converter with CadQuery's full OCP
runtime and exposes the result through CadQuery `Shape` objects. Tessellation
and the local viewer use the same OCCT result and are available in either
optional profile; they do not require VTK, a CDN, or Node.js at runtime.

You can confirm that one optional runtime is usable without importing it during
normal parsing:

```python
from parasolid_kit.interop import require_occt

OCP = require_occt()  # imports OCP only after distribution checks pass
```

Convert a parsed result by naming the source length unit explicitly:

```python
from parasolid_kit import read_brep
from parasolid_kit.interop.occt import to_occt, write_step

parsed = read_brep("model.x_t", schema_dir="/path/to/schema")
converted = to_occt(
    parsed.brep,
    source_unit="m",
    target_unit="mm",
)

print(converted.report.occt_valid)
print(converted.report.metrics.to_dict())
print(converted.source_map.to_dict())
shape = converted.shape  # owned TopoDS_Shape runtime object

exported = write_step(
    converted,
    "model.step",
    output_unit="mm",
)
print(exported.report.validation.passed)
print(exported.sidecar_path)  # model.step.conversion.json
```

With the `[cadquery]` profile, the shortest interactive confirmation path is:

```python
from parasolid_kit.interop.cadquery import to_cadquery, to_cadquery_shapes

shape = to_cadquery(parsed.brep, source_unit="m")
print(type(shape).__name__, shape.BoundingBox(), shape.Area())
print(sum(solid.Volume() for solid in shape.Solids()))  # solid-only volume

# One immutable tuple entry per source body, in source order.
body_shapes = to_cadquery_shapes(parsed.brep, source_unit="m")
```

A single source body becomes its most specific CadQuery `Shape` subclass;
multiple bodies become a `cadquery.Compound`. The adapter does not infer a
`cadquery.Assembly`, reconstruct a `Workplane` chain, or recreate editable
feature history. Source mapping belongs to the original conversion result and must not
be treated as valid after a returned CadQuery object is modified.

Generate a persistent, self-contained preview from that same conversion:

```python
from parasolid_kit.interop.preview import PreviewOptions, write_preview

preview = write_preview(
    converted,
    parsed.brep,
    "model.parasolid-preview",
    options=PreviewOptions(linear_deflection=0.1),
)
print(preview.index_path, preview.report.to_dict())
```

The directory contains `index.html`, `viewer.js`, `viewer.css`, `preview.glb`,
and `preview.manifest.json`. Each face/edge primitive retains its
conversion-local key plus Parasolid entity ID, source node ID, byte range,
geometry kind, and diagnostics. The manifest never includes raw source bytes
or a path-like `source_identity`; only an exact `sha256:<digest>` is retained.

![parasolid-kit offline B-Rep viewer showing the model and its Parasolid source mapping](https://raw.githubusercontent.com/monozukuri-ai/parasolid-kit/main/assets/viewer.png)

I7 extends the exact OCCT path with ellipses, parabolas, hyperbolas, explicit
trimmed curves, cone frustums, untrimmed spheres and ring tori, open
non-periodic non-rational 3D NURBS, and exact offset surfaces.
`geometry_coverage()` exposes the parser, OCCT, STEP, and constraint status
without importing OCP. Rational, closed, or periodic NURBS, pcurves,
intersection curves, and blend surfaces remain explicit errors; they are not
approximated from incomplete semantics. Unknown orientation, invalid
references or topology, metric disagreement, and requested healing likewise
stop with `OcctConversionError`, whose partial report retains the diagnostic.
Generated OCCT seam/boundary topology remains explicit in the source map.

`write_step()` accepts only a complete, valid `OcctConversionResult`. It stages
both outputs, cold-reimports the STEP in a separate Python process, and commits
the final paths only after body/face counts, validity, bounding box, area, and
volume pass. Existing output is rejected unless `overwrite=True`. The sidecar
contains versions, units, conversion status, metrics, limits, and the complete
source mapping; a caller-provided path-like source identity is omitted by
default. A `source_identity="sha256:<digest>"` remains safe to retain.

To install from a source checkout, Rust 1.88 or newer is also required:

```bash
git clone https://github.com/monozukuri-ai/parasolid-kit.git
cd parasolid-kit
python -m pip install .
```

## Schema catalogs

Header inspection works without external data. Complete parsing requires the
exact `sch_*.sch_txt` catalog identified by the file's internal schema key.
There is no fallback to a nearby version and no inferred replacement for a
missing catalog.

Siemens schema catalogs are not included in this repository or its packages.
Obtain the catalog from a Parasolid SDK or a Parasolid-based product available
to you, then point `--schema-dir` at the directory that contains it. The parser
reads the catalog in place and does not copy it into the package.

### 1. Find the required catalog

Inspecting the header does not require a catalog:

```bash
parasolid-kit inspect model.x_b
```

Read `header.schema_key` in the JSON output. The required filename is selected
as follows:

| Internal schema key | Required catalog |
|---|---|
| `SCH_3000000_30000` | `sch_30000.sch_txt` |
| `SCH_3000310_30000_13006` | `sch_13006.sch_txt` |

For a two-number key, use the second number. For a three-number embedded-base
key, use the third number. This value is also called the *provider schema*.

### 2. Obtain and locate the catalog

- If you already have a Parasolid SDK or a Parasolid-based CAD product, search
  its installation directory for the exact filename. Product layouts vary;
  schema directories are commonly named `schema`, and an installation used
  during this project's validation placed them under `ETC/schema`.
- If you do not have a suitable installation, request access through the
  [Siemens Parasolid SDK](https://www.siemens.com/en-us/products/plm-components/parasolid/3d-modeling-sdk/),
  the [Siemens 3D SDK trial page](https://www.siemens.com/en-gb/products/plm-components/3d-sdk-software-trials/),
  or [Parasolid Support](https://parasolid-support.industrysoftware.automation.siemens.com/).
  Ask specifically for the numeric schema version reported by `inspect`.
- If the X_T/X_B file came from another CAD system, its vendor or the file
  producer may be able to supply the matching catalog or export to a Parasolid
  version for which you already have one.

For example, search a known product installation directory without scanning
the whole machine:

```bash
# Linux or macOS
find /path/to/product -type f -iname 'sch_13006.sch_txt'
```

```powershell
# Windows PowerShell
Get-ChildItem 'C:\path\to\product' -Recurse -File -Filter 'sch_13006.sch_txt'
```

Pass the containing directory, not the catalog file itself:

```bash
parasolid-kit check model.x_b --schema-dir /path/to/product/ETC/schema
```

`DirectorySchemaProvider` loads only the exact
`sch_<provider-schema>.sch_txt` filename and verifies that the identifier inside
the catalog matches. A similarly numbered catalog is not substituted.

## Python example

```python
from pathlib import Path

from parasolid_kit import read_brep

source = Path("model.x_b")
parsed = read_brep(
    source,
    schema_dir="/path/to/schema",
)

print(parsed.summary.to_dict())
print(len(parsed.document.nodes), len(parsed.brep.bodies), parsed.complete)
```

`read_brep()` selects X_T/X_B from a known suffix or signature, loads only the
exact `sch_<provider-schema>.sch_txt` catalog, parses the document, maps the
B-Rep, and returns all three views as `ParsedBrep`. It never chooses a nearby
schema catalog.

The lower-level `inspect_xb()`, `parse_xb()`, `map_brep()`, and `write_xb()`
APIs remain available when each stage must be controlled independently.
`map_brep()` can return a valid but incomplete model when a well-formed geometry
record has no typed mapping yet. Inspect `parsed.complete` and
`parsed.brep.diagnostics` instead of assuming that every parsed record has been
interpreted.

## Command line

```bash
# Header inspection does not need a schema catalog.
parasolid-kit inspect model.x_b

# Parse, map, and print a compact human-readable report.
parasolid-kit check model.x_b --schema-dir /path/to/schema

# Use stable JSON when the result is consumed by another program.
parasolid-kit check model.x_t --schema-dir /path/to/schema --json

# Complete parsing and comparison require the exact catalog in --schema-dir.
parasolid-kit parse model.x_t --schema-dir /path/to/schema
parasolid-kit parse model.x_b --schema-dir /path/to/schema --brep
parasolid-kit compare model.x_t model.x_b --schema-dir /path/to/schema

# Requires one optional profile; source units are explicit and output is
# cold-reimported before model.step becomes visible.
parasolid-kit export-step model.x_t model.step \
  --schema-dir /path/to/schema \
  --source-unit m

# Generate the same bounded artifacts, bind an ephemeral localhost port, and
# open the bundled offline viewer. Use --no-open for remote/CI shells.
parasolid-kit view model.x_t \
  --schema-dir /path/to/schema \
  --source-unit m
```

`check` is human-readable by default and accepts `--json`; the existing
`inspect`, `parse`, `compare`, `export-step`, and `view` reports remain JSON.
`view` writes `<input-stem>.parasolid-preview` before serving only its five
fixed resources from `127.0.0.1` and an ephemeral port. `--write-only` retains
the artifacts without a server, `--overwrite` replaces an existing output,
and non-loopback `--host` values require the separate `--allow-external`
acknowledgement. Partial display is opt-in with `--allow-partial` and always
shows a warning plus the missing-entity list. Exit
status is `0` for a complete/exported result or equivalent documents, `1` for
an incomplete B-Rep mapping or a valid comparison that found differences, and
`2` for input, schema, parse, conversion, or export errors.
`python -m parasolid_kit` provides the same interface.

## Documentation

- [Python API](docs/api.md)
- [Format support and limitations](docs/format-support.md)
- [Corpus provenance and redistribution policy](corpus/README.md)

## Project boundaries

`parasolid-kit` does not parse native CAD containers such as iCAD `.icd`, CAD
assembly structures, occurrence transforms, visibility, or appearance. A
format-specific adapter can extract a bounded X_T/X_B payload and pass it to
this package without making the parser depend on that CAD product.

The base package has no runtime dependency on a CAD product, geometry kernel,
or OpenCascade binding. Optional profiles add an explicitly selected OCP or
CadQuery runtime without making it part of the parser core. The package does
not bundle Siemens schema data or proprietary CAD files.

## License

This project is licensed under the [MIT License](LICENSE).
