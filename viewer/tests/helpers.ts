import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const fixtureRoot = fileURLToPath(new URL("fixtures/", import.meta.url));
export function fixture(name = "box") {
  const input = readFileSync(`${fixtureRoot}${name}/preview.glb`);
  return { bytes: input.buffer.slice(input.byteOffset, input.byteOffset + input.byteLength),
    manifest: JSON.parse(readFileSync(`${fixtureRoot}${name}/preview.manifest.json`, "utf8")) };
}
// Independent test-only byte reader/editor: never calls the production GLB decoder.
export function unpack(bytes: ArrayBuffer) {
  const header = new DataView(bytes), jsonLength = header.getUint32(12, true);
  const doc = JSON.parse(new TextDecoder().decode(new Uint8Array(bytes, 20, jsonLength)));
  const binary = new Uint8Array(bytes.slice(28 + jsonLength));
  return { doc, binary };
}
export function repack(doc: unknown, binary: Uint8Array): ArrayBuffer {
  const json = new TextEncoder().encode(JSON.stringify(doc));
  const jsonLength = Math.ceil(json.length / 4) * 4, bytes = new ArrayBuffer(28 + jsonLength + binary.length);
  const header = new DataView(bytes);
  [0x46546c67, 2, bytes.byteLength, jsonLength, 0x4e4f534a].forEach((n, i) => header.setUint32(i * 4, n, true));
  new Uint8Array(bytes, 20, jsonLength).fill(32); new Uint8Array(bytes, 20, json.length).set(json);
  header.setUint32(20 + jsonLength, binary.length, true); header.setUint32(24 + jsonLength, 0x004e4942, true);
  new Uint8Array(bytes, 28 + jsonLength).set(binary);
  return bytes;
}
