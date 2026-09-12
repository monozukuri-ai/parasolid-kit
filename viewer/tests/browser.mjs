import assert from "node:assert/strict";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { once } from "node:events";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { chromium } from "playwright";

const configIndex = process.argv.indexOf("--config");
const supplied = configIndex < 0 ? null : JSON.parse(await readFile(resolve(process.argv[configIndex + 1]), "utf8"));
const reportIndex = process.argv.indexOf("--report-dir");
const reportDir = reportIndex < 0 ? fileURLToPath(new URL("../test-results", import.meta.url)) : resolve(process.argv[reportIndex + 1]);
process.chdir(fileURLToPath(new URL("..", import.meta.url)));
const models = ["box", "cylinder-hole", "two-boxes", "sheet", "box-cm-no-edges", "box-unknown-edges", "box-partial", "box-diagnostics"];
const oracle = supplied?.oracle || JSON.parse(await readFile("tests/fixtures/oracle.json", "utf8"));
for (const name of ["box-unknown-edges", "box-diagnostics"]) oracle[name] = oracle.box;
const assets = supplied?.assets || JSON.parse(await readFile("asset-manifest.json", "utf8"));
for (const [name, expected] of supplied ? [] : Object.entries(assets.assets)) {
  assert.equal(createHash("sha256").update(await readFile(`dist/${name}`)).digest("hex"), expected.sha256);
}
const csp = "default-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; script-src 'self'; style-src 'self'; style-src-attr 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const server = supplied ? null : createServer(async (req, res) => {
  try {
    const [, model, requested = ""] = new URL(req.url, "http://localhost").pathname.split("/");
    if (!models.includes(model) && !["bad-glb", "bad-manifest", "bad-json", "missing", "incomplete"].includes(model)) { res.writeHead(404); res.end(); return; }
    const name = requested || "index.html";
    let data, type;
    if (Object.hasOwn(assets.assets, name)) {
      data = await readFile(`dist/${name}`); type = name.endsWith("html") ? "text/html" : name.endsWith("css") ? "text/css" : "text/javascript";
    } else if (["preview.glb", "preview.manifest.json"].includes(name)) {
      if (model === "missing") { res.writeHead(404); res.end(); return; }
      const base = ["box-unknown-edges", "box-diagnostics", "bad-glb", "bad-manifest", "bad-json", "incomplete"].includes(model) ? "box" : model;
      data = await readFile(`tests/fixtures/${base}/${name}`);
      type = name.endsWith("json") ? "application/json" : "model/gltf-binary";
      if (name.endsWith("json")) {
        const m = JSON.parse(data);
        if (model === "box-unknown-edges") for (const p of m.primitives) if (p.kind === "edge") p.body_ids = [];
        if (model === "box-diagnostics") {
          m.primitives[1].diagnostic_codes = ["fixture.top"];
          m.primitives[1].source_entities[0].relation_note = '<img src="https://example.invalid/source" onerror="window.injected=true">';
          m.diagnostics = [{ code: "fixture.top", severity: "warning", message: '<img src="https://example.invalid/diagnostic" onerror="window.injected=true">' }];
        }
        if (model === "bad-manifest") m.primitives[0].pick_id = 99;
        if (model === "incomplete") { m.source.complete = false; m.preview.partial = true; m.preview.options.allow_partial = true; }
        data = model === "bad-json" ? "{invalid JSON" : JSON.stringify(m);
      } else if (model === "bad-glb") data = data.subarray(0, 20);
    } else { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { "Content-Type": type, "Content-Security-Policy": csp, "X-Content-Type-Options": "nosniff" }); res.end(data);
  } catch { res.destroy(); }
});
const report = { status: "running", input: supplied?.input || "checked-in synthetic fixtures", node: process.version, platform: process.platform, architecture: process.arch, assets, csp, cases: {} };
let browser, activePage;
await mkdir(reportDir, { recursive: true });
async function clear(page) {
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => testViewer.selection.length === 0, null, { timeout: 4000 });
  assert.equal(await page.locator("#selection-title").innerText(), "Nothing selected");
}
async function project(page, world) {
  return page.evaluate(world => {
    const camera = testViewer.viewer.camera.getCamera(), v = camera.position.clone().set(...world).project(camera);
    const rect = testViewer.viewer.renderer.domElement.getBoundingClientRect();
    return { x: rect.x + (v.x + 1) * rect.width / 2, y: rect.y + (1 - v.y) * rect.height / 2 };
  }, world);
}
async function clickWorld(page, world) {
  const p = await project(page, world); await page.mouse.move(p.x, p.y); await page.waitForTimeout(180); await page.mouse.click(p.x, p.y);
  await page.waitForTimeout(200);
}
async function clickSource(page, model, world, kind, entityId) {
  const key = `parasolid:${kind}:${String(entityId).padStart(6, "0")}`;
  const expected = oracle[model].sources[key]; assert.ok(expected);
  await page.locator("#pick-kind").selectOption(kind);
  const point = await project(page, world);
  await page.mouse.move(point.x, point.y);
  await page.waitForTimeout(180); // Let the normal hover/render loop pick before mouseup.
  await page.mouse.click(point.x, point.y);
  await page.waitForFunction(key => testViewer.selection.some(s => s.primitive.source_entities.some(v => v.key === key)), key, { timeout: 4000 });
  const selected = await page.evaluate(key => testViewer.selection.find(s => s.primitive.source_entities.some(v => v.key === key)), key);
  assert.equal(selected.primitive.kind, kind);
  const source = selected.primitive.source_entities.find(s => s.key === key);
  for (const [field, value] of Object.entries(expected)) assert.deepEqual(source[field], value, `${key}.${field}`);
  assert.ok((await page.locator("#selection-details").innerText()).includes(key));
  const result = { world, pixel: point, expectedSource: expected, selected, realPointerClick: true };
  report.cases[model].interactions.push(result);
  return result;
}
try {
  if (server) { server.listen(0, "127.0.0.1"); await once(server, "listening"); }
  const caseUrl = model => supplied ? supplied.urls[model] : `http://127.0.0.1:${server.address().port}/${model}/`;
  browser = await chromium.launch({ ...(process.env.VIEWER_CHROME ? { executablePath: process.env.VIEWER_CHROME } : {}), headless: true,
    args: ["--no-sandbox", "--disable-dev-shm-usage", "--enable-unsafe-swiftshader", "--use-angle=swiftshader-webgl"] });
  report.browser = browser.version();
  for (const model of process.argv.includes("--errors-only") ? [] : models) {
    const origin = new URL(caseUrl(model)).origin;
    const page = await browser.newPage({ viewport: { width: 1440, height: 850 }, deviceScaleFactor: 1 }); activePage = page;
    const result = { interactions: [], consoleErrors: [], pageErrors: [], externalRequests: [], badResponses: [] };
    report.cases[model] = result;
    page.on("console", m => { if (m.type() === "error") result.consoleErrors.push(m.text()); });
    page.on("pageerror", e => result.pageErrors.push(String(e)));
    page.on("response", r => { if (!r.ok()) result.badResponses.push([r.status(), r.url()]); });
    await page.route("**/*", async route => {
      if (new URL(route.request().url()).origin !== origin) { result.externalRequests.push(route.request().url()); await route.abort(); }
      else await route.continue();
    });
    await page.addInitScript(() => {
      window.cspViolations = [];
      document.addEventListener("securitypolicyviolation", e => cspViolations.push([e.effectiveDirective, e.blockedURI]));
    });
    const response = await page.goto(caseUrl(model));
    assert.equal(response.headers()["content-security-policy"], csp);
    if (supplied) for (const [name, expected] of Object.entries(assets.assets)) {
      const asset = await page.request.get(new URL(name, caseUrl(model)).href);
      assert.equal(asset.status(), 200);
      assert.equal(createHash("sha256").update(await asset.body()).digest("hex"), expected.sha256, name);
    }
    await page.waitForFunction(() => document.getElementById("preview-app").dataset.status !== "loading", null, { timeout: 20000 });
    assert.equal(await page.locator("#preview-app").getAttribute("data-status"), "ready", await page.locator("#preview-error").textContent());
    await page.evaluate(async () => { window.testViewer = await (await import("./viewer.js")).application; });
    if (model === "box") {
      await clickSource(page, model, [20, 15, 20], "face", 2); await clear(page);
      const face = await clickSource(page, model, [40, 15, 10], "face", 6);
      const edge = await clickSource(page, model, [40, 15, 20], "edge", 6);
      assert.equal(face.selected.backendId.split("_").at(-1), edge.selected.backendId.split("_").at(-1));
      assert.notEqual(face.selected.backendId, edge.selected.backendId);
      assert.equal(await page.evaluate(() => testViewer.selection.length), 2);
      await page.keyboard.press("Backspace");
      await page.waitForFunction(() => testViewer.selection.length === 1, null, { timeout: 4000 });
      assert.equal(await page.evaluate(() => testViewer.selection[0].primitive.kind), "face");
      await clear(page);
    } else if (model === "cylinder-hole") {
      await clickSource(page, model, [7, 0, 30], "face", 3); await clear(page);
      await clickSource(page, model, [Math.SQRT1_2 * 10, Math.SQRT1_2 * 10, 30], "edge", 2); await clear(page);
    } else if (model === "two-boxes") {
      const a = await clickSource(page, model, [20, 15, 20], "face", 2);
      const b = await clickSource(page, model, [70, 15, 20], "face", 102);
      assert.equal(a.selected.backendId.split("/faces/")[1], b.selected.backendId.split("/faces/")[1]);
      assert.notDeepEqual(a.selected.primitive.body_ids, b.selected.primitive.body_ids);
      assert.equal(await page.evaluate(() => testViewer.selection.length), 2); await clear(page);
      await clickSource(page, model, [40, 15, 20], "edge", 6);
      await clickSource(page, model, [90, 15, 20], "edge", 106); await clear(page);
    } else if (model === "box-unknown-edges") {
      const edge = await clickSource(page, model, [40, 15, 20], "edge", 6);
      assert.ok(edge.selected.backendId.startsWith("/model/body_unknown/edges/"));
      await clear(page);
    } else if (model === "box-partial" || model === "box-diagnostics") {
      await clickSource(page, model, [20, 15, 20], "face", 2); await clear(page);
    } else {
      await clickSource(page, model, model === "sheet" ? [5, 5, 0] : [2, 1.5, 2], "face", model === "sheet" ? 1 : 2);
      await clear(page);
    }
    // Exercise the shipped application controls, not a parallel test UI.
    if (model === "box") {
      await page.locator("#show-axes").uncheck(); assert.equal(await page.evaluate(() => testViewer.viewer.getAxes()), false);
      await page.locator("#orthographic").uncheck(); assert.equal(await page.evaluate(() => testViewer.viewer.getOrtho()), false);
      await page.locator("#orthographic").check(); await page.locator("#show-axes").check();
      const canvas = page.locator("#cad canvas"); const rect = await canvas.boundingBox();
      const oldCamera = await page.evaluate(() => testViewer.viewer.getCameraPosition());
      await page.mouse.move(rect.x + rect.width / 2, rect.y + rect.height / 2);
      await page.mouse.down(); await page.mouse.move(rect.x + rect.width / 2 + 70, rect.y + rect.height / 2 + 25, { steps: 8 }); await page.mouse.up();
      assert.notDeepEqual(await page.evaluate(() => testViewer.viewer.getCameraPosition()), oldCamera);
      const oldTarget = await page.evaluate(() => testViewer.viewer.getCameraTarget());
      await page.keyboard.down("Shift"); await page.mouse.down(); await page.mouse.move(rect.x + rect.width / 2 + 105, rect.y + rect.height / 2 + 40, { steps: 8 }); await page.mouse.up(); await page.keyboard.up("Shift");
      assert.notDeepEqual(await page.evaluate(() => testViewer.viewer.getCameraTarget()), oldTarget);
      const oldZoom = await page.evaluate(() => testViewer.viewer.getCameraZoom()); await page.mouse.wheel(0, -250);
      await page.waitForFunction(old => testViewer.viewer.getCameraZoom() !== old, oldZoom);
      await page.locator("#fit-view").click();
      await page.locator("#camera-view").selectOption("top");
      const face = await clickSource(page, model, [20, 15, 20], "face", 2);
      await page.locator("#clip-axis").selectOption("2");
      assert.equal(await page.locator("#selection-title").innerText(), "Nothing selected");
      await clickWorld(page, [20, 15, 20]);
      assert.ok(await page.evaluate(() => testViewer.selection.every(s => !s.primitive.parasolid_face_ids.includes(2))));
      await page.screenshot({ path: "test-results/box-clipped.png" });
      await page.locator("#clip-reverse").check();
      await clickSource(page, model, [20, 15, 20], "face", 2); await clear(page);
      await page.locator("#clip-reverse").uncheck();
      await page.locator("#clip-axis").selectOption("off");
      const restored = await clickSource(page, model, [20, 15, 20], "face", 2); assert.equal(restored.selected.backendId, face.selected.backendId); await clear(page);
      await page.locator("#show-faces").uncheck(); await clickWorld(page, [20, 15, 20]);
      assert.equal(await page.evaluate(() => testViewer.selection.length), 0);
      await page.locator("#show-faces").check(); await page.locator("#show-edges").uncheck();
      await page.locator("#pick-kind").selectOption("edge"); await clickWorld(page, [40, 15, 20]);
      assert.equal(await page.evaluate(() => testViewer.selection.length), 0); await page.locator("#show-edges").check();
      await page.setViewportSize({ width: 800, height: 900 });
      await page.waitForFunction(() => Math.abs(document.querySelector("#cad canvas").getBoundingClientRect().width - document.getElementById("cad-container").getBoundingClientRect().width) < 4);
      await clickSource(page, model, [20, 15, 20], "face", 2); await clear(page);
      await page.screenshot({ path: "test-results/box-narrow.png" }); await page.setViewportSize({ width: 1440, height: 850 });
      result.cameraControls = result.clipping = result.visibility = result.resize = true;
    } else if (model === "two-boxes") {
      const first = await clickSource(page, model, [20, 15, 20], "face", 2);
      await page.locator('[data-group-id="/model/body_1"]').uncheck();
      assert.equal(await page.evaluate(() => testViewer.selection.length), 0);
      await clickWorld(page, [20, 15, 20]); assert.equal(await page.evaluate(() => testViewer.selection.length), 0);
      const second = await clickSource(page, model, [70, 15, 20], "face", 102);
      assert.equal(second.selected.backendId, "/model/body_101/faces/faces_1"); await clear(page);
      await page.locator('[data-group-id="/model/body_1"]').check();
      const restored = await clickSource(page, model, [20, 15, 20], "face", 2);
      assert.equal(restored.selected.backendId, first.selected.backendId); await clear(page); result.bodyVisibility = true;
    } else if (model === "cylinder-hole") {
      await page.locator("#surface-filter").selectOption("plane");
      const selected = await clickSource(page, model, [7, 0, 30], "face", 3);
      assert.equal(selected.selected.backendId, "/model/body_1/faces/faces_2"); await clear(page);
      assert.ok(await page.evaluate(() => [...testViewer.visible].every(id => testViewer.preview.sourceMap.get(id).surface_kinds.includes("plane"))));
      await page.locator("#reset-filters").click(); result.surfaceFilter = true;
    } else if (model === "box-partial") {
      assert.equal(await page.locator("#partial-banner").isVisible(), true);
      assert.match(await page.locator("#missing-list").innerText(), /parasolid:face:000001/);
      assert.equal(await page.locator("#source-status").innerText(), "Source complete");
      assert.ok(await page.evaluate(() => testViewer.viewer.shapes.parts.every(p => p.subtype === "faces"))); result.partial = true;
    } else if (model === "box-cm-no-edges") {
      assert.equal(await page.locator("#show-edges").isDisabled(), true);
      assert.equal(await page.locator('#pick-kind option[value="edge"]').evaluate(option => option.disabled), true);
      assert.equal(await page.locator("#unit-status").innerText(), "Unit cm");
      await page.locator("#clip-axis").selectOption("2"); assert.match(await page.locator("#clip-value").innerText(), /^1\.0+ cm$/);
      await page.locator("#clip-axis").selectOption("off"); result.noEdgesAndUnits = true;
    } else if (model === "box-diagnostics") {
      const before = await clickSource(page, model, [20, 15, 20], "face", 2);
      const camera = await page.evaluate(() => testViewer.viewer.getCameraPosition());
      await page.locator("#diagnostic-filter").selectOption("fixture.top");
      const newCamera = await page.evaluate(() => testViewer.viewer.getCameraPosition());
      assert.ok(newCamera.every((v, i) => Math.abs(v - camera[i]) < 1e-7));
      const filtered = await clickSource(page, model, [20, 15, 20], "face", 2);
      assert.equal(before.selected.backendId, filtered.selected.backendId);
      assert.equal(await page.locator("#visible-count").innerText(), "1 faces · 0 edges");
      assert.match(await page.locator("#selection-details").innerText(), /<img src=/);
      assert.equal(await page.locator("#selection-details img").count(), 0); await clear(page);
      await clickWorld(page, [40, 15, 10]); assert.equal(await page.evaluate(() => testViewer.selection.length), 0);
      await page.locator("#diagnostics-panel summary").click();
      assert.match(await page.locator("#diagnostic-list").innerText(), /<img src=/); assert.equal(await page.locator("#diagnostic-list img").count(), 0);
      assert.equal(await page.evaluate(() => window.injected), undefined);
      await page.locator('[data-group-id="/model/body_1"]').uncheck(); assert.equal(await page.locator("#empty-view").isVisible(), true);
      await clickWorld(page, [20, 15, 20]); assert.equal(await page.evaluate(() => testViewer.selection.length), 0);
      await page.locator("#reset-filters").click();
      const restored = await clickSource(page, model, [20, 15, 20], "face", 2); assert.equal(restored.selected.backendId, before.selected.backendId);
      await clear(page); result.diagnosticFilter = result.emptyView = result.untrustedText = true;
    }
    await page.screenshot({ path: `${reportDir}/${model}.png` });
    result.bufferBytes = await page.evaluate(() => testViewer.preview.bufferBytes);
    result.webgl = await page.evaluate(() => {
      const gl = testViewer.viewer.renderer.getContext(), debug = gl.getExtension("WEBGL_debug_renderer_info");
      return { version: gl.getParameter(gl.VERSION), renderer: debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : null };
    });
    result.cspViolations = await page.evaluate(() => cspViolations);
    assert.equal(await page.locator("#preview-app").getAttribute("data-status"), "ready");
    for (const field of ["cspViolations", "consoleErrors", "pageErrors", "externalRequests", "badResponses"]) assert.deepEqual(result[field], [], field);
    result.status = "passed";
    await page.evaluate(() => { testViewer.dispose(); window.dispatchEvent(new Event("resize")); });
    await page.waitForTimeout(150);
    assert.equal(await page.locator("#cad canvas").count(), 0); assert.deepEqual(result.pageErrors, []);
    result.disposed = true; await page.close(); activePage = null;
    console.log(`${model}: passed (${result.interactions.length} source-checked pointer clicks)`);
  }
  report.negativeCases = {};
  for (const model of ["bad-glb", "bad-manifest", "bad-json", "missing", "incomplete"]) {
    const page = await browser.newPage(); activePage = page;
    await page.goto(caseUrl(model));
    const expected = model === "incomplete" ? "ready" : "error";
    await page.waitForFunction(status => document.getElementById("preview-app").dataset.status === status, expected);
    if (model === "incomplete") {
      assert.equal(await page.locator("#source-status").innerText(), "Source incomplete"); assert.equal(await page.locator("#partial-banner").isVisible(), true);
    } else { assert.equal(await page.locator("#preview-error").isVisible(), true); assert.equal(await page.locator("#fit-view").isDisabled(), true); }
    report.negativeCases[model] = { status: "passed", message: await page.locator(model === "incomplete" ? "#missing-list" : "#preview-error").innerText() };
    await page.close(); activePage = null;
  }
  const unavailable = await chromium.launch({ ...(process.env.VIEWER_CHROME ? { executablePath: process.env.VIEWER_CHROME } : {}), headless: true, args: ["--no-sandbox", "--disable-webgl"] });
  try {
    const page = await unavailable.newPage();
    await page.goto(caseUrl("box"));
    await page.waitForFunction(() => document.getElementById("preview-app").dataset.status === "error");
    assert.match(await page.locator("#preview-error").innerText(), /WebGL2 could not start/);
    report.negativeCases.webglUnavailable = { status: "passed", method: "Chrome --disable-webgl" };
  } finally { await unavailable.close(); }
  const lostPage = await browser.newPage(); activePage = lostPage;
  const lostErrors = []; lostPage.on("pageerror", e => lostErrors.push(String(e)));
  await lostPage.goto(caseUrl("box"));
  await lostPage.waitForFunction(() => document.getElementById("preview-app").dataset.status === "ready");
  await lostPage.evaluate(async () => {
    const app = await (await import("./viewer.js")).application;
    app.viewer.renderer.getContext().getExtension("WEBGL_lose_context").loseContext();
  });
  await lostPage.waitForFunction(() => document.getElementById("preview-app").dataset.status === "error");
  assert.match(await lostPage.locator("#preview-error").innerText(), /WebGL context was lost/);
  await lostPage.waitForTimeout(150); assert.deepEqual(lostErrors, []);
  report.negativeCases.webglContextLost = { status: "passed", method: "WEBGL_lose_context" };
  await lostPage.close(); activePage = null;
  report.status = "passed";
} catch (error) {
  report.status = "failed"; report.error = String(error.stack || error); console.error(report.error); process.exitCode = 1;
  if (activePage) await activePage.screenshot({ path: `${reportDir}/failure.png` });
} finally {
  await writeFile(`${reportDir}/${process.argv.includes("--errors-only") ? "browser-errors" : "browser"}.json`, JSON.stringify(report, null, 2) + "\n");
  await browser?.close();
  if (server) { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}
