import "three-cad-viewer/css";
export { Display, Viewer } from "three-cad-viewer";
export { adaptPreview } from "./adapter.ts";
export { bindSelection } from "./selection.ts";
export { PreviewDataError, PreviewLimitError, DEFAULT_LIMITS } from "./validation.ts";
export type { AdaptedPreview } from "./adapter.ts";
export type { SourceSelection } from "./selection.ts";
export type { PrimitiveMetadata, SourceEntity } from "./manifest.ts";
export type { PreviewLimits } from "./validation.ts";
import "./style.css";
import { loadPreview } from "./app.ts";
export { createPreviewApp, loadPreview } from "./app.ts";
export type { PreviewApp } from "./app.ts";
const root = document.getElementById("preview-app");
export const application = root ? loadPreview(root) : Promise.resolve(null);
// A page restored from the back/forward cache needs a fresh renderer after pagehide disposal.
if (root) window.addEventListener("pageshow", event => { if (event.persisted) location.reload(); });
