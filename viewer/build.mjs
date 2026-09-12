import { build } from "esbuild";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { basename } from "node:path";

process.chdir(fileURLToPath(new URL(".", import.meta.url)));
const mode = process.argv.slice(2);
assert.ok(mode.length <= 1 && mode.every(arg => ["--update", "--sync"].includes(arg)), "Usage: node build.mjs [--update|--sync]");
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const pkg = JSON.parse(await readFile("package.json", "utf8"));
const lockBytes = await readFile("package-lock.json");
const licenseBytes = await readFile("third-party/manifest.json");
const licenses = JSON.parse(licenseBytes);
const notices = [];
for (const item of licenses.packages) {
  const installed = JSON.parse(await readFile(`node_modules/${item.packagePath}/package.json`, "utf8"));
  assert.equal(installed.version, item.version, `${item.name} version`);
  assert.equal(installed.license, item.declaredLicense, `${item.name} declared license`);
  const original = await readFile(`node_modules/${item.packagePath}/${item.licensePath}`);
  const notice = await readFile(`third-party/${item.noticeFile}`);
  assert.equal(hash(original), item.sha256, `${item.name} upstream notice hash`);
  assert.deepEqual(notice, original, `${item.name} preserved notice`);
  notices.push(`${item.name}@${item.version}\nDeclared: ${item.declaredLicense}; license text: ${item.noticeLicense}\n${item.discrepancy || ""}\n${notice.toString("utf8")}`);
}
const banner = `/*! Parasolid viewer ${pkg.version}\nThird-party notices (unaltered license text):\n${notices.join("\n\n")}\n*/`;
assert.ok(!banner.slice(0, -2).includes("*/"), "Notice contains a comment terminator");
const result = await build({
  entryPoints: ["src/index.ts"], bundle: true, format: "esm", target: "chrome125",
  outfile: "dist/viewer.js", minify: true, sourcemap: false, write: false,
  legalComments: "inline", metafile: true, banner: { js: banner, css: banner },
  loader: { ".woff2": "dataurl", ".png": "dataurl", ".svg": "dataurl" },
});
// Upstream ESM already bundles its runtime dependencies. Reject a second Three instance.
const dependencyInputs = Object.keys(result.metafile.inputs).filter(p => p.startsWith("node_modules/"));
assert.deepEqual(dependencyInputs.sort(), [
  "node_modules/three-cad-viewer/dist/three-cad-viewer.css",
  "node_modules/three-cad-viewer/dist/three-cad-viewer.esm.js",
]);
// Upstream uses fixed HTML templates and contains URLs in notices/SVG namespaces.
// Keep HTML insertion out of our source-data boundary, independently of that bundle.
for (const name of Object.keys(result.metafile.inputs).filter(p => p.startsWith("src/") && p.endsWith(".ts"))) {
  assert.doesNotMatch(await readFile(name, "utf8"), /\b(innerHTML|outerHTML|insertAdjacentHTML|eval)\b|document\s*\.\s*write\b|new\s+Function\b/, `${name}: source values must remain text`);
}
for (const output of Object.values(result.metafile.outputs)) assert.ok(output.imports.every(i => i.kind === "url-token" && i.path.startsWith("data:image/")), "Bundle must be self-contained");
const assets = {};
await mkdir("dist", { recursive: true });
for (const file of result.outputFiles) {
  const name = basename(file.path);
  assert.ok(["viewer.js", "viewer.css"].includes(name));
  assets[name] = { bytes: file.contents.length, sha256: hash(file.contents) };
  await writeFile(`dist/${name}`, file.contents);
}
const html = await readFile("src/index.html");
assets["index.html"] = { bytes: html.length, sha256: hash(html) };
await writeFile("dist/index.html", html);
const manifest = { version: pkg.version, threeCadViewer: pkg.dependencies["three-cad-viewer"], protocol: 3,
  lockSha256: hash(lockBytes), licensesSha256: hash(licenseBytes), assets };
const serialized = JSON.stringify(manifest, null, 2) + "\n";
if (process.argv.includes("--update")) await writeFile("asset-manifest.json", serialized);
else assert.equal(serialized, await readFile("asset-manifest.json", "utf8"), "Assets changed: review and run npm run build:update");
await writeFile("dist/asset-manifest.json", serialized);
// Never rewrite the writer or the independent archive gate's reviewed hash lists.
if (!process.argv.includes("--update")) for (const name of Object.keys(assets)) {
  const destination = `../src/parasolid_kit/interop/preview/static/${name}`;
  const data = await readFile(`dist/${name}`);
  if (process.argv.includes("--sync")) await writeFile(destination, data);
  else assert.deepEqual(await readFile(destination), data, `${name}: run npm run build:sync and review the Python asset hash lists`);
}
console.log(serialized.trim());
