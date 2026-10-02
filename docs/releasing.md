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
package.

## Publish Python

1. Commit the version change, push it and create its matching version tag.
2. Publish a GitHub Release for that tag. This starts `release.yml`, which runs
   the full CI, builds and tests the distributions, and publishes to PyPI after
   all required jobs succeed. Mark RCs and development versions as prereleases.
3. Check that the workflow completed and that PyPI contains the four wheels and
   source distribution for the selected version.

No `release-verification.json`, separate candidate run or private CAD data is
required. PyPI publication uses the existing `pypi` environment and Trusted
Publisher. Python publication does not depend on a crates.io upload: the wheels
already contain the native Rust core, and the sdist contains its source.

The release workflow always:

- Resolves the selected source once and uses that exact commit in every test
  and build job, including manual publication from a newer workflow revision.
- Runs the reusable CI: Python/Rust tests, lint, MSRV, public corpus, packaged
  core tests, sanitizer smoke, viewer checks and optional installation profiles.
- Builds Linux x86-64, Windows x86-64, macOS Intel and Apple Silicon wheels, a
  source distribution and a Rust crate. It checks archive metadata, license
  bytes, isolated installs and OCCT/CadQuery behavior on each platform.
- Compares all distributions together and records their SHA-256 hashes as job
  outputs. The publish job downloads artifacts from the **same workflow run**,
  checks those hashes again, and uploads the tested wheels and sdist.

A missing platform, failed check or changed artifact blocks publication. The
publish job does not rebuild packages. Artifact retention is 30 days.

## Build without publishing

Push a `release/*` branch, or run `release.yml` manually with `release_tag` left
empty. It runs the same checks and builds the same distributions, with the PyPI
job skipped. This is useful when reviewing a candidate or running additional
private validation; it is optional for normal publication.

## Publish Rust

Rust publication is a separate manual workflow. It resolves the tag to one
commit, runs the reusable CI, packages and tests the distributable crate, and
then performs the selected operation:

```bash
gh workflow run rust-release.yml --ref main -f release_tag=v0.3.5 -f dry_run=true
```

After a successful dry run, publish with:

```bash
gh workflow run rust-release.yml --ref main -f release_tag=v0.3.5 -f dry_run=false
```

A real upload requires the repository's `CARGO_REGISTRY_TOKEN` secret. Neither
a GitHub Release asset nor a private verification receipt is required. Inspect
crates.io before retrying an upload; a published version cannot be replaced.

## Recover a failed Python release

Fix the failure and rerun the failed jobs in the same workflow run. A publish
retry reuses the distributions and hashes from that run. Before retrying a
partially completed upload, inspect PyPI to determine which files were already
published; do not blindly rebuild and upload a used version.

If the tagged workflow itself is obsolete, use the current workflow on `main`
to build, verify and publish the existing tag:

```bash
gh workflow run release.yml --ref main -f release_tag=v0.3.5
```

This also recovers old `no assets to download` failures caused by the former
`release-verification.json` requirement. The tag stays fixed. The current
workflow checks out the selected tag separately, runs CI at its exact commit,
and publishes only the distributions produced and verified by this new run.
Old failed attempts retain their original workflow revision, so rerunning them
will not pick up the fix. Check the new manual run for the recovery result.

## Additional validation for relevant changes

Run private CAD, geometry-oracle, downstream and longer robustness checks when
changes affect their behavior or support claims. They are not required for every
release. Keep the existing `verify_release_corpus.py`, corpus manifests, geometry
checks, benchmarks and fuzz tools for this work.

When testing a release artifact, download it from the candidate workflow and
cold-install the wheel and sdist separately. Build Rust probes from the downloaded
crate and test downstream consumers in isolated checkouts. Reports should record
the tested commit, artifact/input hashes, actual results and known partial cases.
Retain detailed reports and private CAD inputs locally; do not upload them as
public release assets. A local report does not establish arbitrary format or
geometry support.

Public inputs, package contents and declared support remain bounded by
[format support](format-support.md) and [resource validation](resource-validation.md).
