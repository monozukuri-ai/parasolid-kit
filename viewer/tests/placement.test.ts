import assert from "node:assert/strict";
import { test } from "node:test";
import type { ShapeBinary } from "three-cad-viewer";
import { adaptPreview } from "../src/adapter.ts";
import { filterPreview } from "../src/filter.ts";
import { PreviewDataError, PreviewLimitError } from "../src/validation.ts";
import { fixture, unpack, repack } from "./helpers.ts";

function assembled() {
  const input = fixture();
  input.manifest.occurrences = [
    { body_id: 1, instance_path: [7], assembly_path: [3], matrix: [[1,0,0,0], [0,1,0,0], [0,0,1,0]] },
    { body_id: 1, instance_path: [8,9], assembly_path: [3,4], matrix: [[0,-1,0,100], [1,0,0,0], [0,0,1,50]] },
  ];
  return input;
}

test("repeated bodies share input tessellation and keep distinct placed picks and bounds", () => {
  const input = assembled(), preview = adaptPreview(input.bytes, input.manifest);
  assert.equal(input.manifest.primitives.length, 18);
  assert.equal(preview.shapes.parts!.length, 2);
  assert.equal(preview.sourceMap.size, 36);
  assert.deepEqual(preview.manifest.bounds, [0,0,0,100,40,70]);
  assert.deepEqual(preview.shapes.bb, { xmin: 0, ymin: 0, zmin: 0, xmax: 100, ymax: 40, zmax: 70 });
  const first = preview.sourceMap.get("/model/instance_7/faces/faces_0")!;
  const second = preview.sourceMap.get("/model/instance_8_9/faces/faces_0")!;
  assert.deepEqual(first.source_entities, second.source_entities);
  assert.deepEqual(first.instance_path, [7]); assert.deepEqual(second.instance_path, [8,9]);
  const a = preview.shapes.parts![0]!.shape as ShapeBinary, b = preview.shapes.parts![1]!.shape as ShapeBinary;
  assert.equal(a.triangles, b.triangles); // The unchanged index array is reused.
  for (let i = 0; i < a.vertices.length; i += 3) {
    assert.equal(b.vertices[i], 100 - a.vertices[i+1]!);
    assert.equal(b.vertices[i+1], a.vertices[i]); assert.equal(b.vertices[i+2], a.vertices[i+2]! + 50);
    assert.ok(Math.abs(b.normals[i]! + a.normals[i+1]!) < 1e-7);
    assert.ok(Math.abs(b.normals[i+1]! - a.normals[i]!) < 1e-7);
  }
  const filtered = filterPreview(preview, { hiddenGroups: new Set(["/model/instance_7"]), surface: "", diagnostic: "", faces: true, edges: true });
  assert.equal(filtered.visible.size, 18);
  assert.ok([...filtered.visible].every(key => key.startsWith("/model/instance_8_9/")));
});

test("reflections reverse triangle winding and normals use inverse transpose", () => {
  const input = assembled();
  input.manifest.occurrences[1].matrix = [[-2,0,0,100], [0,3,0,0], [0,0,4,0]];
  const preview = adaptPreview(input.bytes, input.manifest);
  const a = preview.shapes.parts![0]!.shape as ShapeBinary, b = preview.shapes.parts![1]!.shape as ShapeBinary;
  for (let i = 0; i < a.triangles.length; i += 3) assert.deepEqual([...b.triangles.slice(i,i+3)], [a.triangles[i],a.triangles[i+2],a.triangles[i+1]]);
  for (let i = 0; i < b.normals.length; i += 3) assert.ok(Math.abs(Math.hypot(...b.normals.slice(i,i+3)) - 1) < 1e-6);
});

test("assembly expansion is bounded and malformed occurrences are rejected", () => {
  const input = assembled();
  const single = adaptPreview(fixture().bytes, fixture().manifest);
  const shape = single.shapes.parts![0]!.shape as ShapeBinary;
  const perBody = (shape.vertices.length + shape.edges.length) / 3;
  assert.throws(() => adaptPreview(input.bytes, input.manifest, { max_vertices: perBody * 2 - 1 }), PreviewLimitError);
  for (const change of [
    (m: any) => { m.occurrences[1].instance_path = [7]; m.occurrences[1].assembly_path = [3]; },
    (m: any) => { m.occurrences[0].body_id = 999; },
    (m: any) => { m.occurrences[0].matrix[0] = [0,0,0,0]; },
    (m: any) => { m.occurrences[0].matrix[0][3] = 1e100; },
    (m: any) => { m.occurrences[1].assembly_path = [3,3]; },
  ]) {
    const bad = assembled(); change(bad.manifest);
    assert.throws(() => adaptPreview(bad.bytes, bad.manifest), PreviewDataError);
  }
});

test("stored geometry renders without an OCCT validity or solid-capping claim", () => {
  const { bytes, manifest } = fixture(), { doc, binary } = unpack(bytes);
  manifest.conversion.backend = "stored"; manifest.conversion.occt_valid = null;
  manifest.source.complete = false; manifest.preview.partial = true; manifest.preview.options.allow_partial = true;
  manifest.bodies = [];
  manifest.primitives.forEach((p: any, i: number) => {
    p.body_ids = []; p.target_key = p.target_key.replace(/^occt:/, "native:");
    doc.meshes[0].primitives[i].extras.targetKey = p.target_key;
  });
  const preview = adaptPreview(repack(doc,binary), manifest);
  assert.equal(preview.manifest.backend, "stored");
  assert.ok(preview.shapes.parts!.every(p => p.subtype === "faces" && p.renderback));
  manifest.conversion.occt_valid = true;
  assert.throws(() => adaptPreview(repack(doc,binary), manifest), PreviewDataError);
});
