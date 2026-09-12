/** Validation for the deliberately bounded, package-generated preview format. */
export type JsonObject = Record<string, unknown>;

export class PreviewDataError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PreviewDataError";
  }
}

export class PreviewLimitError extends PreviewDataError {
  readonly resource: string;
  readonly observed: number;
  readonly limit: number;
  constructor(resource: string, observed: number, limit: number) {
    super(`${resource}: ${observed} exceeds ${limit}`);
    this.name = "PreviewLimitError";
    this.resource = resource;
    this.observed = observed;
    this.limit = limit;
  }
}

export function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new PreviewDataError(message);
}

export function object(value: unknown, name: string): JsonObject {
  check(value !== null && typeof value === "object" && !Array.isArray(value), `${name} must be an object`);
  return value as JsonObject;
}

export function array(value: unknown, name: string): unknown[] {
  check(Array.isArray(value), `${name} must be an array`);
  return value;
}

export function integer(value: unknown, name: string, minimum = 0): number {
  check(typeof value === "number" && Number.isSafeInteger(value) && value >= minimum, `${name} must be a safe integer >= ${minimum}`);
  return value;
}

export function finite(value: unknown, name: string): number {
  check(typeof value === "number" && Number.isFinite(value), `${name} must be finite`);
  return value;
}

export function string(value: unknown, name: string): string {
  check(typeof value === "string", `${name} must be a string`);
  return value;
}

export function boolean(value: unknown, name: string): boolean {
  check(typeof value === "boolean", `${name} must be boolean`);
  return value;
}

export function strings(value: unknown, name: string): readonly string[] {
  return Object.freeze(array(value, name).map(v => string(v, name)));
}

export function ids(value: unknown, name: string): readonly number[] {
  const result = array(value, name).map(v => integer(v, name));
  check(new Set(result).size === result.length, `${name} contains duplicate IDs`);
  return Object.freeze(result.sort((a, b) => a - b));
}

export function limit(resource: string, value: number, maximum: number): number {
  if (!Number.isSafeInteger(value) || value < 0 || value > maximum) {
    throw new PreviewLimitError(resource, value, maximum);
  }
  return value;
}

export interface PreviewLimits {
  readonly max_entities: number;
  readonly max_occt_subshapes: number;
  readonly max_curve_samples: number;
  readonly max_triangles: number;
  readonly max_vertices: number;
  readonly max_output_bytes: number;
  readonly max_diagnostics: number;
}

// Match the Python InteropLimits defaults. Tests compare these with live fixture manifests.
export const DEFAULT_LIMITS: Readonly<PreviewLimits> = Object.freeze({
  max_entities: 1_000_000, max_occt_subshapes: 2_000_000, max_curve_samples: 1_000_000,
  max_triangles: 5_000_000, max_vertices: 10_000_000, max_output_bytes: 512 * 1024 * 1024,
  max_diagnostics: 10_000,
});

/** Neither the manifest nor the caller can raise the browser's hard ceilings. */
export function resolveLimits(declared: unknown, requested: Partial<PreviewLimits> = {}): Readonly<PreviewLimits> {
  const source = object(declared, "limits");
  for (const key of Object.keys(requested)) check(Object.hasOwn(DEFAULT_LIMITS, key), `Unknown limit: ${key}`);
  const result = {} as Record<keyof PreviewLimits, number>;
  for (const key of Object.keys(DEFAULT_LIMITS) as (keyof PreviewLimits)[]) {
    const supplied = requested[key] === undefined ? DEFAULT_LIMITS[key] : integer(requested[key], key, 1);
    result[key] = Math.min(DEFAULT_LIMITS[key], integer(source[key], `limits.${key}`, 1), supplied);
  }
  return Object.freeze(result);
}
