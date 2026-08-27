# Python API

`parasolid_kit` is the supported Python namespace. `parasolid_kit._core` is a
private implementation module and must not be imported by applications. Public
functions validate their arguments and return immutable typed values.

The package is currently pre-alpha. The documented names and behaviors are the
intended public boundary, but backward compatibility is not yet guaranteed
between development releases.

## Inputs and schema selection

`inspect_xb`, `inspect_xt`, `parse_xb`, and `parse_xt` accept a path or a
`bytes`, `bytearray`, or `memoryview` value. They do not accept a file-like
object. Inspection validates only the header and must not be treated as proof
that the node stream or geometry is valid.

Complete parsing uses the schema key inside the X_B/X_T stream. For
`SCH_<modeller>_<effective>` the provider must supply `<effective>`; for
`SCH_<modeller>_<effective>_<base>` it must supply `<base>`. The parser does not
fall back to a nearby version, infer an unknown field layout, or select the
human-oriented common-header `SCH` value in X_T.

```python
from parasolid_kit import InMemorySchemaProvider, load_schema_catalog, parse_xb

catalog = load_schema_catalog("/caller/owned/schema/sch_30000.sch_txt")
provider = InMemorySchemaProvider((catalog,))
document = parse_xb("model.x_b", schema_provider=provider)
```

The repository and package contain no Siemens schema catalog. The caller owns
catalog acquisition, storage, access control, and redistribution decisions.

## Entry points

| Function | Result | Purpose |
|---|---|---|
| `inspect_xb(source, *, limits=...)` | `XbHeader` | Validate and inspect an X_B header |
| `inspect_xt(source, *, limits=...)` | `XtHeader` | Validate and inspect an X_T header |
| `load_schema_catalog(path, *, expected_schema_id=None, limits=...)` | `SchemaCatalog` | Load and validate one exact ASCII `sch_*.sch_txt` catalog |
| `parse_xb(source, *, schema_provider=None, limits=...)` | `ParasolidDocument` | Parse a complete X_B node stream |
| `parse_xt(source, *, schema_provider=None, limits=...)` | `ParasolidDocument` | Parse a complete X_T node stream |
| `write_xb(document)` | `bytes` | Byte-exact reconstruction of an unmodified X_B document |
| `compare_documents(left, right, *, absolute_tolerance=..., relative_tolerance=..., max_differences=...)` | `DocumentComparison` | Structural comparison after pointer-index remapping |
| `map_brep(document, *, limits=...)` | `BrepModel` | Validated Parasolid-native topology and geometry model |

`ParasolidDocument.format` is `"binary"` or `"text"`. Its `nodes` remain in
physical source order and retain node indices, effective definitions, decoded
field values, exact byte ranges, schema-resolution provenance, terminator, raw
bytes, diagnostics, and schema-coverage data. Public model objects provide
`to_dict()` where a JSON-compatible report is required.

`write_xb` rejects an X_T-derived document. It is a reconstruction API, not a
general editor or serializer for a mutated object graph.

`compare_documents` does not require equal bytes, node order, or physical node
indices. `equivalent` is true only when schema coverage, node-type counts,
topology, and field values agree under the supplied finite non-negative numeric
tolerances. Differences are structured and capped by `max_differences`.

`map_brep` preserves source topology and analytic/NURBS definitions. A valid raw
record whose geometry kind is not decoded remains explicit as
`UnsupportedGeometry`, and `BrepModel.complete` is false. Metrics are returned
only when their exact preconditions are met; curved shapes are not estimated
from endpoints. See [format support and limitations](format-support.md) for the
current geometry coverage.

## Limits and failures

`ParseLimits` bounds file size, node count, schema type count, fields per type,
string bytes, variable elements, and retained B-Rep diagnostics. The same
relevant limits are enforced again inside Rust before allocation or traversal.

Package-defined failures derive from `ParasolidError`:

- `ParseError` carries a structured `.diagnostic` with stable code, severity,
  location, fatal flag, and details.
- `SchemaError` reports unavailable or invalid schema resolution.
- `LimitExceededError` reports the resource, actual value, and configured
  bound.

Ordinary Python contract errors use `TypeError` or `ValueError`.

## CLI

The installed `parasolid-kit` command and `python -m parasolid_kit` expose the
same interface:

```text
parasolid-kit inspect MODEL.x_b
parasolid-kit parse MODEL.x_t --schema-dir /caller/owned/schema
parasolid-kit parse MODEL.x_b --schema-dir /caller/owned/schema --brep
parasolid-kit compare LEFT.x_t RIGHT.x_b --schema-dir /caller/owned/schema
```

Successful reports are JSON on stdout. Errors are JSON on stderr. Exit status
is `0` for success or equivalent documents, `1` for a valid comparison that is
different, and `2` for input, schema, or parse errors. Auto-detection accepts
only known suffixes or signatures; ambiguous files require `--format x-b` or
`--format x-t`. Complete parsing loads only the exact
`sch_<provider-schema>.sch_txt` path and never guesses a fallback catalog.

## Non-goals

This package does not parse iCAD `.icd` containers, product structures,
occurrence transforms, visibility, or appearance. A separate iCAD adapter may
extract a bounded Parasolid resource and call this API; the dependency remains
one-way from that adapter to `parasolid-kit`.
