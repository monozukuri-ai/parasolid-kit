import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import type { ShapeBinary } from "three-cad-viewer";
import { adaptPreview } from "../src/adapter.ts";
import { DEFAULT_LIMITS, PreviewDataError, PreviewLimitError } from "../src/validation.ts";
import { fixture, fixtureRoot, unpack, repack } from "./helpers.ts";

const oracle = JSON.parse(readFileSync(`${fixtureRoot}oracle.json`, "utf8"));
const cases = [
  ["box", 6, 12, 12, 1], ["cylinder-hole", 4, 6, 232, 1], ["two-boxes", 12, 24, 24, 2],
  ["sheet", 1, 4, 2, 1], ["box-cm-no-edges", 6, 0, 12, 1],
] as const;
for (const [name, faces, edges, triangles, bodies] of cases) test(`${name}: geometry, units, source oracle and typed arrays`, () => {
  const { bytes, manifest } = fixture(name), result = adaptPreview(bytes, manifest);
  assert.deepEqual(manifest.limits, DEFAULT_LIMITS);
  assert.deepEqual(manifest.bodies, oracle[name].bodies);
  for (const [file, expected] of Object.entries(oracle[name].inputs)) {
    assert.equal(createHash("sha256").update(readFileSync(`${fixtureRoot}${name}/${file}`)).digest("hex"), expected);
  }
  assert.equal(result.sourceMap.size, faces + edges); assert.equal(result.shapes.parts!.length, bodies);
  const { doc, binary } = unpack(bytes);
  function values(index: number): Float32Array | Uint32Array {
    const a = doc.accessors[index], b = doc.bufferViews[a.bufferView];
    return a.componentType === 5126 ? new Float32Array(binary.buffer, b.byteOffset, a.count * 3) : new Uint32Array(binary.buffer, b.byteOffset, a.count);
  }
  let outputTriangles = 0, outputBytes = bytes.byteLength;
  for (const part of result.shapes.parts!) {
    const shape = part.shape as ShapeBinary;
    assert.equal(part.subtype, name === "sheet" ? "faces" : "solid");
    assert.ok(shape.vertices instanceof Float32Array); assert.ok(shape.normals instanceof Float32Array);
    assert.ok(shape.triangles instanceof Uint32Array); assert.ok(shape.edges instanceof Float32Array);
    assert.ok(shape.triangles_per_face instanceof Uint32Array); assert.ok(shape.segments_per_edge instanceof Uint32Array);
    assert.equal(shape.obj_vertices.length, 0);
    outputBytes += Object.values(shape).reduce((n, a) => n + a.byteLength, 0);
    outputTriangles += shape.triangles.length / 3;
    let vo = 0, ti = 0, eo = 0;
    for (const [kind, length] of [["face", shape.triangles_per_face.length], ["edge", shape.segments_per_edge.length]] as const) {
      for (let local = 0; local < length; local++) {
        const source = result.sourceMap.get(`${part.id}/${kind}s/${kind}s_${local}`)!;
        assert.ok(source);
        const expected = manifest.primitives[source.primitive_index], primitive = doc.meshes[0].primitives[source.primitive_index];
        assert.deepEqual(source.source_entities, expected.source_entities);
        for (const ref of source.source_entities) for (const [field, value] of Object.entries(oracle[name].sources[ref.key])) {
          assert.deepEqual(ref[field as keyof typeof ref], value, `${name} ${ref.key} ${field}`);
        }
        const position = values(primitive.attributes.POSITION), indices = values(primitive.indices);
        if (kind === "face") {
          assert.deepEqual(shape.vertices.subarray(vo, vo + position.length), position);
          assert.deepEqual(shape.normals.subarray(vo, vo + position.length), values(primitive.attributes.NORMAL));
          for (const index of indices) assert.equal(shape.triangles[ti++], index + vo / 3);
          assert.equal(shape.triangles_per_face[local], indices.length / 3);
          for (let k = vo; k < vo + position.length; k += 3) {
            assert.ok(Math.abs(Math.hypot(shape.normals[k]!, shape.normals[k + 1]!, shape.normals[k + 2]!) - 1) < 1e-6);
          }
          vo += position.length;
        } else {
          for (let i = 0; i < indices.length - 1; i++) for (const index of [indices[i]!, indices[i + 1]!]) {
            assert.deepEqual(shape.edges.subarray(eo, eo + 3), position.subarray(index * 3, index * 3 + 3)); eo += 3;
          }
          assert.equal(shape.segments_per_edge[local], indices.length - 1);
        }
      }
    }
  }
  assert.equal(outputTriangles, triangles); assert.equal(outputBytes, result.bufferBytes);
  const [xmin, ymin, zmin, xmax, ymax, zmax] = manifest.preview.bounds;
  assert.deepEqual(result.shapes.bb, { xmin, ymin, zmin, xmax, ymax, zmax });
  if (name === "box-cm-no-edges") {
    assert.equal(result.manifest.targetUnit, "cm"); assert.deepEqual(manifest.preview.bounds, [0, 0, 0, 4, 3, 2]);
  }
});

test("shared/unknown/partial membership stays conservative and is never duplicated", () => {
  const { bytes, manifest } = fixture("two-boxes");
  manifest.primitives[0].body_ids = [101, 1];
  manifest.primitives[1].body_ids = [1, 101];
  const shared = adaptPreview(bytes, manifest);
  assert.equal(shared.shapes.parts!.length, 3); assert.equal(shared.sourceMap.size, 36);
  assert.ok(shared.shapes.parts!.every(p => p.subtype === "faces"));
  assert.equal((shared.shapes.parts!.find(p => p.id === "/model/body_1_101")!.shape as ShapeBinary).triangles_per_face.length, 2);
  manifest.primitives[0].body_ids = [];
  assert.ok(adaptPreview(bytes, manifest).shapes.parts!.some(p => p.id === "/model/body_unknown"));
  const box = fixture(); box.manifest.preview.partial = true; box.manifest.source.complete = false; box.manifest.preview.options.allow_partial = true;
  assert.equal(adaptPreview(box.bytes, box.manifest).shapes.parts![0]!.subtype, "faces");
});

test("generated edges preserve face sources and ambiguous types use Other", () => {
  const { bytes, manifest } = fixture("cylinder-hole");
  const result = adaptPreview(bytes, manifest);
  assert.ok([...result.sourceMap.values()].some(p => p.kind === "edge" && !p.parasolid_edge_ids.length && p.parasolid_face_ids.length));
  manifest.primitives[0].surface_kinds = ["plane", "cylinder"];
  manifest.primitives[1].surface_kinds = ["trimmed"];
  const shape = adaptPreview(bytes, manifest).shapes.parts![0]!.shape as ShapeBinary;
  assert.deepEqual([...shape.face_types.slice(0, 2)], [10, 10]);
});

const mutations: [string, (m: ReturnType<typeof fixture>["manifest"]) => void][] = [
  ["missing bodies", m => { delete m.bodies; }], ["unknown body", m => { m.primitives[0].body_ids = [999]; }],
  ["duplicate body", m => { m.bodies.push(m.bodies[0]); }], ["duplicate pick", m => { m.primitives[1].pick_id = m.primitives[0].pick_id; }],
  ["unsafe identity", m => { m.primitives[0].source_entities[0].node_id = 2 ** 53; }],
  ["bad byte range", m => { m.primitives[0].source_entities[0].byte_range.end = 0; }],
  ["source ID mismatch", m => { m.primitives[0].parasolid_face_ids = [999]; }],
  ["wrong target", m => { m.primitives[0].target_key = "occt:face:999999"; }],
  ["counts", m => { m.preview.counts.triangles++; }], ["bounds", m => { m.preview.bounds[3]++; }],
  ["not partial", m => { m.source.complete = false; }], ["incomplete conversion", m => { m.conversion.complete = false; }],
];
for (const [name, change] of mutations) test(`reject manifest: ${name}`, () => {
  const { bytes, manifest } = fixture(); change(manifest);
  assert.throws(() => adaptPreview(bytes, manifest), PreviewDataError);
});
const corruptions: [string, (d: ReturnType<typeof unpack>["doc"], b: Uint8Array) => void][] = [
  ["external URI", d => { d.buffers[0].uri = "https://example.invalid/model.bin"; }],
  ["node transform", d => { d.nodes[0].translation = [1, 0, 0]; }],
  ["compression", d => { d.extensionsUsed = ["KHR_draco_mesh_compression"]; }],
  ["sparse", d => { d.accessors[0].sparse = {}; }], ["stride", d => { d.bufferViews[0].byteStride = 12; }],
  ["misalignment", d => { d.accessors[0].byteOffset = 1; }], ["view overrun", d => { d.accessors[0].byteOffset = 4; }],
  ["bin overrun", d => { d.bufferViews[0].byteLength = 1_000_000; }],
  ["alias", d => { d.meshes[0].primitives[1].attributes.POSITION = 0; }],
  ["wrong mode", d => { d.meshes[0].primitives[0].mode = 5; }],
  ["negative accessor count", d => { d.accessors[0].count = -1; }],
  ["wrong normal count", d => { d.accessors[1].count = 3; }],
  ["out of range index", (d, b) => { new DataView(b.buffer).setUint32(d.bufferViews[2].byteOffset, 999, true); }],
  ["nonfinite position", (_d, b) => { new DataView(b.buffer).setFloat32(0, NaN, true); }],
  ["nonfinite normal", (d, b) => { new DataView(b.buffer).setFloat32(d.bufferViews[1].byteOffset, Infinity, true); }],
];
for (const [name, corrupt] of corruptions) test(`reject GLB: ${name}`, () => {
  const { bytes, manifest } = fixture(), { doc, binary } = unpack(bytes); corrupt(doc, binary);
  assert.throws(() => adaptPreview(repack(doc, binary), manifest), PreviewDataError);
});

test("truncated headers/chunks fail with preview errors", () => {
  const { bytes, manifest } = fixture();
  for (const size of [0, 12, 20, 28, bytes.byteLength - 1]) assert.throws(() => adaptPreview(bytes.slice(0, size), manifest), PreviewDataError);
});

test("limits reject input counts and expanded output before materialization", () => {
  const { bytes, manifest } = fixture("cylinder-hole");
  const result = adaptPreview(bytes, manifest);
  const outputVertices = result.shapes.parts!.reduce((n, p) => n + (p.shape as ShapeBinary).vertices.length / 3 + (p.shape as ShapeBinary).edges.length / 3, 0);
  assert.ok(outputVertices > manifest.preview.counts.vertices);
  for (const requested of [{ max_vertices: manifest.preview.counts.vertices }, { max_output_bytes: result.bufferBytes - 1 },
    { max_triangles: 1 }, { max_curve_samples: 1 }, { max_occt_subshapes: 1 }, { max_entities: 1 }]) {
    assert.throws(() => adaptPreview(bytes, manifest, requested), PreviewLimitError);
  }
  assert.equal(adaptPreview(bytes, manifest, { max_output_bytes: result.bufferBytes }).bufferBytes, result.bufferBytes);
  const huge = fixture(); huge.manifest.limits.max_output_bytes = Number.MAX_SAFE_INTEGER;
  assert.equal(adaptPreview(huge.bytes, huge.manifest, { max_output_bytes: Number.MAX_SAFE_INTEGER }).manifest.limits.max_output_bytes, DEFAULT_LIMITS.max_output_bytes);
});
