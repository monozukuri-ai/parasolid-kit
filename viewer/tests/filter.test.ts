import assert from "node:assert/strict";
import { test } from "node:test";
import type { ShapeBinary } from "three-cad-viewer";
import { fixture } from "./helpers.ts";
import { adaptPreview } from "../src/adapter.ts";
import { filterPreview } from "../src/filter.ts";
import type { Filters } from "../src/filter.ts";
import { PreviewLimitError } from "../src/validation.ts";
const all: Filters = { hiddenGroups: new Set(), surface: "", diagnostic: "", faces: true, edges: true };
test("sparse filter keeps local topology slots and immutable source geometry", () => {
  const { bytes, manifest } = fixture("two-boxes");
  manifest.primitives[1].diagnostic_codes = ["fixture.top"];
  const preview = adaptPreview(bytes, manifest);
  const filtered = filterPreview(preview, { ...all, diagnostic: "fixture.top" });
  assert.deepEqual([...filtered.visible], ["/model/body_1/faces/faces_1"]);
  const shape = filtered.shapes.parts![0]!.shape as ShapeBinary;
  assert.deepEqual([...shape.triangles_per_face], [0, 2, 0, 0, 0, 0]);
  assert.equal(shape.triangles.length, 6);
  assert.equal(filtered.shapes.parts![0]!.subtype, "faces");
  assert.deepEqual(preview.sourceMap.get([...filtered.visible][0]!)!.parasolid_face_ids, [2]);
  const restored = filterPreview(preview, all);
  assert.equal(restored.visible.size, 36);
  assert.deepEqual(restored.shapes.parts![0]!.shape, preview.shapes.parts![0]!.shape);
});
test("body and surface filters combine, including empty and edges-only views", () => {
  const { bytes, manifest } = fixture("two-boxes"), preview = adaptPreview(bytes, manifest);
  const filtered = filterPreview(preview, { ...all, hiddenGroups: new Set(["/model/body_1"]), faces: false });
  assert.equal(filtered.visible.size, 12); assert.ok([...filtered.visible].every(id => id.startsWith("/model/body_101/edges/")));
  const empty = filterPreview(preview, { ...all, surface: "cylinder" });
  assert.equal(empty.visible.size, 0);
  assert.ok(empty.shapes.parts!.every(p => (p.shape as ShapeBinary).triangles.length === 0 && (p.shape as ShapeBinary).edges.length === 0));
});
test("partial generated manifest has missing provenance and stays faces after filtering", () => {
  const { bytes, manifest } = fixture("box-partial"), preview = adaptPreview(bytes, manifest);
  assert.equal(preview.manifest.partial, true); assert.equal(manifest.missing_entities[0].entity_id, 1);
  assert.ok(filterPreview(preview, all).shapes.parts!.every(p => p.subtype === "faces"));
});
test("filter buffer allocation obeys the remaining adapter budget", () => {
  const { bytes, manifest } = fixture(), preview = adaptPreview(bytes, manifest);
  const limited = adaptPreview(bytes, manifest, { max_output_bytes: preview.bufferBytes });
  assert.throws(() => filterPreview(limited, all), PreviewLimitError);
});
