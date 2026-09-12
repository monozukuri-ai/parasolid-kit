import { array, boolean, check, finite, ids, integer, limit, object, resolveLimits, string, strings } from "./validation.ts";
import type { PreviewLimits } from "./validation.ts";

export interface SourceEntity {
  readonly key: string;
  readonly kind: string;
  readonly entity_id: number;
  readonly node_index: number;
  readonly node_type: number;
  readonly type_name: string;
  readonly node_id: number | null;
  readonly byte_range: Readonly<{ start: number; end: number; length: number }>;
  readonly relation: string;
  readonly relation_note?: string;
}
export interface PrimitiveMetadata {
  readonly primitive_index: number;
  readonly pick_id: number;
  readonly kind: "face" | "edge";
  readonly target_key: string;
  readonly vertex_count: number;
  readonly triangle_count: number;
  readonly curve_sample_count: number;
  readonly source_entities: readonly SourceEntity[];
  readonly parasolid_face_ids: readonly number[];
  readonly parasolid_edge_ids: readonly number[];
  readonly body_ids: readonly number[];
  readonly surface_kinds: readonly string[];
  readonly curve_kinds: readonly string[];
  readonly diagnostic_codes: readonly string[];
}
export type Bounds = readonly [number, number, number, number, number, number];
export interface Manifest {
  readonly limits: Readonly<PreviewLimits>;
  readonly bodies: ReadonlyMap<number, string>;
  readonly primitives: readonly PrimitiveMetadata[];
  readonly bounds: Bounds;
  readonly targetUnit: string;
  readonly partial: boolean;
  readonly counts: Readonly<Record<string, number>>;
}
const sourceKinds = new Set(["body", "region", "shell", "face", "loop", "half_edge", "edge", "vertex", "point", "curve", "surface"]);

function sourceEntity(value: unknown): SourceEntity {
  const s = object(value, "source entity"), r = object(s.byte_range, "byte_range");
  const kind = string(s.kind, "source.kind"), id = integer(s.entity_id, "entity_id");
  const key = string(s.key, "source.key");
  check(sourceKinds.has(kind) && key === `parasolid:${kind}:${String(id).padStart(6, "0")}`, "Source key/identity mismatch");
  const start = integer(r.start, "byte_range.start"), end = integer(r.end, "byte_range.end");
  check(end >= start && r.length === end - start, "Invalid byte range");
  const relation = string(s.relation, "relation");
  check(["direct", "split", "merged", "generated"].includes(relation), "Invalid source relation");
  return Object.freeze({ key, kind, entity_id: id,
    node_index: integer(s.node_index, "node_index"), node_type: integer(s.node_type, "node_type"),
    type_name: string(s.type_name, "type_name"), node_id: s.node_id === null ? null : integer(s.node_id, "node_id"),
    byte_range: Object.freeze({ start, end, length: end - start }), relation,
    ...(s.relation_note === undefined ? {} : { relation_note: string(s.relation_note, "relation_note") }),
  });
}

/** Validate consumed schema-1 fields; additive producer fields remain available to the UI in its original manifest. */
export function readManifest(value: unknown, requested?: Partial<PreviewLimits>): Manifest {
  const m = object(value, "manifest");
  check(m.schema_version === 1 && m.producer === "parasolid-kit.interop.preview", "Unsupported preview manifest");
  const limits = resolveLimits(m.limits, requested);
  const source = object(m.source, "source"), conversion = object(m.conversion, "conversion");
  check(conversion.complete === true && conversion.occt_valid === true, "Preview requires a complete, valid OCCT conversion");
  const targetUnit = string(conversion.target_unit, "target_unit");
  check(["m", "cm", "mm", "in", "ft"].includes(targetUnit), "Unsupported target unit");
  check(finite(conversion.applied_scale, "applied_scale") > 0, "Scale must be positive");
  const preview = object(m.preview, "preview"), options = object(preview.options, "preview.options");
  const partial = boolean(preview.partial, "partial"), complete = boolean(source.complete, "source.complete");
  const missing = array(m.missing_entities, "missing_entities"), diagnostics = array(m.diagnostics, "diagnostics");
  limit("max_occt_subshapes", missing.length, limits.max_occt_subshapes);
  limit("max_diagnostics", diagnostics.length, limits.max_diagnostics);
  check(partial === (!complete || missing.length > 0), "Inconsistent partial status");
  check(!partial || options.allow_partial === true, "Partial preview requires allow_partial");
  const counts = Object.fromEntries(Object.entries(object(preview.counts, "counts")).map(([k, v]) => [k, integer(v, `counts.${k}`)]));
  for (const kind of ["face", "edge"]) {
    check(counts[`missing_${kind}s`] === missing.filter(v => object(v, "missing entity").kind === kind).length, "Missing entity count mismatch");
  }
  check(counts.missing_faces! + counts.missing_edges! === missing.length, "Unsupported missing entity kind");
  const bounds = array(preview.bounds, "bounds").map(v => finite(v, "bounds"));
  check(bounds.length === 6 && bounds.slice(0, 3).every((v, i) => v <= bounds[i + 3]!), "Invalid preview bounds");
  const bodyData = array(m.bodies, "bodies (regenerate previews made before V1)"), bodies = new Map<number, string>();
  limit("max_entities", bodyData.length, limits.max_entities);
  for (const value of bodyData) {
    const b = object(value, "body"), id = integer(b.id, "body.id"), kind = string(b.kind, "body.kind");
    check(!bodies.has(id) && ["solid", "sheet", "wire", "general"].includes(kind), "Invalid or duplicate body");
    bodies.set(id, kind);
  }
  const data = array(m.primitives, "primitives"), picks = new Set<number>(), targets = new Set<string>();
  limit("max_occt_subshapes", data.length + missing.length, limits.max_occt_subshapes);
  limit("pick_ids", data.length, 0xffffff);
  check(data.length > 0, "Preview contains no geometry");
  let references = bodyData.length;
  const primitives = data.map((value, index): PrimitiveMetadata => {
    const p = object(value, "primitive"), kind = p.kind;
    check(p.primitive_index === index && (kind === "face" || kind === "edge"), "Invalid primitive order/kind");
    const pick = integer(p.pick_id, "pick_id", 1), target = string(p.target_key, "target_key");
    check(pick <= 0xffffff && !picks.has(pick) && !targets.has(target) && new RegExp(`^occt:${kind}:\\d{6,}$`).test(target), "Invalid or duplicate primitive identity");
    picks.add(pick); targets.add(target);
    const bodyIds = ids(p.body_ids, "body_ids");
    check(bodyIds.every(id => bodies.has(id)), "Primitive references an unknown body");
    const sources = array(p.source_entities, "source_entities");
    // Bound source occurrences too: one merged primitive must not amplify metadata without a limit.
    references += sources.length;
    limit("max_entities", references, limits.max_entities);
    const entities = Object.freeze(sources.map(sourceEntity));
    check(new Set(entities.map(s => s.key)).size === entities.length, "Duplicate source entity");
    const faceIds = ids(p.parasolid_face_ids, "parasolid_face_ids"), edgeIds = ids(p.parasolid_edge_ids, "parasolid_edge_ids");
    for (const [kind, values] of [["face", faceIds], ["edge", edgeIds]] as const) {
      const expected = entities.filter(s => s.kind === kind).map(s => s.entity_id).sort((a, b) => a - b);
      check(expected.length === values.length && expected.every((v, i) => v === values[i]), "Parasolid ID/source mismatch");
    }
    return Object.freeze({ primitive_index: index, pick_id: pick, target_key: target, kind,
      vertex_count: integer(p.vertex_count, "vertex_count", kind === "face" ? 3 : 2),
      triangle_count: kind === "face" ? integer(p.triangle_count, "triangle_count", 1) : 0,
      curve_sample_count: kind === "edge" ? integer(p.curve_sample_count, "curve_sample_count", 2) : 0,
      source_entities: entities, parasolid_face_ids: faceIds, parasolid_edge_ids: edgeIds, body_ids: bodyIds,
      surface_kinds: strings(p.surface_kinds, "surface_kinds"), curve_kinds: strings(p.curve_kinds, "curve_kinds"),
      diagnostic_codes: strings(p.diagnostic_codes, "diagnostic_codes"),
    });
  });
  check(options.include_edges === true || (options.include_edges === false && primitives.every(p => p.kind === "face")), "Inconsistent include_edges option");
  for (const [field, maximum] of [["vertices", limits.max_vertices], ["triangles", limits.max_triangles], ["curve_samples", limits.max_curve_samples]] as const) {
    limit(`max_${field}`, integer(counts[field], field), maximum);
  }
  return Object.freeze({ limits, bodies, primitives: Object.freeze(primitives), bounds: Object.freeze(bounds) as unknown as Bounds,
    targetUnit, partial, counts: Object.freeze(counts) });
}
