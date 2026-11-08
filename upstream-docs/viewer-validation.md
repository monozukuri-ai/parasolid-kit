# Viewer validation record

Local implementation evidence, recorded on 2026-09-12. The Python package is
`0.1.0`, the viewer bundle is `2.0.0`, and the pinned three-cad-viewer version is
`5.0.6` (protocol 3). This records the V4 runtime qualification and the V5
documentation screenshot; it is not a published release or a remote CI result.

## Reproduce

Start with the [install and usage example](api.md#bounded-local-preview).
The [viewer development guide](../viewer/README.md) gives exact commands for
frontend builds, browser checks, both CLI names, installed wheel/sdist checks,
and a synthetic demo that needs no CAD input file or schema catalog.

`npm ci`, Python installation and browser installation may download dependencies.
After installation, generation and viewing use packaged assets and loopback HTTP.
Node/Playwright drive the development tests; they are not viewer runtime dependencies.

## Inputs and source oracle

All viewer cases below use public synthetic `BrepModel` objects from
[`tests/_occt_fixtures.py`](../tests/_occt_fixtures.py). The
[generator](../viewer/tests/generate_fixtures.py) extracts source expectations
before OCCT conversion, then creates the checked-in GLB/manifest pairs.
The [oracle](../viewer/tests/fixtures/oracle.json) includes entity IDs, node
IDs/types/indexes and byte ranges. Browser tests compare real canvas selections
against those fields; a rendered mesh alone is not considered source-mapping proof.

| Positive case | Main coverage |
| --- | --- |
| `box` | Face/edge selection, camera, clipping, resize and disposal |
| `cylinder-hole` | Curved faces, hole and edge picking |
| `two-boxes` | Body visibility, multi-selection and colliding local indices |
| `sheet` | Open non-rational NURBS face |
| `box-cm-no-edges` | Target centimeters without rescaling; edge controls disabled |
| `box-partial` | Explicitly removed source mapping, warning and missing list |
| `box-unknown-edges` | Unknown body membership, derived from the box fixture |
| `box-diagnostics` | HTML-like source/diagnostic text rendered as text |

Additional cases cover invalid GLB, inconsistent manifest, malformed JSON, HTTP
404, incomplete source, unavailable WebGL and lost WebGL context. The incomplete
source case succeeds with a warning; the other cases check the expected error UI.
The last two positive cases and corrupted inputs are test transformations of
generated fixtures, rather than additional CAD files.

## V4 local results

| Check | Observed result |
| --- | --- |
| TypeScript / deterministic asset build | Passed |
| Node unit tests | 42 passed |
| Checked-in fixture browser suite | 8 positive cases, 25 source-checked clicks; 7 additional cases passed |
| Fresh OCCT/writer/Python-server browser suite (I6) | Same 8 positive / 7 additional cases passed |
| Real CLI `viewer` and `view` with only parsing replaced | 8 source-checked clicks; both SIGINT exits 0 and servers closed |
| Focused Python CLI, preview and archive tests | 65 passed |
| Actual wheel/sdist archive gate | Passed; all 43 V4 frontend source files matched |
| Frontend rebuilt from extracted sdist | Identical HTML/JS/CSS |
| Cold base installation | Wheel and sdist passed |
| Cold OCCT profile tests | 655 passed, 17 skipped |
| Cold installed wheel and sdist viewer checks | Each passed 8 positive / 7 additional cases and 25 source-checked clicks |

Normal browser cases reported zero CSP violations, external requests, console/page
errors and failed HTTP responses. Cold runtime checks ran outside the checkout,
with an empty `PATH` and audit guards rejecting checkout/catalog reads, subprocess
launches and non-loopback networking. Separate rejection probes checked the guards.
The 17 skips concern Python 3.11+ release tooling, CadQuery-only tests, and tests
specifically requiring an absent optional runtime; they are not counted as passes.

Environment: Linux x64, Node 22.18.0, Playwright 1.56.1, Chromium 141.0.7390.37
(build 1194), WebGL2 through ANGLE/Vulkan SwiftShader Subzero. Cold Python used
3.10.12 with `cadquery-ocp-novtk` / `cadquery-ocp-proxy` 7.9.3.1.1.

## Artifact identities

The [machine-readable evidence summary](viewer-validation.json) records full
SHA-256 values for the V4 wheel/sdist, original local reports, reviewed fixture
files, source oracle, runtime assets and the V5 screenshot. The original logs and
temporary installed environments are not distributed; the summary is a selected
record of their results, and the development commands regenerate full reports.

The V4 qualified artifacts are
`parasolid_kit-0.1.0-cp310-abi3-manylinux_2_34_x86_64.whl` and
`parasolid_kit-0.1.0.tar.gz`. Their recorded hashes identify those exact local
builds. Adding the V5 documentation and screenshot helper changes distribution
contents; it does not retroactively qualify a new archive under the V4 hashes.
The V5 source allowlist contains 44 frontend files. Runtime HTML/JS/CSS are
unchanged, and their bytes are checked against
[`asset-manifest.json`](../viewer/asset-manifest.json) on every frontend build.

Original dependency notices and their hashes are preserved under
[`viewer/third-party/`](../viewer/third-party/manifest.json). The bundle includes
MIT and Zlib notices. n8ao 1.10.1 declares ISC in its package metadata but ships
CC0-1.0 license text; both are retained without resolving that discrepancy.

## V5 screenshot

The [README image](images/viewer.png) is an unedited 1440 × 1000 browser capture
from the actual CLI/OCCT/writer/server with the public two-box source. Only the
parser is replaced. The capture script clicks the second box's top face and
checks `parasolid:face:000102`, source node 1075, and byte range `[10750, 10759)`
against the oracle. It also verifies the three served runtime asset hashes and
requires zero CSP, external-network, browser and HTTP errors.

The screenshot uses the same Chromium/SwiftShader configuration listed above;
display colors are illustrative. Its report and regeneration command are described
in [the capture instructions](../viewer/README.md#reproduce-the-readme-screenshot).
Rendering can vary across platforms; the image hash identifies this capture,
not a cross-platform pixel equality requirement.

## Unrun checks and limits

Remote CI has been configured, but no successful remote run is asserted here.
This viewer campaign did not run Windows/macOS generation or browser checks,
physical-GPU checks, or end-to-end viewer parsing of real X_T/X_B files.
Existing parser/interop support has its own evidence and does not extend this
browser campaign. See the [display boundary](format-support.md#viewer-display-boundary)
for partial output, unit handling, sections and unimplemented capabilities.
Publishing a release remains a separate task.
