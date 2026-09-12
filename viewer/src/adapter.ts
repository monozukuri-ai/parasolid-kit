import type { Shapes, ShapeBinary } from "three-cad-viewer";
import { decodeGlb } from "./glb.ts";
import type { DecodedPrimitive } from "./glb.ts";
import { readManifest } from "./manifest.ts";
import type { Manifest, PrimitiveMetadata } from "./manifest.ts";
import { limit } from "./validation.ts";
import type { PreviewLimits } from "./validation.ts";

export interface AdaptedPreview {
  readonly shapes: Shapes;
  readonly sourceMap: ReadonlyMap<string, PrimitiveMetadata>;
  readonly manifest: Manifest;
  /** Input GLB plus the adapter's output typed arrays; excludes renderer/GPU/JS object overhead. */
  readonly bufferBytes: number;
}
interface Group {
  readonly id: string;
  readonly bodyIds: readonly number[];
  readonly faces: DecodedPrimitive[];
  readonly edges: DecodedPrimitive[];
  vertices: number;
  indices: number;
  segments: number;
}
const surfaceTypes: Readonly<Record<string, number>> = { plane: 0, cylinder: 1, cone: 2, sphere: 3, torus: 4, nurbs: 6 };
const curveTypes: Readonly<Record<string, number>> = { line: 0, circle: 1, ellipse: 2, hyperbola: 3, parabola: 4, nurbs: 6 };
function geometryType(kinds: readonly string[], types: Readonly<Record<string, number>>, other: number): number {
  return kinds.length === 1 && Object.hasOwn(types, kinds[0]!) ? types[kinds[0]!]! : other;
}

/** Group geometry by exact body membership. Source identity remains separate from mutable renderer data. */
export function adaptPreview(bytes: ArrayBuffer, metadata: unknown, requestedLimits?: Partial<PreviewLimits>): AdaptedPreview {
  const manifest = readManifest(metadata, requestedLimits), decoded = decodeGlb(bytes, manifest);
  const groups = new Map<string, Group>(), sourceMap = new Map<string, PrimitiveMetadata>();
  for (const primitive of decoded) {
    const bodyIds = primitive.metadata.body_ids;
    const id = `/model/body_${bodyIds.join("_") || "unknown"}`;
    let group = groups.get(id);
    if (!group) {
      group = { id, bodyIds, faces: [], edges: [], vertices: 0, indices: 0, segments: 0 };
      groups.set(id, group);
    }
    if (primitive.metadata.kind === "face") {
      group.faces.push(primitive); group.vertices += primitive.positions.length / 3; group.indices += primitive.indices.length;
    } else { group.edges.push(primitive); group.segments += primitive.indices.length - 1; }
  }
  // Complete preflight before allocating any concatenated geometry. Edge strips expand to endpoint pairs.
  let bufferBytes = bytes.byteLength, outputVertices = 0;
  for (const g of groups.values()) {
    outputVertices += g.vertices + 2 * g.segments;
    limit("max_vertices", outputVertices, manifest.limits.max_vertices);
    limit("index_count", g.indices, manifest.limits.max_triangles * 3);
    limit("max_occt_subshapes", g.faces.length + g.edges.length, manifest.limits.max_occt_subshapes);
    bufferBytes += g.vertices * 24 + g.indices * 4 + g.segments * 24 + g.faces.length * 8 + g.edges.length * 5;
    limit("max_output_bytes", bufferBytes, manifest.limits.max_output_bytes);
  }
  const sharedFaceBodies = new Set(decoded.filter(p => p.metadata.kind === "face" && p.metadata.body_ids.length > 1).flatMap(p => p.metadata.body_ids));
  const unknownFaces = decoded.some(p => p.metadata.kind === "face" && p.metadata.body_ids.length === 0);
  const parts: Shapes[] = [];
  for (const g of groups.values()) {
    const shape: ShapeBinary = {
      vertices: new Float32Array(g.vertices * 3), normals: new Float32Array(g.vertices * 3), triangles: new Uint32Array(g.indices),
      edges: new Float32Array(g.segments * 6), triangles_per_face: new Uint32Array(g.faces.length), segments_per_edge: new Uint32Array(g.edges.length),
      face_types: new Uint32Array(g.faces.length), edge_types: new Uint8Array(g.edges.length), obj_vertices: new Float32Array(0),
    };
    let vertexOffset = 0, indexOffset = 0, edgeOffset = 0;
    g.faces.forEach((f, i) => {
      shape.vertices.set(f.positions, vertexOffset * 3); shape.normals.set(f.normals!, vertexOffset * 3);
      for (const index of f.indices) shape.triangles[indexOffset++] = index + vertexOffset;
      vertexOffset += f.positions.length / 3;
      shape.triangles_per_face[i] = f.indices.length / 3;
      shape.face_types[i] = geometryType(f.metadata.surface_kinds, surfaceTypes, 10);
      sourceMap.set(`${g.id}/faces/faces_${i}`, f.metadata);
    });
    g.edges.forEach((e, i) => {
      for (let j = 0; j + 1 < e.indices.length; j++) {
        const a = e.indices[j]! * 3, b = e.indices[j + 1]! * 3;
        shape.edges.set(e.positions.subarray(a, a + 3), edgeOffset);
        shape.edges.set(e.positions.subarray(b, b + 3), edgeOffset + 3);
        edgeOffset += 6;
      }
      shape.segments_per_edge[i] = e.indices.length - 1;
      shape.edge_types[i] = geometryType(e.metadata.curve_kinds, curveTypes, 8);
      sourceMap.set(`${g.id}/edges/edges_${i}`, e.metadata);
    });
    const solid = !manifest.partial && !unknownFaces && !sharedFaceBodies.has(g.bodyIds[0]!) && g.faces.length > 0 && g.bodyIds.length === 1 && manifest.bodies.get(g.bodyIds[0]!) === "solid";
    const name = g.bodyIds.length === 0 ? "Unknown membership" : g.bodyIds.length === 1 ? `Body ${g.bodyIds[0]}` : `Shared bodies ${g.bodyIds.join(", ")}`;
    parts.push({ version: 3, id: g.id, name, type: "shapes", subtype: solid ? "solid" : "faces", shape,
      color: "#4b92c4", alpha: 1, renderback: !solid, state: [g.faces.length ? 1 : 3, g.edges.length ? 1 : 3] });
  }
  const [xmin, ymin, zmin, xmax, ymax, zmax] = manifest.bounds;
  return { shapes: { version: 3, id: "/model", name: "Parasolid", parts, bb: { xmin, ymin, zmin, xmax, ymax, zmax } }, sourceMap, manifest, bufferBytes };
}
