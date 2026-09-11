# Fuzz and resource validation

The shared parser and an embedding application enforce different bounds.
`DocumentLimits` bounds strict parsing; `max_diagnostics` bounds retained B-Rep
diagnostics. Partial readers accept already extracted body slices, so callers
must bound input size, decompression and expansion ratios before invoking them.
The defaults are configuration values, not a measured capacity guarantee.

## Sanitizer campaigns

The parse target runs both strict parsing and B-Rep mapping with at most 256
diagnostics, while retaining the existing partial-reader scans. Synthetic seeds
cover all four compiled profiles, valid BODY/REGION/SHELL mapping, analytic and
trimmed geometry, schema Copy/Delete/Insert/Append, unknown membership, array
and node limits, non-finite values, cycles and truncation. The seed tests require
actual parse and B-Rep results; retaining source bytes is not a success criterion.

Install nightly Rust and cargo-fuzz 0.13.2, then run from the repository root:

```bash
python3 scripts/run_fuzz.py parse --seconds 600 --output fuzz/runs/parse-600
```

Targets are `inspect`, `parse` and `schema_catalog`. The runner bounds each input
to 64 KiB, each invocation to 5 seconds and libFuzzer RSS to 1,024 MiB. It records
the command, input seed hashes, exit status, logs and reproducer artifacts. Use
a new output directory for each run. A reproducer artifact makes the run fail
even if the child returns zero. The outer process timeout also terminates child
processes. AddressSanitizer and LeakSanitizer require an environment that permits
their operation; a sandbox/ptrace failure is not a successful sanitizer run.

PR CI retains 100-run smoke tests. Weekly and manual CI runs use 600 seconds per
target; manual runs can select 1,800 or 3,600 seconds. Logs and reproducer inputs
are uploaded on failure as well as success. Native CAD data is not used in these
public jobs.

## Repeatable measurements

The Linux measurement runner uses fresh processes, one warmup per process and at
least five measured samples. It reports median stage time and process peak RSS.
Inputs are read before timing. Rust parse uses a preselected provider; the Python
parse facade includes inspection, provider selection and Python model creation.
B-Rep measurements exclude parsing. RSS includes interpreter/library loading,
the raw document and warmup; it is not an incremental allocation estimate.

```bash
cargo build --release --locked -p parasolid-core --example benchmark
python3 scripts/benchmark_parser.py \
  --manifest /path/to/frozen-manifest.json \
  --rust-probe target/release/examples/benchmark \
  --python /path/to/installed-release-wheel-env/bin/python \
  --output /path/to/new-measurement-directory
```

The manifest contains `cases`, each with `id`, `kind: "strict"`, `encoding`
(`binary` or `text`), `size_class`, `path`, `sha256`, and an `expected` object
containing the known `schema_key` and optionally `nodes` and `bodies` counts.
For example:

```json
{
  "cases": [{
    "id": "small-binary", "kind": "strict", "encoding": "binary",
    "size_class": "small", "path": "/private/model.x_b",
    "sha256": "replace-with-the-input-sha256",
    "expected": {"schema_key": "SCH_3000000_30000", "bodies": 1}
  }]
}
```

An optional `--consumer-probe` measures caller integration for `kind: "partial"`
cases. The integration probe belongs to the consumer and emits JSON containing
`status: "passed"`, `seconds`, and the independently expected `consumer_status`
and `geometry_transferred` values. A partial result is not silently reclassified
as complete geometry. The private sldkit measurements use its existing decoder,
including container inspection, bounded inflation and source/byte accounting.

Each measurement process has a 256 MiB address-space cap, a 10-second CPU cap and
a 20-second wall timeout. These are measurement limits; they do not change the
library defaults. Run measurements after fuzz/build jobs finish. Record the
compiler, host, Python/native module path, source revision/diff, fixture hashes
and release artifacts together.

For a subsequent measurement, `--baseline previous/report.json` flags increases
above 20% in median time or peak RSS as `needs_review`, rather than passing the
gate. The fixture manifest, machine, compiler, limits and sample count must
match. Keep the same Python interpreter and build configuration. Reproduce a
regression under comparable conditions before attributing it to code changes.

## First measured scope (M9.4)

The local Linux release measurements used seven strict inputs across four exact
profiles and three existing sldkit native inputs, with five samples for each of
31 input/stage combinations. The strict inputs were 1,645–49,552 bytes and at most
1,192 nodes. The native inputs were 56,501–158,509 bytes; two retained their
existing partial/no-B-Rep result and one transferred geometry. They are not a
large-file or final-saved-state qualification corpus.

The initial caller bounds used for measurement are:

| Scope | Bounds |
| --- | --- |
| Strict parser | 64 KiB input, 2,048 nodes, 1,024 types, 128 fields/type, 8 KiB strings, 4,096 elements/array, 256 B-Rep diagnostics |
| sldkit integration | 256 KiB native input, 4 MiB total inflation, expansion ratio 200 |

The largest iCAD input correctly exceeded an initial 1,024-node limit; it passed
with the selected 2,048-node measurement bound. Parser limits and caller
file/inflate/ratio failures were checked separately. Broader inputs require
additional measurements before increasing these operational bounds. This first
run establishes a regression baseline and does not assert a 20% improvement or
a universal latency/memory guarantee.
