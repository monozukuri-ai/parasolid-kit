import type { Shapes, ShapeBinary } from "three-cad-viewer";
import type { PrimitiveMetadata, Manifest, Bounds } from "./manifest.ts";
import { check, limit } from "./validation.ts";

/** Expand occurrences from shared source tessellations, keeping selection identity per path. */
export function placePreview(parts: Shapes[], sourceMap: ReadonlyMap<string, PrimitiveMetadata>, manifest: Manifest, baseBytes: number) {
  if (manifest.occurrences === undefined) return { parts, sourceMap, manifest, bufferBytes: baseBytes };
  const byBody = new Map<number, Shapes>();
  for (const part of parts) {
    const shape = part.shape as ShapeBinary;
    const p = sourceMap.get(`${part.id}/${shape.triangles_per_face.length ? "faces/faces_0" : "edges/edges_0"}`)!;
    check(p.body_ids.length === 1 && !byBody.has(p.body_ids[0]!), "Assembly requires unique body membership");
    byBody.set(p.body_ids[0]!, part);
  }
  let bufferBytes = baseBytes, vertices = 0, triangles = 0, samples = 0, entities = 0;
  const plans = manifest.occurrences.map(occurrence => {
    const part = byBody.get(occurrence.body_id);
    check(part, "Assembly body has no geometry");
    const shape = part.shape as ShapeBinary, m = occurrence.matrix;
    const cross = (a: readonly number[], b: readonly number[]) => [a[1]! * b[2]! - a[2]! * b[1]!, a[2]! * b[0]! - a[0]! * b[2]!, a[0]! * b[1]! - a[1]! * b[0]!];
    const cofactors = [cross(m[1]!, m[2]!), cross(m[2]!, m[0]!), cross(m[0]!, m[1]!)];
    const determinant = m[0]!.slice(0, 3).reduce((n, v, i) => n + v * cofactors[0]![i]!, 0);
    check(Number.isFinite(determinant) && determinant !== 0, "Assembly matrix is singular");
    vertices += (shape.vertices.length + shape.edges.length) / 3;
    triangles += shape.triangles.length / 3; samples += shape.edges.length / 3;
    entities += shape.triangles_per_face.length + shape.segments_per_edge.length;
    limit("max_vertices", vertices, manifest.limits.max_vertices);
    limit("max_triangles", triangles, manifest.limits.max_triangles);
    limit("max_curve_samples", samples, manifest.limits.max_curve_samples);
    limit("max_occt_subshapes", entities, manifest.limits.max_occt_subshapes);
    bufferBytes += (shape.vertices.length + shape.normals.length + shape.edges.length) * 4 + (determinant < 0 ? shape.triangles.byteLength : 0);
    limit("max_output_bytes", bufferBytes, manifest.limits.max_output_bytes);
    return { occurrence, part, shape, m, cofactors, determinant };
  });
  const bounds = [Infinity, Infinity, Infinity, -Infinity, -Infinity, -Infinity];
  const placedSources = new Map<string, PrimitiveMetadata>();
  const placed = plans.map(({ occurrence, part, shape, m, cofactors, determinant }): Shapes => {
    const transform = (source: Float32Array, normal: boolean) => {
      const output = new Float32Array(source.length);
      for (let i = 0; i < source.length; i += 3) {
        const values = (normal ? cofactors : m).map(row => (row[0]! * source[i]! + row[1]! * source[i + 1]! + row[2]! * source[i + 2]!) / (normal ? determinant : 1) + (normal ? 0 : row[3]!));
        const length = normal ? Math.hypot(...values) : 1;
        check(Number.isFinite(length) && length > 0, "Invalid placed normal");
        values.forEach((v, axis) => {
          const value = Math.fround(v / length); check(Number.isFinite(value), "Assembly placement overflow");
          output[i + axis] = value;
          if (!normal) { bounds[axis] = Math.min(bounds[axis]!, value); bounds[axis + 3] = Math.max(bounds[axis + 3]!, value); }
        });
      }
      return output;
    };
    const id = occurrence.instance_path.length ? `/model/instance_${occurrence.instance_path.join("_")}` : `/model/body_${occurrence.body_id}`;
    let indices = shape.triangles;
    if (determinant < 0) {
      indices = indices.slice();
      for (let i = 0; i < indices.length; i += 3) [indices[i + 1], indices[i + 2]] = [indices[i + 2]!, indices[i + 1]!];
    }
    for (const [kind, count] of [["face", shape.triangles_per_face.length], ["edge", shape.segments_per_edge.length]] as const) {
      for (let i = 0; i < count; i++) {
        const original = sourceMap.get(`${part.id}/${kind}s/${kind}s_${i}`)!;
        placedSources.set(`${id}/${kind}s/${kind}s_${i}`, Object.freeze({ ...original, instance_path: occurrence.instance_path }));
      }
    }
    return { ...part, id, name: occurrence.instance_path.length ? `Instance ${occurrence.instance_path.join(" / ")} · Body ${occurrence.body_id}` : part.name,
      shape: { ...shape, vertices: transform(shape.vertices, false), normals: transform(shape.normals, true), edges: transform(shape.edges, false), triangles: indices } };
  });
  return { parts: placed, sourceMap: placedSources, bufferBytes,
    manifest: Object.freeze({ ...manifest, bounds: Object.freeze(bounds) as unknown as Bounds }) };
}
