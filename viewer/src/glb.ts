import { array, check, integer, limit, object, PreviewDataError } from "./validation.ts";
import type { JsonObject } from "./validation.ts";
import type { Manifest, PrimitiveMetadata } from "./manifest.ts";

export interface DecodedPrimitive {
  readonly positions: Float32Array;
  readonly normals: Float32Array | null;
  readonly indices: Uint32Array;
  readonly metadata: PrimitiveMetadata;
}
function keys(value: JsonObject, allowed: string[], label: string): void {
  check(Object.keys(value).every(k => allowed.includes(k)), `Unsupported ${label} property`);
}
function one(value: unknown, label: string): JsonObject {
  const values = array(value, label);
  check(values.length === 1, `Expected one ${label}`);
  return object(values[0], label);
}

/** Only the embedded, untransformed GLB subset emitted by preview/glb.py. Views borrow the input buffer. */
export function decodeGlb(bytes: ArrayBuffer, manifest: Manifest): readonly DecodedPrimitive[] {
  const limits = manifest.limits;
  limit("max_output_bytes", bytes.byteLength, limits.max_output_bytes);
  check(bytes.byteLength >= 28, "Truncated GLB header");
  const view = new DataView(bytes);
  check(view.getUint32(0, true) === 0x46546c67 && view.getUint32(4, true) === 2 && view.getUint32(8, true) === bytes.byteLength, "Invalid GLB header");
  const jsonLength = view.getUint32(12, true), binHeader = 20 + jsonLength;
  check(jsonLength % 4 === 0 && binHeader + 8 <= bytes.byteLength && view.getUint32(16, true) === 0x4e4f534a, "Invalid GLB JSON chunk");
  const binLength = view.getUint32(binHeader, true), binOffset = binHeader + 8;
  check(binLength % 4 === 0 && binOffset + binLength === bytes.byteLength && view.getUint32(binHeader + 4, true) === 0x004e4942, "Invalid GLB BIN chunk");
  let parsed: unknown;
  try { parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(bytes, 20, jsonLength))); }
  catch { throw new PreviewDataError("Invalid GLB JSON encoding"); }
  const doc = object(parsed, "GLB");
  keys(doc, ["asset", "scene", "scenes", "nodes", "meshes", "materials", "buffers", "bufferViews", "accessors"], "GLB");
  const asset = object(doc.asset, "asset");
  keys(asset, ["version", "generator"], "asset");
  check(asset.version === "2.0" && doc.scene === 0, "Unsupported GLB version/scene");
  const scene = one(doc.scenes, "scene"), node = one(doc.nodes, "node"), mesh = one(doc.meshes, "mesh"), buffer = one(doc.buffers, "buffer");
  keys(scene, ["nodes"], "scene"); keys(node, ["mesh", "name"], "node");
  keys(mesh, ["name", "primitives"], "mesh"); keys(buffer, ["byteLength"], "buffer");
  const sceneNodes = array(scene.nodes, "scene.nodes");
  check(sceneNodes.length === 1 && sceneNodes[0] === 0 && node.mesh === 0, "Unsupported GLB scene graph");
  const byteLength = integer(buffer.byteLength, "buffer.byteLength");
  check(byteLength <= binLength && binLength - byteLength < 4, "Invalid embedded buffer length");
  const primitives = array(mesh.primitives, "mesh.primitives"), accessors = array(doc.accessors, "accessors"), views = array(doc.bufferViews, "bufferViews");
  check(primitives.length === manifest.primitives.length, "GLB/manifest primitive count mismatch");
  // Generated buffers have one accessor and one view per attribute/index. Reject unused or aliased data.
  const expectedAccessors = manifest.primitives.reduce((n, p) => n + (p.kind === "face" ? 3 : 2), 0);
  check(accessors.length === expectedAccessors && views.length === expectedAccessors, "Unexpected accessor/view count");
  const usedAccessors = new Set<number>(), usedViews = new Set<number>();
  const ranges: [number, number][] = [];
  function accessor(index: unknown, vector: boolean): Float32Array | Uint32Array {
    const id = integer(index, "accessor index");
    check(id < accessors.length && !usedAccessors.has(id), "Unknown or reused accessor");
    usedAccessors.add(id);
    const a = object(accessors[id], "accessor");
    keys(a, ["bufferView", "byteOffset", "componentType", "count", "type", "min", "max"], "accessor");
    check(a.componentType === (vector ? 5126 : 5125) && a.type === (vector ? "VEC3" : "SCALAR"), "Unsupported accessor type");
    const count = integer(a.count, "accessor.count", 1), size = count * (vector ? 12 : 4);
    limit(vector ? "max_vertices" : "index_count", count, vector ? limits.max_vertices : Math.max(limits.max_triangles * 3, limits.max_curve_samples));
    limit("max_output_bytes", size, limits.max_output_bytes);
    const viewId = integer(a.bufferView, "bufferView index");
    check(viewId < views.length && !usedViews.has(viewId), "Unknown or reused bufferView");
    usedViews.add(viewId);
    const b = object(views[viewId], "bufferView");
    keys(b, ["buffer", "byteOffset", "byteLength", "target"], "bufferView");
    check(b.buffer === 0 && b.target === (vector ? 34962 : 34963), "Invalid bufferView target");
    const offset = integer(b.byteOffset ?? 0, "bufferView.byteOffset"), length = integer(b.byteLength, "bufferView.byteLength"), local = integer(a.byteOffset ?? 0, "accessor.byteOffset");
    check(offset % 4 === 0 && local % 4 === 0 && offset + length <= byteLength && local + size <= length, "Accessor exceeds or misaligns its bufferView");
    ranges.push([offset, offset + length]);
    const start = binOffset + offset + local;
    return vector ? new Float32Array(bytes, start, count * 3) : new Uint32Array(bytes, start, count);
  }
  let vertices = 0, triangles = 0, samples = 0, faces = 0, edges = 0;
  const bounds = [Infinity, Infinity, Infinity, -Infinity, -Infinity, -Infinity];
  const decoded = primitives.map((value, index): DecodedPrimitive => {
    const p = object(value, "GLB primitive"), m = manifest.primitives[index]!;
    keys(p, ["attributes", "indices", "material", "mode", "extras"], "primitive");
    const extras = object(p.extras, "primitive.extras"), attrs = object(p.attributes, "attributes");
    check(extras.entityKind === m.kind && extras.pickId === m.pick_id && extras.targetKey === m.target_key, "GLB/manifest source mapping mismatch");
    check(p.mode === (m.kind === "face" ? 4 : 3), "Unsupported primitive mode");
    keys(attrs, m.kind === "face" ? ["POSITION", "NORMAL"] : ["POSITION"], "attributes");
    const positions = accessor(attrs.POSITION, true) as Float32Array;
    const indices = accessor(p.indices, false) as Uint32Array;
    const normals = m.kind === "face" ? accessor(attrs.NORMAL, true) as Float32Array : null;
    const count = positions.length / 3;
    check(count === m.vertex_count, "Primitive vertex count mismatch");
    vertices += count; limit("max_vertices", vertices, limits.max_vertices);
    for (let j = 0; j < positions.length; j++) {
      const v = positions[j]!, axis = j % 3;
      check(Number.isFinite(v), "Non-finite position");
      bounds[axis] = Math.min(bounds[axis]!, v); bounds[axis + 3] = Math.max(bounds[axis + 3]!, v);
    }
    for (const value of indices) check(value < count, "Index out of range");
    if (normals !== null) {
      check(normals.length === positions.length && indices.length % 3 === 0 && indices.length / 3 === m.triangle_count, "Face array/count mismatch");
      for (const value of normals) check(Number.isFinite(value), "Non-finite normal");
      triangles += indices.length / 3; faces++;
      limit("max_triangles", triangles, limits.max_triangles);
    } else {
      check(indices.length === count && count === m.curve_sample_count, "Edge array/count mismatch");
      for (let i = 0; i < count; i++) check(indices[i] === i, "Expected sequential LINE_STRIP indices");
      samples += count; edges++;
      limit("max_curve_samples", samples, limits.max_curve_samples);
    }
    return { positions, indices, normals, metadata: m };
  });
  ranges.sort((a, b) => a[0] - b[0]);
  for (let i = 1; i < ranges.length; i++) check(ranges[i]![0] >= ranges[i - 1]![1], "Overlapping bufferViews");
  for (const [key, value] of Object.entries({ vertices, triangles, curve_samples: samples, face_primitives: faces, edge_primitives: edges })) {
    check(manifest.counts[key] === value, `GLB/manifest ${key} count mismatch`);
  }
  check(bounds.every((v, i) => v === Math.fround(manifest.bounds[i]!)), "GLB/manifest bounds mismatch");
  return decoded;
}
