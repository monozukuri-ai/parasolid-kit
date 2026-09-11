# Corpus policy

The distributable corpus contains only Parasolid X_T/X_B files whose origin,
generator, export settings, checksums, and redistribution status are recorded
in `manifest.jsonl` and validated by `manifest.schema.json`.

The public corpus contains three small, project-authored synthetic framing
probes: an X_T/X_B integer-array pair and an unsupported-key input. Their recipe
is `tests/support/release_fixture.py`; they are not CAD-produced geometry or new
holdouts. Real CAD fixtures remain local until their provenance and
redistribution status have been reviewed.

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

## Required release regression

`verify_corpus.py` checks redistribution provenance and permits an empty public
corpus. `verify_release_corpus.py` additionally requires a nonempty, explicit
set of cases. Every manifest entry must have exactly one rule in a sidecar
validated by `release-checks.schema.json`. Missing inputs, baselines, required
oracles, checksums, or measurements fail the release check.

```sh
cargo build -p parasolid-core --example release_corpus --locked
python scripts/verify_release_corpus.py \
  --manifest corpus/manifest.jsonl --checks corpus/release-checks.json \
  --root corpus --rust-probe target/debug/examples/release_corpus
```

The runner starts a fresh `--python` interpreter (the current interpreter by
default), with catalog and Python network access prohibited. It checks the
actual API and CLI exit codes/JSON, full consumption, decoded values and source
ranges against Rust, the compiled profile identity, independent value
reencoding, paired-document equivalence, and the declared B-Rep regression
baseline. Embedded schema blobs used by the value encoder are reported as
replayed; source record payloads are reencoded from decoded values.

Baselines are hashed JSON reports, not independent geometry ground truth. Their
floating values use the existing document-comparison tolerances (absolute and
relative `1e-12`); integer values, indices, names and ranges compare exactly.
The Rust/Python comparison covers every raw value and range plus B-Rep counts,
completeness, diagnostics and topology validation. The full Python source B-Rep
baseline also preserves geometry, ownership and senses.

An optional `onshape_analytic` oracle runs in a separate `--oracle-python`
environment with OCP installed. It requires the declared immutable native state,
native topology/geometry, matching source primitives and points in STEP, and
STEP/native area and volume. Requested core metrics must be available. All
physical units and numeric tolerances are explicit in the sidecar. Additional
STEP curve types, including seam/degenerate edges, are reported separately;
they never substitute for a missing required source-geometry match. Other
geometry families need their own independent evidence before this oracle can
be requested for them.

Known diagnostics have their exact code and offset checked and are counted
separately from parsed inputs. `usage: holdout` records provenance supplied by
the maintainer; the label alone does not prove that an input was unseen.
Freeze the implementation, expectations, tolerances and exclusion list before
collecting a new holdout. A failed holdout used for a fix becomes a regression.

Private manifests, rules, baselines and results belong under `.internal/`.
The runner accepts local-only entries without changing the public provenance
gate's redistribution requirements. All input/reference paths are normalized
relative to `--root`; symlinks, traversal and missing counterparts are rejected.
`--timeout` is a positive time limit in seconds for each runtime/probe process.

X_T/X_B fixture bytes remain excluded from wheel and sdist. The source
distribution includes the runner, schemas and public synthetic baseline
metadata. Pass fixtures separately with `--root`, and select the installed
wheel or sdist interpreter with `--python` to repeat the same case IDs against
distribution artifacts. The Rust example and its independent encoder are also
included in the `parasolid-core` crate.
