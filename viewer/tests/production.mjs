import assert from "node:assert/strict";
import { readFile, writeFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

process.chdir(fileURLToPath(new URL("..", import.meta.url)));
const assets = JSON.parse(await readFile("asset-manifest.json", "utf8"));
const oracle = JSON.parse(await readFile("tests/fixtures/oracle.json", "utf8"))["two-boxes"];
const python = process.env.VIEWER_PYTHON || "../.venv/bin/python";
const directory = await mkdtemp(join(tmpdir(), "parasolid-viewer-v3-"));
const report = { status: "running", platform: process.platform, architecture: process.arch, assets,
  input: "public synthetic two-box BrepModel; parser replaced, real CLI/OCCT/writer/server", cases: {} };
let browser, child, activePage;
await mkdir("test-results", { recursive: true });

function startup(process) {
  return new Promise((resolve, reject) => {
    let stdout = "", stderr = "";
    const timer = setTimeout(() => reject(new Error(`CLI startup timed out: ${stderr}\n${stdout}`)), 30000);
    process.stderr.on("data", data => { stderr += data; });
    process.on("error", error => { clearTimeout(timer); reject(error); });
    process.on("exit", code => { clearTimeout(timer); reject(new Error(`CLI exited ${code}: ${stderr}\n${stdout}`)); });
    process.stdout.on("data", data => {
      stdout += data;
      let json; try { json = JSON.parse(stdout); } catch { return; }
      clearTimeout(timer); resolve(json);
    });
  });
}

try {
  browser = await chromium.launch({ ...(process.env.VIEWER_CHROME ? { executablePath: process.env.VIEWER_CHROME } : {}),
    headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage", "--enable-unsafe-swiftshader", "--use-angle=swiftshader-webgl"] });
  report.browser = browser.version();
  for (const command of ["viewer", "view"]) {
    const output = join(directory, command);
    child = spawn(python, ["tests/launch_cli.py", command, "synthetic-two-boxes.x_t", "--source-unit", "mm", "--no-open", "--output", output]);
    const response = await startup(child);
    assert.equal(response.status, "serving");
    assert.equal(response.preview.asset_bundle.version, assets.version);
    const origin = new URL(response.url).origin;
    for (const [name, expected] of Object.entries(assets.assets)) {
      assert.equal(createHash("sha256").update(await readFile(join(output, name))).digest("hex"), expected.sha256);
    }
    // Mutate only fixture metadata on disk before the browser loads it.
    const metadata = JSON.parse(await readFile(response.manifest, "utf8"));
    const diagnostic = '<img id="diagnostic-injection" src="https://example.invalid/diagnostic" onerror="window.injected=true">';
    const note = '<img id="source-injection" src="https://example.invalid/source" onerror="window.injected=true">';
    metadata.diagnostics.push({ code: "fixture.injection", severity: "warning", message: diagnostic });
    for (const primitive of metadata.primitives) for (const source of primitive.source_entities) source.relation_note = note;
    await writeFile(response.manifest, JSON.stringify(metadata));
    const result = { response, requests: [], consoleErrors: [], pageErrors: [], badResponses: [], externalRequests: [], selections: [] };
    report.cases[command] = result;
    const page = await browser.newPage({ viewport: { width: 1440, height: 850 }, deviceScaleFactor: 1 }); activePage = page;
    page.on("console", m => { if (m.type() === "error") result.consoleErrors.push(m.text()); });
    page.on("pageerror", e => result.pageErrors.push(String(e)));
    page.on("response", r => { if (!r.ok()) result.badResponses.push([r.status(), r.url()]); });
    page.on("request", r => result.requests.push(r.url()));
    await page.route("**/*", async route => {
      if (new URL(route.request().url()).origin !== origin) { result.externalRequests.push(route.request().url()); await route.abort(); }
      else await route.continue();
    });
    await page.addInitScript(() => {
      window.cspViolations = [];
      document.addEventListener("securitypolicyviolation", e => cspViolations.push([e.effectiveDirective, e.blockedURI]));
    });
    const document = await page.goto(response.url);
    result.csp = document.headers()["content-security-policy"];
    assert.equal(result.csp, "default-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; script-src 'self'; style-src 'self'; style-src-attr 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'");
    await page.waitForFunction(() => document.getElementById("preview-app").dataset.status !== "loading");
    assert.equal(await page.locator("#preview-app").getAttribute("data-status"), "ready", await page.locator("#preview-error").textContent());
    await page.evaluate(async () => { window.testViewer = await (await import("./viewer.js")).application; });
    result.renderer = await page.evaluate(() => {
      const gl = testViewer.viewer.renderer.getContext(), info = gl.getExtension("WEBGL_debug_renderer_info");
      return info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
    });
    await page.locator("#diagnostics-panel summary").click();
    assert.ok((await page.locator("#diagnostic-list").innerText()).includes(diagnostic));
    for (const [kind, id, world] of [["face", 2, [20, 15, 20]], ["face", 102, [70, 15, 20]], ["edge", 6, [40, 15, 20]], ["edge", 106, [90, 15, 20]]]) {
      const key = `parasolid:${kind}:${String(id).padStart(6, "0")}`;
      await page.locator("#pick-kind").selectOption(kind);
      const point = await page.evaluate(world => {
        const viewer = testViewer.viewer, camera = viewer.camera.getCamera();
        const v = camera.position.clone().set(...world).project(camera), r = viewer.renderer.domElement.getBoundingClientRect();
        return { x: r.x + (v.x + 1) * r.width / 2, y: r.y + (1 - v.y) * r.height / 2 };
      }, world);
      await page.mouse.move(point.x, point.y); await page.waitForTimeout(180); await page.mouse.click(point.x, point.y);
      await page.waitForFunction(key => testViewer.selection.some(s => s.primitive.source_entities.some(v => v.key === key)), key);
      const selected = await page.evaluate(key => testViewer.selection.find(s => s.primitive.source_entities.some(v => v.key === key)), key);
      const source = selected.primitive.source_entities.find(s => s.key === key);
      for (const [field, value] of Object.entries(oracle.sources[key])) assert.deepEqual(source[field], value, `${key}.${field}`);
      assert.equal(source.relation_note, note);
      assert.ok((await page.locator("#selection-details").innerText()).includes(note));
      result.selections.push({ key, world, source, backendId: selected.backendId, realPointerClick: true });
      await page.locator("#clear-selection").click();
    }
    assert.equal(await page.locator("#source-injection, #diagnostic-injection").count(), 0);
    assert.equal(await page.evaluate(() => window.injected), undefined);
    result.cspViolations = await page.evaluate(() => cspViolations);
    for (const key of ["consoleErrors", "pageErrors", "badResponses", "externalRequests", "cspViolations"]) assert.deepEqual(result[key], [], key);
    assert.deepEqual([...new Set(result.requests.map(url => new URL(url).pathname))].sort(), ["/", "/preview.glb", "/preview.manifest.json", "/viewer.css", "/viewer.js"]);
    await page.screenshot({ path: `test-results/production-${command}.png` });
    await page.close(); activePage = null;
    const exited = once(child, "exit"); child.kill("SIGINT");
    const timeout = setTimeout(() => child.kill("SIGKILL"), 5000);
    const [code, signal] = await exited; clearTimeout(timeout); child = null;
    assert.equal(code, 0); assert.equal(signal, null);
    result.interruptExitCode = code;
    await assert.rejects(fetch(response.url), /fetch failed/);
    result.serverClosed = true;
  }
  report.status = "passed";
} catch (error) {
  report.status = "failed"; report.error = String(error.stack || error); console.error(report.error); process.exitCode = 1;
  if (activePage) await activePage.screenshot({ path: "test-results/production-failure.png" });
} finally {
  if (child && child.exitCode === null) { const stopped = once(child, "exit"); child.kill("SIGKILL"); await stopped; }
  await browser?.close();
  await writeFile("test-results/production.json", JSON.stringify(report, null, 2) + "\n");
  await rm(directory, { recursive: true, force: true });
}
