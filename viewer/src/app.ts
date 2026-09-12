import { Display, Viewer } from "three-cad-viewer";
import type { ViewerOptions } from "three-cad-viewer";
import { adaptPreview } from "./adapter.ts";
import type { AdaptedPreview } from "./adapter.ts";
import { filterPreview } from "./filter.ts";
import { bindSelection, setSelectionKind } from "./selection.ts";
import type { SourceSelection } from "./selection.ts";
import { array, object, string, check, DEFAULT_LIMITS, limit, PreviewDataError } from "./validation.ts";

function element<T extends HTMLElement>(root: HTMLElement, id: string): T {
  const value = root.querySelector<T>(`#${id}`);
  if (!value) throw new Error(`Missing viewer element: ${id}`);
  return value;
}
function detail(parent: HTMLElement, name: string, value: unknown): void {
  const term = document.createElement("dt"), description = document.createElement("dd");
  term.textContent = name;
  description.textContent = typeof value === "string" ? value : JSON.stringify(value);
  parent.append(term, description);
}
function showSelection(root: HTMLElement, selection: readonly SourceSelection[]): void {
  element(root, "selection-title").textContent = selection.length ? `${selection.length} selected` : "Nothing selected";
  const panel = element(root, "selection-details"); panel.replaceChildren();
  for (const { backendId, primitive: p } of selection) {
    const section = document.createElement("section"); section.className = "selection-item";
    section.dataset.backendId = backendId;
    const heading = document.createElement("h3"); heading.textContent = `${p.kind} · ${p.target_key}`;
    const data = document.createElement("dl"); data.className = "details";
    detail(data, "Body IDs", p.body_ids.length ? p.body_ids.join(", ") : "Unknown membership");
    detail(data, "Parasolid face IDs", p.parasolid_face_ids.join(", ") || "—");
    detail(data, "Parasolid edge IDs", p.parasolid_edge_ids.join(", ") || "—");
    detail(data, "Surface / curve kinds", [...p.surface_kinds, ...p.curve_kinds].join(", ") || "—");
    detail(data, "Diagnostics", p.diagnostic_codes.join(", ") || "—");
    section.append(heading, data);
    for (const source of p.source_entities) {
      const record = document.createElement("dl"); record.className = "details source-record";
      detail(record, "Source", source.key);
      detail(record, "Node", `${source.type_name} · ID ${source.node_id ?? "unknown"} · index ${source.node_index} · type ${source.node_type}`);
      detail(record, "Byte range", `[${source.byte_range.start}, ${source.byte_range.end}) · ${source.byte_range.length} bytes`);
      detail(record, "Relation", source.relation);
      if (source.relation_note) detail(record, "Relation note", source.relation_note);
      section.append(record);
    }
    if (!p.source_entities.length) detail(data, "Source", "No source mapping available");
    panel.append(section);
  }
}
function showStatus(root: HTMLElement, value: unknown, preview: AdaptedPreview): void {
  const m = object(value, "manifest"), source = object(m.source, "source");
  element(root, "source-status").textContent = `Source ${source.complete ? "complete" : "incomplete"}`;
  element(root, "source-status").classList.toggle("warning", !source.complete);
  element(root, "conversion-status").textContent = "Conversion complete";
  element(root, "occt-status").textContent = "OCCT valid";
  element(root, "unit-status").textContent = `Unit ${preview.manifest.targetUnit}`;
  element(root, "footer-unit").textContent = preview.manifest.targetUnit;
  element(root, "partial-banner").hidden = !preview.manifest.partial;
  const missingList = element(root, "missing-list");
  for (const value of array(m.missing_entities, "missing_entities")) {
    const missing = object(value, "missing entity"), row = document.createElement("li");
    row.textContent = `${string(missing.kind, "missing.kind")} · ${string(missing.source_key, "missing.source_key")} · ${string(missing.reason, "missing.reason")}`;
    missingList.append(row);
  }
  if (preview.manifest.partial && !missingList.childElementCount) {
    const row = document.createElement("li"); row.textContent = "Source is incomplete; additional source geometry may be absent."; missingList.append(row);
  }
  const diagnostics = array(m.diagnostics, "diagnostics");
  element(root, "diagnostic-count").textContent = String(diagnostics.length);
  for (const value of diagnostics) {
    const diagnostic = object(value, "diagnostic"), row = document.createElement("li");
    row.textContent = `${string(diagnostic.code, "diagnostic.code")} · ${string(diagnostic.message, "diagnostic.message")}\n${JSON.stringify(diagnostic, null, 2)}`;
    element(root, "diagnostic-list").append(row);
  }
}
function options(select: HTMLSelectElement, values: Iterable<string>): void {
  for (const value of [...new Set(values)].sort()) {
    const option = document.createElement("option"); option.value = value; option.textContent = value; select.append(option);
  }
}
export interface PreviewApp {
  readonly viewer: Viewer;
  readonly preview: AdaptedPreview;
  readonly selection: readonly SourceSelection[];
  readonly visible: ReadonlySet<string>;
  dispose(): void;
}

/** Create the application from already-loaded inputs; it owns renderer, controls and observers. */
export function createPreviewApp(root: HTMLElement, bytes: ArrayBuffer, metadata: unknown): PreviewApp {
  const preview = adaptPreview(bytes, metadata);
  showStatus(root, metadata, preview);
  const get = <T extends HTMLElement>(id: string) => element<T>(root, id);
  const surface = get<HTMLSelectElement>("surface-filter"), diagnostic = get<HTMLSelectElement>("diagnostic-filter");
  options(surface, [...preview.sourceMap.values()].flatMap(p => p.surface_kinds));
  options(diagnostic, [...preview.sourceMap.values()].flatMap(p => p.diagnostic_codes));
  const hiddenGroups = new Set<string>(), abort = new AbortController();
  const cad = get("cad"), container = get("cad-container");
  const rect = container.getBoundingClientRect();
  const display = new Display(cad, { cadWidth: Math.max(1, rect.width - 2), height: Math.max(1, rect.height - 2), treeWidth: 0,
    tools: false, glass: false, pinning: false, zscaleTool: false, externalMeasurementBackend: false,
    measureTools: false, selectTool: true, explodeTool: false, zebraTool: false, studioTool: false, theme: "light" });
  let viewer: Viewer;
  try { viewer = new Viewer(display, {}, () => {}); }
  catch (error) { display.dispose(); cad.replaceChildren(); throw new PreviewDataError(`WebGL2 could not start: ${error instanceof Error ? error.message : error}`); }
  let selection: readonly SourceSelection[] = [], visible: ReadonlySet<string> = new Set(), disposed = false;
  let detach = () => {}, frame = 0;
  const observer = new ResizeObserver(() => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      if (disposed || !viewer.ready) return;
      const rect = container.getBoundingClientRect();
      viewer.resizeCadView(Math.max(1, rect.width - 2), 0, Math.max(1, rect.height - 2));
    });
  });
  function dispose(): void {
    if (disposed) return;
    disposed = true; abort.abort(); observer.disconnect(); cancelAnimationFrame(frame); detach();
    selection = []; visible = new Set(); showSelection(root, selection);
    viewer.dispose(); display.dispose(); cad.replaceChildren();
    root.dataset.status = "disposed";
  }
  function fail(error: unknown): void {
    dispose(); showError(root, error);
  }
  function on(target: HTMLElement, event: string, callback: () => void): void {
    target.addEventListener(event, () => { try { callback(); } catch (error) { fail(error); } }, { signal: abort.signal });
  }
  function clear(): void {
    if (viewer.ready) viewer.clearSelection();
    selection = []; showSelection(root, selection);
  }
  function setPickKind(): void {
    setSelectionKind(display, viewer, get<HTMLSelectElement>("pick-kind").value as "face" | "edge");
  }
  function clip(): void {
    clear();
    const axis = get<HTMLSelectElement>("clip-axis").value, enabled = axis !== "off";
    const reverse = get<HTMLInputElement>("clip-reverse"), position = get<HTMLInputElement>("clip-position");
    reverse.disabled = position.disabled = !enabled;
    viewer.resetClip();
    viewer.clipping.setVisible(enabled); viewer.setLocalClipping(enabled);
    get("clip-value").textContent = "—";
    if (enabled) {
      const index = Number(axis) as 0 | 1 | 2, bounds = preview.manifest.bounds;
      const coordinate = bounds[index] + (bounds[index + 3]! - bounds[index]) * Number(position.value) / 100;
      const center = (bounds[index] + bounds[index + 3]!) / 2, sign = reverse.checked ? 1 : -1;
      const normal: [number, number, number] = [0, 0, 0]; normal[index] = sign;
      viewer.setClipNormal(index, normal);
      // refreshPlane accepts signed constants including -1, which setClipSlider treats as a sentinel.
      viewer.refreshPlane(index, -sign * (coordinate - center));
      get("clip-value").textContent = `${coordinate.toPrecision(5)} ${preview.manifest.targetUnit}`;
    }
    viewer.update(true, false);
  }
  function render(): void {
    const camera: ViewerOptions = viewer.ready ? { position: viewer.getCameraPosition() as [number, number, number],
      quaternion: viewer.getCameraQuaternion(), target: viewer.getCameraTarget(), zoom: viewer.getCameraZoom() } : {};
    clear(); detach();
    // Clear first so the old filtered buffers/GPU objects do not overlap a replacement allocation.
    if (viewer.ready) viewer.clear();
    const filtered = filterPreview(preview, { hiddenGroups, surface: surface.value, diagnostic: diagnostic.value,
      faces: get<HTMLInputElement>("show-faces").checked, edges: get<HTMLInputElement>("show-edges").checked });
    visible = filtered.visible;
    detach = bindSelection(viewer, preview.sourceMap, next => {
      if (next.some(s => !visible.has(s.backendId))) { fail(new PreviewDataError("Selection refers to filtered geometry")); return; }
      selection = next; showSelection(root, next);
    }, fail);
    viewer.render(filtered.shapes, { edgeColor: 0x364f60 }, { ...camera, up: "Z", control: "orbit",
      ortho: get<HTMLInputElement>("orthographic").checked, axes: get<HTMLInputElement>("show-axes").checked,
      grid: [false, false, false] });
    display.setTool("select", true); setPickKind(); clip();
    const faces = [...visible].filter(id => preview.sourceMap.get(id)!.kind === "face").length;
    get("visible-count").textContent = `${faces} faces · ${visible.size - faces} edges`;
    get("empty-view").hidden = visible.size > 0;
  }
  try {
    for (const part of preview.shapes.parts!) {
      const label = document.createElement("label"), input = document.createElement("input"), text = document.createElement("span");
      input.type = "checkbox"; input.checked = true; input.dataset.groupId = part.id;
      const bodyId = preview.sourceMap.get(`${part.id}/faces/faces_0`)?.body_ids;
      const kind = bodyId?.length === 1 ? preview.manifest.bodies.get(bodyId[0]!) : undefined;
      text.textContent = `${part.name}${kind ? ` · ${kind}` : ""}`; label.append(input, text); get("body-list").append(label);
      on(input, "change", () => { if (input.checked) hiddenGroups.delete(part.id); else hiddenGroups.add(part.id); render(); });
    }
    if (!preview.manifest.counts.edge_primitives) {
      get<HTMLInputElement>("show-edges").checked = false; get<HTMLInputElement>("show-edges").disabled = true;
      get<HTMLSelectElement>("pick-kind").options[1]!.disabled = true;
    }
    for (const id of ["surface-filter", "diagnostic-filter", "show-faces", "show-edges"]) on(get(id), "change", render);
    on(get("reset-filters"), "click", () => {
      surface.value = diagnostic.value = ""; hiddenGroups.clear();
      get<HTMLInputElement>("show-faces").checked = true; get<HTMLInputElement>("show-edges").checked = !!preview.manifest.counts.edge_primitives;
      get("body-list").querySelectorAll<HTMLInputElement>("input").forEach(input => { input.checked = true; }); render();
    });
    on(get("pick-kind"), "change", setPickKind);
    on(get("clear-selection"), "click", () => { clear(); viewer.update(true, false); });
    on(get("show-axes"), "change", () => viewer.setAxes(get<HTMLInputElement>("show-axes").checked));
    on(get("orthographic"), "change", () => viewer.setOrtho(get<HTMLInputElement>("orthographic").checked));
    on(get("camera-view"), "change", () => viewer.presetCamera(get<HTMLSelectElement>("camera-view").value as "iso" | "top" | "front" | "right"));
    on(get("fit-view"), "click", () => { viewer.reset(); get<HTMLSelectElement>("camera-view").value = "iso"; });
    for (const id of ["clip-axis", "clip-reverse"]) on(get(id), "change", clip);
    on(get("clip-position"), "input", clip);
    on(viewer.renderer.domElement, "webglcontextlost", () => fail(new PreviewDataError("WebGL context was lost. Reload the preview to restart.")));
    cad.addEventListener("keydown", event => {
      if (event.key !== "Escape" && event.key !== "Backspace") event.stopPropagation();
    }, { capture: true, signal: abort.signal });
    render(); observer.observe(container);
    for (const id of ["view-controls", "filter-controls", "clip-controls"]) get<HTMLFieldSetElement>(id).disabled = false;
    get("load-status").hidden = true; root.dataset.status = "ready";
    window.addEventListener("pagehide", dispose, { once: true, signal: abort.signal });
    return { viewer, preview, get selection() { return selection; }, get visible() { return visible; }, dispose };
  } catch (error) { dispose(); throw error; }
}

export function showError(root: HTMLElement, error: unknown): void {
  element(root, "load-status").hidden = true;
  const panel = element(root, "preview-error"); panel.hidden = false;
  panel.textContent = `Preview failed: ${error instanceof Error ? error.message : String(error)}`;
  for (const id of ["view-controls", "filter-controls", "clip-controls"]) element<HTMLFieldSetElement>(root, id).disabled = true;
  root.dataset.status = "error";
}

async function fetchBytes(name: string, signal: AbortSignal): Promise<ArrayBuffer> {
  const response = await fetch(new URL(name, document.baseURI), { signal });
  check(response.ok, `${name}: HTTP ${response.status}`);
  const reader = response.body?.getReader(); check(reader, `${name}: empty response`);
  const chunks: Uint8Array[] = []; let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read(); if (done) break;
      size += value.length; limit("max_output_bytes", size, DEFAULT_LIMITS.max_output_bytes); chunks.push(value);
    }
  } catch (error) { await reader.cancel(); throw error; }
  const bytes = new Uint8Array(size); let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  return bytes.buffer;
}
export async function loadPreview(root: HTMLElement): Promise<PreviewApp | null> {
  const abort = new AbortController();
  const cancel = () => abort.abort(); window.addEventListener("pagehide", cancel, { once: true });
  try {
    const [bytes, manifest] = await Promise.all([fetchBytes("preview.glb", abort.signal), fetchBytes("preview.manifest.json", abort.signal)]);
    let metadata: unknown;
    try { metadata = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(manifest)); }
    catch { throw new PreviewDataError("preview.manifest.json is not valid UTF-8 JSON"); }
    return createPreviewApp(root, bytes, metadata);
  } catch (error) { if (!abort.signal.aborted) showError(root, error); abort.abort(); return null; }
  finally { window.removeEventListener("pagehide", cancel); }
}
