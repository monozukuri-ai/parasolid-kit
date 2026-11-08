# Viewer development

This directory contains the three-cad-viewer frontend: GLB decoding, protocol-3
geometry, source selection, a diagnostic UI, and a reproducible HTML/JS/CSS build.
It supports orbit/pan/zoom/fit, camera and axes controls, body and face/edge visibility,
surface/diagnostic filters, section views, and source details for multiple selections.
Completeness, target units, partial warnings, and missing mappings remain visible.

`parasolid-kit viewer` and its `view` alias use this application and the same
Python handler. Built assets are shipped under `parasolid_kit/interop/preview/static/`.
The sdist also includes frontend sources, the lockfile, original notices, tests,
and the reviewed public synthetic fixtures. Node modules, browser binaries, build
outputs and test reports are excluded. The archive gate checks an explicit source
allowlist and compares its bytes with the reviewed tree.

## Development

From the repository root, install the Python package into a virtual environment
for the generation/CLI checks (a source build needs Rust 1.88+):

```sh
python -m venv .venv
source .venv/bin/activate
python -m pip install ".[occt]"
```

On Windows use `.venv\Scripts\Activate.ps1` in PowerShell. For frontend checks,
use Node.js 22.18 or later. From this directory (`cd viewer`):

```sh
npm ci
npm run typecheck
npm test
npm run build
npx playwright install chromium
npm run test:browser
npm run test:production
```

To use an existing Chrome installation, run
`VIEWER_CHROME=/usr/bin/google-chrome npm run test:browser` instead.
The browser test checks the generated files against `asset-manifest.json`, then
serves the built application and fixture inputs on loopback. It exercises the actual
UI controls, canvas clicks, filtering, clipping, resize/dispose, and failure screens.
Reports and screenshots go to ignored `test-results/`.
`test:production` additionally requires an OCCT-enabled Python environment
(`VIEWER_PYTHON`, default `../.venv/bin/python`). It launches both CLI names,
checks the newly generated assets through the actual Python HTTP server, clicks
faces/edges, checks source/diagnostic HTML-like text and network/CSP activity,
and verifies SIGINT cleanup on Linux. Only the parser boundary is replaced with
a public synthetic B-Rep; this does not qualify X_T/X_B parsing or other platforms.

`npm run build` emits `dist/index.html`, `dist/viewer.js`, and `dist/viewer.css` and verifies their exact
hashes against the committed manifest and bytes against the Python static assets.
After an intentional source or dependency change, run `npm run build:update`,
review the manifest diff, then `npm run build:sync` to copy the three assets.
Update the reviewed hashes/version/license in `src/parasolid_kit/interop/preview/writer.py` and
the independent `scripts/verify_artifacts.py` gate, then rerun the checks.
The build never rewrites those Python allowlists automatically.
The bundle version is independent of the Python package version.
Node/npm are development tools, with no new Python runtime dependencies.

The small public fixtures are generated from synthetic `BrepModel` objects, with a
separate source oracle extracted before conversion. Regenerate them from the repository
root with `.venv/bin/python viewer/tests/generate_fixtures.py` in an OCCT-enabled
environment. Review both input hashes and any OCCT-dependent tessellation changes.
Normal Node tests do not require Python or OCCT.

## Reproduce the README screenshot

With the environments above installed, run from the repository root:

```sh
.venv/bin/python viewer/tests/launch_cli.py viewer synthetic-two-boxes.x_t \
  --source-unit mm --output viewer/test-results/demo --no-open
```

This helper replaces only parsing with the public two-box `BrepModel` and runs the
real CLI, OCCT conversion, writer and HTTP server. The filename is illustrative;
no X_T file is read. Open the JSON `url` for an interactive demo. To capture the
documented source selection, keep that process running and, in another terminal
from `viewer/`, substitute its URL below:

```sh
node tests/screenshot.mjs http://127.0.0.1:PORT/ ../docs/images/viewer.png
```

The script uses a real canvas click on face 102, compares its source fields with
the independent fixture oracle, checks the served asset hashes and records browser,
renderer, source and image evidence in `test-results/screenshot.json`. It fails on
CSP, network or browser errors. Stop the Python process with Ctrl-C. The preview
directory remains; use a new path or explicitly add `--overwrite` for another run.

## Run the viewer

Install the Python package with its `occt` extra, then run:

```sh
parasolid-kit viewer model.x_t --source-unit mm
# Same handler and JSON contract:
parasolid-kit view model.x_t --source-unit mm --write-only --output preview
```

The default serves the generated five files on an ephemeral loopback port and opens
a browser. Use `--no-open` to print the URL without opening a browser, or `--write-only`
to keep the files without starting a server. Existing output is refused unless
`--overwrite` is explicit. `--allow-partial`, units, limits and JSON fields retain
their existing contracts. Regenerate older previews to include body metadata.

Left-drag orbits, Shift-drag pans, and the wheel zooms. The Pick dropdown selects
faces or edges; successive clicks allow multiple selections. Escape clears all and
Backspace removes the last selection. Body checkboxes and surface/diagnostic filters
combine; Reset filters restores them. View changes that hide geometry (filtering,
visibility, or section position) clear the selection. Camera changes retain it.

Section position uses the declared target unit. Reverse flips the retained side.
Caps are displayed only for complete solid leaves; a section is a mesh display,
not a new B-Rep or an exact section calculation. Measurement, CAD vertex selection,
material/Studio editing, animation, and inferred assembly hierarchies are not exposed.
Source and diagnostic strings are inserted with text DOM APIs.

The page loads the two adjacent preview files automatically. `createPreviewApp`
also accepts already-loaded data for browser embedding; the returned controller owns
its renderer and provides `dispose()`. Callers should use one app per document.
Loading is aborted on page exit; successful apps disconnect their resize observer,
remove listeners and release the viewer/display. A page restored from the browser's
back/forward cache reloads after that disposal.

## Adapter contract

`adaptPreview(arrayBuffer, manifest, optionalLimits)` returns `shapes`, `sourceMap`,
validated `manifest` metadata, and `bufferBytes`. Import it from `src/adapter.ts` in
tests or `dist/viewer.js` in the browser. The built module also starts the application
when the `preview-app` root is present. Pass `shapes` to the exported `Viewer`;
connect `bindSelection(viewer, sourceMap, onChange, onError)` before rendering, and
call its returned detach function before disposing the viewer or replacing the model.
Keep one selection binding per viewer and replace it when the source map changes.

- Only this package's embedded, untransformed GLB subset is accepted. Accessors,
  indices, finite coordinates/normals, primitive identities, counts, and bounds
  are checked before concatenated geometry is allocated. Unsupported transforms,
  compression, external buffers, sparse/interleaved accessors, and aliasing fail.
- Schema-1 manifests must include the additive `bodies: [{id, kind}]` field.
  Regenerate older previews. Source IDs must fit JavaScript safe integers.
- Exact body membership sets form leaves. Shared and unknown membership are not
  assigned to an arbitrary body or duplicated. A leaf is `solid` only when its
  body is a known solid and its face set is complete. Partial, shared, unknown,
  and sheet face sets use `faces`. The grouping is not an assembly hierarchy.
- Vertex positions and normals remain typed arrays. Triangle offsets are adjusted
  on concatenation, face order is retained in `triangles_per_face`, and LINE_STRIP
  endpoints become segment pairs with `segments_per_edge`. There are no CAD
  topology vertices in the input, so `obj_vertices` stays empty. Ambiguous geometry
  kinds use Other. Fixed display colors are not source material claims.
- Coordinates already use `conversion.target_unit`; there is no additional glTF
  meter conversion. Bounds are compared at float32 precision and retained in the
  declared target unit. Normals are preserved from the OCCT preview mesh.
- Effective limits are the minimum of the Python manifest's `InteropLimits`, the
  matching browser defaults, and optional caller limits. Limits cannot be raised
  by the manifest/caller. Both input and expanded output vertex counts are checked.
  `max_output_bytes` also bounds input GLB plus all adapter output typed arrays,
  including duplicated edge endpoints and topology count/type arrays. This is a
  conservative adapter buffer budget, not a bound on JavaScript object overhead,
  JSON parsing, library allocations, or GPU memory. Source reference occurrences
  are bounded by `max_entities`; nothing is silently simplified to meet a limit.
- `sourceMap` exposes read-only primitive records, including every source relation,
  node, byte range, diagnostic code, and face/edge/body ID. These are immutable
  copies, separate from renderer data. Retain the input manifest for the
  UI's global diagnostics and missing-entity details.

`filter.ts` shares the original positions/normals and copies only selected triangle
indices, edge endpoints, and topology counts. Hidden elements keep zero-count slots
in `triangles_per_face` / `segments_per_edge`, so local backend indices never shift.
The pinned viewer registers these slots even when their geometry is empty. Filtered
face sets use `faces` subtype. Each filter change disposes the old scene before
allocating the replacement and preserves the camera; the extra filter arrays are
included in `max_output_bytes` checks. Original source metadata remains unchanged.

`selection.ts` deliberately depends on the **pinned 5.0.6** distributed types:
`cadTools.selectObject.selectedShapes`, `backendId`, `topo`, and `onAfterRender`.
The upstream numeric `selected` notification alone loses body/type identity.
The bridge reports changes in the ordered full-ID selection, clears stale details
and calls `onError` for an unmapped or unsupported selection, and restores the
previous render callback on detach. Only face/edge selection is supported here;
vertex/whole-solid selection and measurement are outside this milestone.
The same module sets the typed `currentFilter` and hides the upstream menu and its
vertex/solid shortcuts. This is a version-specific bridge, not a claim of stable
upstream notification API.

## Dependencies and licenses

`three-cad-viewer@5.0.6`, development dependencies, and the npm lockfile are pinned.
The build consumes the package's prebundled ESM/CSS; it verifies that a second Three.js
runtime has not entered the bundle. The original notices for three-cad-viewer,
Three.js, n8ao, and postprocessing are under `third-party/` and embedded in the JS and CSS
assets. License versions, hashes, and declared/text labels are recorded in
`third-party/manifest.json` and checked against installed packages at build time.
In particular, n8ao 1.10.1 declares ISC in `package.json` but distributes CC0-1.0
text in `LICENSE`. Both are preserved explicitly; the bundle is not labeled solely MIT.

The [upstream data format](https://github.com/bernhard-42/three-cad-viewer/blob/master/Data%20Format.md)
provides context. Compatibility checks use the pinned npm package's declarations,
implementation, and actual browser behavior, rather than assuming master is identical.

The application HTML, Python server and browser tests apply the same CSP, including
`style-src-attr 'unsafe-inline'` for upstream's inline style attributes. It checks
CSP violations, console/page errors, failed responses, and external requests.
Only our source-data boundary forbids HTML insertion; reviewed upstream templates,
SVG namespaces and license URLs are allowed in the bundle. The build checks local
source insertion paths, bundle dependencies and external imports; the browser tests
check observable behavior on hostile metadata.

## Generated previews and distribution checks

From the repository root (with OCCT and the frontend development dependencies installed):

```sh
.venv/bin/python scripts/verify_optional_interop_i6.py
# Or select an existing browser:
.venv/bin/python scripts/verify_optional_interop_i6.py --chrome /usr/bin/google-chrome
```

I6 runs the same Playwright suite against freshly generated files served by the Python
server. It covers 8 positive cases and 7 failure/status cases, including body/face/edge
selection, filters, partial provenance, clipping, resize/disposal and hostile text.
Source expectations come from the public B-Rep before conversion. `--skip-browser`
retains the Python-only gate, with a visibly skipped browser result. Use
`--browser-output` to choose the JSON/screenshot directory.

After building wheel and sdist:

```sh
.venv/bin/python scripts/verify_viewer_install.py \
  --wheel dist/parasolid_kit-0.1.0-cp310-abi3-manylinux_2_34_x86_64.whl \
  --sdist dist/parasolid_kit-0.1.0.tar.gz --python 3.10 \
  --output viewer/test-results/distribution
```

Adjust the wheel filename for your platform. This checks actual archive contents,
rebuilds identical assets with `npm ci` from the extracted sdist, and cold-installs
each Python artifact with the OCCT extra. The runtime subprocess runs outside the
checkout with an empty `PATH`; audit guards deny source/catalog reads, process launches
and non-loopback networking. Node/Playwright are used by the separate browser test
harness. Install/build may download dependencies; preview generation and display use
the installed package and loopback assets only. Logs, asset hashes, oracle checks,
browser reports and screenshots are retained under `--output` on success or failure.

Linux CI is configured to run these checks and retain their reports. The existing Windows/macOS
optional/release matrices continue to check generation and installs; this does not
claim browser/GPU qualification on those operating systems. Local browser evidence
uses synthetic B-Reps and SwiftShader, not real parsed X_T/X_B files or physical GPUs.
See the [validation record](../docs/viewer-validation.md) for the local run results,
fixture and artifact hashes, screenshot provenance, and checks not yet run.
