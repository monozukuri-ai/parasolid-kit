# Corpus policy

The distributable corpus contains only Parasolid X_T/X_B files whose origin,
generator, export settings, checksums, and redistribution status are recorded
in `manifest.jsonl` and validated by `manifest.schema.json`.

The distributable corpus is currently empty. Files are added only after their
manifest entry, reproducible generation or acquisition record, checksums, and
redistribution status have been reviewed. A locally captured fixture remains
under `corpus/local/` until that review is complete.

## Scope

- Prefer small, self-generated X_T/X_B pairs with a reproducible construction
  recipe.
- Export STEP from the same immutable model state when it is used as a geometry
  oracle.
- Record exact product/build information and the explicitly selected Parasolid
  target version.
- Keep credentials, private cloud document identifiers, and confidential CAD
  data out of the repository.
- Keep native containers such as iCAD `.icd` outside this corpus. They belong to
  an adapter-specific, local-only corpus unless redistribution is explicitly
  allowed.

## Redistribution states

- `allowed`: reviewed and permitted to ship with this repository.
- `local_only`: may be used by a developer but must not be committed or packed.
- `unknown`: provenance is recorded but redistribution has not been cleared.
- `prohibited`: retained only as an external reference; do not copy it here.

An entry marked `unknown` or `prohibited` must never be included in a wheel,
sdist, source archive, or public test fixture.

## Layout

```text
corpus/
  manifest.schema.json
  manifest.jsonl
  generated/
    <generator>/<lineage>/<parasolid-version>/<file>
  local/       # ignored, local-only inputs
  downloads/   # ignored, unreviewed third-party inputs
```

`manifest.jsonl` is created with the first accepted file. Each non-empty line
must be one independent JSON object conforming to `manifest.schema.json`.
Paths are POSIX-style paths relative to this `corpus/` directory and public
entries must live under `generated/`. Every public entry must be marked
`allowed`, state its license or permission basis, and match the recorded
SHA-256 checksum. Run `uv run python scripts/verify_corpus.py` before building
distribution artifacts; the gate also rejects undeclared files under
`generated/`.
