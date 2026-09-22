# Release verification

Python and Rust releases use the same candidate commit. The only version
definition is `[workspace.package].version` in the root `Cargo.toml`. Maturin
derives Python package metadata from it; `parasolid_kit.__version__` reads the
installed package metadata. Artifact and runtime checks derive their expected
versions from the same Cargo manifest.

## Change the version

With Python 3.11+, Cargo and uv available, run from the repository root:

```bash
python3 scripts/bump_version.py 0.4.0
```

This updates `Cargo.toml`, refreshes `Cargo.lock`, `fuzz/Cargo.lock` and `uv.lock`,
then verifies their consistency. Review and commit those generated changes.
There are no version literals to edit in Python code, tests or verification
scripts. Run `uv sync --locked` afterward to rebuild the development install.

For prereleases, use Cargo syntax: `0.4.0-rc.1` becomes Python `0.4.0rc1` and tag
`v0.4.0rc1`; `0.4.0-dev1` becomes `0.4.0.dev1`. A stable `0.4.0` uses tag `v0.4.0`.
Do not use `uv version` to add a second version definition to `pyproject.toml`.

The command refreshes workspace packages with Cargo's offline cache and uses
`uv lock` without upgrading dependencies. On a fresh checkout, install the
development dependencies first. If a lock refresh fails, fix the reported error
and rerun the same command; `scripts/verify_release.py` rejects stale lockfiles.
It does not create commits, tags or publish packages.

Release verification tools run on Python 3.11+; installed-package checks still
exercise Python 3.10. CI checks the source version and locks before testing the
package. A stable version requires a fresh candidate run and fresh evidence.

## Candidate

1. Push a `release/*` branch. The `release` workflow runs the reusable CI at that
   commit: tests, lint, MSRV, public corpus, packaged core, sanitizer smoke and
   installation profiles. It then builds four wheels (Linux x86-64, Windows
   x86-64, macOS Intel and Apple Silicon), an sdist and a core crate, and compares
   all archives and license bytes together on one host. Manual
   dispatch on the same branch also builds candidates without publishing.
2. Wait for the whole candidate workflow to succeed. Download its five
   `release-*` artifacts into one flat directory. Keep the run ID, attempt and
   commit SHA. Artifact retention is 30 days; expiration requires a new run and
   revalidation. Rerunning a job invalidates an earlier receipt.
3. On the private validation host, cold-install the **downloaded** Linux wheel
   and sdist separately, outside the checkout. Run every required M9.2 case using
   `verify_release_corpus.py` and a probe built from the downloaded crate. Run the
   M9.3 sldkit regressions against that unpacked crate, preserving expected partial
   results. Validate M9.4 robustness evidence against the candidate source.
   Keep input manifests, hashes, reports, logs and isolated runtime locations
   locally. iCAD native-container integration awaits its own parser.
4. Create `release-verification.json` with the format below. Each private report
   must identify the candidate commit, tested artifact hashes and actual results;
   the receipt carries only report hashes. The receipt is an attestation by the
   releasing maintainer, not an independent signature or a proof of arbitrary
   geometry support. Do not upload private reports, filenames or CAD inputs.

For 0.2.0, the private case set also includes the current-key analytic,
ellipse/direct-NURBS and [compound-geometry](onshape-composite.md) campaigns,
with both producer encodings. Use `onshape_parametric` for NURBS and compound
cases, retaining the frozen per-family exceptions and strict-distance failures
in every artifact report. The older M9.2 corpus alone does not exercise the
new profile coverage. Freeze the parser, runtime, oracle, tolerances and recipes
before collecting fresh holdouts; record the freeze hashes and subsequent
immutable Onshape Version. Rerun the same case IDs against each cold install.

```json
{
  "schema_version": 1,
  "repository": "monozukuri-ai/parasolid-kit",
  "source_sha": "<40 lowercase hex characters>",
  "candidate_run_id": 123,
  "candidate_run_attempt": 1,
  "python_version": "0.2.0rc1",
  "rust_version": "0.2.0-rc.1",
  "gates": {
    "private_wheel": {"status": "passed", "report_sha256": "<sha256>"},
    "private_sdist": {"status": "passed", "report_sha256": "<sha256>"},
    "downstream": {"status": "passed", "report_sha256": "<sha256>"},
    "robustness": {"status": "passed", "report_sha256": "<sha256>"}
  },
  "artifacts": {"<each of the six distribution filenames>": "<sha256>"}
}
```

Validate the receipt from the clean candidate checkout:

```bash
python scripts/verify_release.py --tag v0.2.0rc1 \
  --receipt /private/release-verification.json --artifacts /private/candidate
```

## Publication

1. Create the version tag at the verified commit and a **draft** GitHub Release.
   Attach only the sanitized `release-verification.json`. Mark RCs as prereleases.
2. Dispatch `rust-release.yml` from `main` with `release_tag` set to that
   draft and `dry_run=true`. The workflow checks out that tag, independently of
   the workflow revision, so publication fixes do not change candidate artifacts.
   Its manual job needs `contents: write` to read draft releases. It checks the successful candidate run, private
   receipt and all hashes, and requires its repackaged crate to be byte-identical
   before allowing upload. A real upload (`dry_run=false`) additionally requires
   the repository's `CARGO_REGISTRY_TOKEN` secret.
3. After publishing Rust, download the registry crate and verify its checksum.
   Change both sldkit dependency pins and its lockfile to the published exact
   version, remove the candidate path patch and rerun downstream validation in
   an isolated checkout. Record this separately from packaged-crate validation.
4. Publish the GitHub Release. This triggers Python publication through the
   existing `pypi` Trusted Publisher environment. It requires the receipt and
   exact candidate run and verifies the published Rust checksum, then uploads
   the previously tested wheels/sdist without rebuilding them.
5. Download all PyPI distributions and compare their sizes/SHA-256 values with
   the candidate. Cold-install from the registry and check version, native core,
   imports, CLI and schema-free parsing. Record local, CI, registry and downstream
   results separately. An RC does not close the stable-release gate.

If one registry upload fails, record the partial publication and inspect the
registry before retrying. Do not overwrite or blindly re-upload a used version.
Public inputs, package contents and declared support remain bounded by
[format support](format-support.md) and [resource validation](resource-validation.md).

## Recovering a release published without a receipt

`gh release download ... --pattern release-verification.json` reports
`no assets to download` when the release has no assets. Check the release's
assets first; publishing the GitHub Release does not generate its receipt.

The tag, receipt's `source_sha` and candidate run's commit must match exactly.
A different commit with identical files still requires a candidate run at the
tagged commit. Create a `release/*` branch at that commit and complete the
candidate verification above. Keep the published tag fixed, and generate the
receipt from the actual reports and downloaded artifacts.

From the clean tagged checkout, validate and attach the sanitized receipt:

```bash
python scripts/verify_release.py --tag v0.2.0 \
  --receipt /private/release-verification.json --artifacts /private/candidate
gh release upload v0.2.0 /private/release-verification.json
```

Complete Rust publication and verify its registry checksum before retrying the
failed Python publication run. Attaching an asset does not retrigger the
`release: published` event. Inspect both registries for partial publication,
then use `gh run rerun <failed-publication-run-id> --failed`. Rerun the publication
workflow only; rerunning the candidate would invalidate the receipt's attempt.
