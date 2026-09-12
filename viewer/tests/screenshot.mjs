// Capture the real UI from the public two-box preview served by launch_cli.py.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { chromium } from "playwright";

const [url, output] = process.argv.slice(2);
assert.ok(url && output, "Usage: node tests/screenshot.mjs URL output.png");
const origin = new URL(url).origin;
assert.ok(["127.0.0.1", "localhost", "[::1]"].includes(new URL(url).hostname), "Use a loopback preview URL");
const assets = JSON.parse(await readFile(new URL("../asset-manifest.json", import.meta.url), "utf8"));
const oracle = JSON.parse(await readFile(new URL("fixtures/oracle.json", import.meta.url), "utf8"))["two-boxes"];
const reportPath = new URL("../test-results/screenshot.json", import.meta.url);
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");
const report = { status: "running", platform: process.platform, architecture: process.arch, node: process.version,
  input: "public synthetic two-box BrepModel; real CLI/OCCT/writer/server, parser replaced",
  assets, requests: [], consoleErrors: [], pageErrors: [], badResponses: [], failedRequests: [], externalRequests: [] };
let browser;
try {
  browser = await chromium.launch({ headless: true,
    ...(process.env.VIEWER_CHROME ? { executablePath: process.env.VIEWER_CHROME } : {}),
    args: ["--no-sandbox", "--disable-dev-shm-usage", "--enable-unsafe-swiftshader", "--use-angle=swiftshader-webgl"] });
  report.browser = browser.version();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1 });
  page.on("console", message => { if (message.type() === "error") report.consoleErrors.push(message.text()); });
  page.on("pageerror", error => report.pageErrors.push(String(error)));
  page.on("response", response => { if (!response.ok()) report.badResponses.push([response.status(), response.url()]); });
  page.on("requestfailed", request => report.failedRequests.push(request.url()));
  page.on("request", request => report.requests.push(new URL(request.url()).pathname));
  await page.route("**/*", async route => {
    if (new URL(route.request().url()).origin !== origin) {
      report.externalRequests.push(route.request().url()); await route.abort();
    } else await route.continue();
  });
  await page.addInitScript(() => {
    window.cspViolations = [];
    document.addEventListener("securitypolicyviolation", event => window.cspViolations.push([event.effectiveDirective, event.blockedURI]));
  });
  const response = await page.goto(url);
  report.csp = response.headers()["content-security-policy"];
  await page.waitForFunction(() => document.getElementById("preview-app").dataset.status !== "loading");
  assert.equal(await page.locator("#preview-app").getAttribute("data-status"), "ready", await page.locator("#preview-error").textContent());
  report.servedHashes = await page.evaluate(async names => {
    const hashes = {};
    for (const name of names) {
      const response = await fetch(name);
      if (!response.ok) throw new Error(`Asset HTTP ${response.status}: ${name}`);
      const digest = await crypto.subtle.digest("SHA-256", await response.arrayBuffer());
      hashes[name] = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, "0")).join("");
    }
    return hashes;
  }, Object.keys(assets.assets));
  for (const [name, expected] of Object.entries(assets.assets)) assert.equal(report.servedHashes[name], expected.sha256);
  await page.evaluate(async () => { window.testViewer = await (await import("./viewer.js")).application; });
  const point = await page.evaluate(() => {
    const viewer = testViewer.viewer, camera = viewer.camera.getCamera();
    const position = camera.position.clone().set(70, 15, 20).project(camera);
    const rect = viewer.renderer.domElement.getBoundingClientRect();
    return { x: rect.x + (position.x + 1) * rect.width / 2, y: rect.y + (1 - position.y) * rect.height / 2 };
  });
  await page.mouse.move(point.x, point.y); await page.waitForTimeout(180); await page.mouse.click(point.x, point.y);
  const key = "parasolid:face:000102";
  await page.waitForFunction(key => testViewer.selection.some(s => s.primitive.source_entities.some(v => v.key === key)), key);
  const selected = await page.evaluate(key => testViewer.selection.find(s => s.primitive.source_entities.some(v => v.key === key)), key);
  const source = selected.primitive.source_entities.find(s => s.key === key);
  for (const [field, value] of Object.entries(oracle.sources[key])) assert.deepEqual(source[field], value, field);
  report.selection = { source, backendId: selected.backendId, realPointerClick: true };
  report.renderer = await page.evaluate(() => {
    const gl = testViewer.viewer.renderer.getContext(), info = gl.getExtension("WEBGL_debug_renderer_info");
    return info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
  });
  await page.mouse.move(10, 10);
  await mkdir(dirname(resolve(output)), { recursive: true });
  const png = await page.screenshot({ path: output });
  report.image = { sha256: sha256(png), bytes: png.length, width: 1440, height: 1000 };
  report.cspViolations = await page.evaluate(() => window.cspViolations);
  for (const key of ["consoleErrors", "pageErrors", "badResponses", "failedRequests", "externalRequests", "cspViolations"]) assert.deepEqual(report[key], [], key);
  report.status = "passed";
} catch (error) {
  report.status = "failed"; report.error = String(error.stack || error); console.error(report.error); process.exitCode = 1;
} finally {
  await browser?.close();
  await mkdir(new URL("../test-results/", import.meta.url), { recursive: true });
  await writeFile(reportPath, JSON.stringify(report, null, 2) + "\n");
}
