import type { Shapes, ShapeBinary } from "three-cad-viewer";
import type { AdaptedPreview } from "./adapter.ts";
import { limit } from "./validation.ts";

export interface Filters {
  readonly hiddenGroups: ReadonlySet<string>;
  readonly surface: string;
  readonly diagnostic: string;
  readonly faces: boolean;
  readonly edges: boolean;
}

/** Keep zero-count topology slots: 5.0.6 registers their original local index even when hidden. */
export function filterPreview(preview: AdaptedPreview, filters: Filters): { shapes: Shapes; visible: ReadonlySet<string> } {
  const visible = new Set<string>();
  const plans = preview.shapes.parts!.map(part => {
    const shape = part.shape as ShapeBinary;
    const included = (kind: "face" | "edge", index: number) => {
      const id = `${part.id}/${kind}s/${kind}s_${index}`, p = preview.sourceMap.get(id)!;
      const show = !filters.hiddenGroups.has(part.id) && filters[`${kind}s`] &&
        (!filters.surface || p.surface_kinds.includes(filters.surface)) &&
        (!filters.diagnostic || p.diagnostic_codes.includes(filters.diagnostic));
      if (show) visible.add(id);
      return show;
    };
    const faces = Array.from(shape.triangles_per_face, (_, i) => included("face", i));
    const edges = Array.from(shape.segments_per_edge, (_, i) => included("edge", i));
    const indexCount = shape.triangles_per_face.reduce((n, count, i) => n + (faces[i] ? count * 3 : 0), 0);
    const edgeCount = shape.segments_per_edge.reduce((n, count, i) => n + (edges[i] ? count * 6 : 0), 0);
    return { part, shape, faces, edges, indexCount, edgeCount };
  });
  // Source arrays stay immutable. Budget the additional index/endpoint/count arrays before allocating.
  const extraBytes = plans.reduce((n, p) => n + (p.indexCount + p.edgeCount + p.faces.length + p.edges.length) * 4, 0);
  limit("max_output_bytes", preview.bufferBytes + extraBytes, preview.manifest.limits.max_output_bytes);
  const parts = plans.map(({ part, shape, faces, edges, indexCount, edgeCount }): Shapes => {
    const triangles = new Uint32Array(indexCount), endpoints = new Float32Array(edgeCount);
    const faceCounts = new Uint32Array(faces.length), edgeCounts = new Uint32Array(edges.length);
    let source = 0, target = 0;
    faces.forEach((show, i) => {
      const count = shape.triangles_per_face[i]!;
      if (show) { triangles.set(shape.triangles.subarray(source, source + count * 3), target); target += count * 3; faceCounts[i] = count; }
      source += count * 3;
    });
    source = 0; target = 0;
    edges.forEach((show, i) => {
      const count = shape.segments_per_edge[i]!;
      if (show) { endpoints.set(shape.edges.subarray(source, source + count * 6), target); target += count * 6; edgeCounts[i] = count; }
      source += count * 6;
    });
    const solid = part.subtype === "solid" && faces.every(Boolean);
    return { ...part, subtype: solid ? "solid" : "faces", renderback: !solid,
      state: [indexCount ? 1 : 0, edgeCount ? 1 : 0],
      shape: { ...shape, triangles, edges: endpoints, triangles_per_face: faceCounts, segments_per_edge: edgeCounts } };
  });
  return { shapes: { ...preview.shapes, parts }, visible };
}
