import assert from "node:assert/strict";
import { test } from "node:test";
import { adaptPreview } from "../src/adapter.ts";
import { bindSelection } from "../src/selection.ts";
import type { SelectionHost, SourceSelection } from "../src/selection.ts";
import { fixture } from "./helpers.ts";

test("full IDs distinguish bodies and topology, support remove/clear and detach", () => {
  const { bytes, manifest } = fixture("two-boxes"), { sourceMap } = adaptPreview(bytes, manifest);
  const history: (readonly SourceSelection[])[] = [], errors: Error[] = [];
  let originalCalls = 0;
  const previous = () => { originalCalls++; };
  const host: SelectionHost = { onAfterRender: previous, cadTools: { selectObject: { selectedShapes: [] } } };
  const detach = bindSelection(host, sourceMap, value => history.push(value), error => errors.push(error));
  function render(ids: string[]) {
    host.cadTools.selectObject.selectedShapes = ids.map(id => ({ backendId: id, topo: sourceMap.get(id)!.kind, fromSolid: false }));
    host.onAfterRender!();
  }
  const a = "/model/body_1/faces/faces_1", b = "/model/body_101/faces/faces_1", e = "/model/body_101/edges/edges_1";
  render([]); render([a]); render([b]); render([e]); render([a, b, e]); render([a, b]); render([]);
  assert.deepEqual(history.map(s => s.map(v => v.backendId)), [[], [a], [b], [e], [a, b, e], [a, b], []]);
  assert.deepEqual(history[1]![0]!.primitive.parasolid_face_ids, [2]);
  assert.deepEqual(history[2]![0]!.primitive.parasolid_face_ids, [102]);
  assert.notDeepEqual(history[1]![0]!.primitive.source_entities, history[2]![0]!.primitive.source_entities);
  render([]); assert.equal(history.length, 7); assert.equal(originalCalls, 8);
  const stable = sourceMap.get(a)!.source_entities[0]!.byte_range.start;
  manifest.primitives[1].source_entities[0].byte_range.start = -99;
  assert.equal(sourceMap.get(a)!.source_entities[0]!.byte_range.start, stable);
  assert.throws(() => { (sourceMap.get(a)!.source_entities[0]!.byte_range as { start: number }).start = -99; }, TypeError);
  detach(); assert.equal(host.onAfterRender, previous); render([a]); assert.equal(history.length, 7);
  assert.deepEqual(errors, []);
});

test("unknown IDs, topology mismatch and whole-solid selection clear stale details and report once", () => {
  const { bytes, manifest } = fixture(), { sourceMap } = adaptPreview(bytes, manifest);
  const history: (readonly SourceSelection[])[] = [], errors: Error[] = [];
  const host: SelectionHost = { onAfterRender: null, cadTools: { selectObject: { selectedShapes: [] } } };
  bindSelection(host, sourceMap, value => history.push(value), error => errors.push(error));
  const id = "/model/body_1/faces/faces_0";
  for (const selection of [{ backendId: id, topo: "face", fromSolid: false },
    { backendId: "/unknown", topo: "face", fromSolid: false },
    { backendId: id, topo: "edge", fromSolid: false },
    { backendId: id, topo: "face", fromSolid: true }] as const) {
    host.cadTools.selectObject.selectedShapes = [selection]; host.onAfterRender!(); host.onAfterRender!();
  }
  assert.equal(history[0]!.length, 1); assert.ok(history.slice(1).every(s => s.length === 0)); assert.equal(errors.length, 3);
  host.cadTools.selectObject.selectedShapes = []; host.onAfterRender!();
  assert.equal(history.length, 5);
});
