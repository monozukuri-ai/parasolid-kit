# Schema catalogs

Header inspection needs no catalog. Default parsing selects an exact-key
built-in profile from the [supported profiles](format-support.md#supported-profiles).
These profiles cover verified subsets with zero user fields and require no
external catalog or CAD installation at runtime or build time. See
[profile provenance](builtin-profiles.md) for sources and validation limits.

For another key or a type outside those subsets, supply an explicit catalog.
That provider is authoritative: a missing exact catalog or an empty provider
fails without switching to a built-in profile. `schema_provider=None` selects
the default built-in path. Nearby versions and the human-readable X_T
common-header `SCH` value are never substitutes.

Siemens catalogs are not included in this repository or its packages. Obtain
the matching catalog from a Parasolid SDK or a Parasolid-based product available
to you. The parser reads it in place without copying it or accessing the network.

## 1. Find the required catalog

Inspecting the header does not require a catalog:

```bash
parasolid-kit inspect model.x_b
```

Read `header.schema_key` in the JSON output. When using an external provider,
the required filename is selected as follows:

| Internal schema key | Required catalog |
|---|---|
| `SCH_3000000_30000` | `sch_30000.sch_txt` |
| `SCH_3000310_30000_13006` | `sch_13006.sch_txt` |

For a two-number key, use the second number. For a three-number embedded-base
key, use the third number. This value is also called the *provider schema*.

## 2. Obtain and locate the catalog

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

## Python

```python
from parasolid_kit import read_brep

parsed = read_brep("model.x_b", schema_dir="/path/to/schema")
print(parsed.summary.to_dict())
```

For lower-level parsing, pass a `DirectorySchemaProvider` as `schema_provider`.
See [schema selection and diagnostics](api.md#inputs-and-schema-selection) for
that API, missing-catalog errors and unsupported-type diagnostics.
