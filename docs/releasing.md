# Release verification

Python and Rust releases use the same candidate commit. Version `0.1.0rc1` maps
to Rust `0.1.0-rc.1` and tag `v0.1.0rc1`; the stable pair is `0.1.0` / `0.1.0`.
`scripts/verify_release.py` (Python 3.11+) checks the manifests, lockfiles, facade
and tag. Update the fixed expectations in artifact/runtime tests when changing
versions. A stable version requires a fresh candidate run and fresh evidence.

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

```json
{
  "schema_version": 1,
  "repository": "monozukuri-ai/parasolid-kit",
  "source_sha": "<40 lowercase hex characters>",
  "candidate_run_id": 123,
  "candidate_run_attempt": 1,
  "python_version": "0.1.0rc1",
  "rust_version": "0.1.0-rc.1",
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
python scripts/verify_release.py --tag v0.1.0rc1 \
  --receipt /private/release-verification.json --artifacts /private/candidate
```

## Publication

1. Create the version tag at the verified commit and a **draft** GitHub Release.
   Attach only the sanitized `release-verification.json`. Mark RCs as prereleases.
2. Dispatch `rust-release.yml` at the version tag with `release_tag` set to that
   draft and `dry_run=true`. It checks the successful candidate run, private
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
