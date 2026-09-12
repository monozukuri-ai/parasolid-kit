import type { Display, Viewer } from "three-cad-viewer";
import type { PrimitiveMetadata } from "./manifest.ts";
import { check, PreviewDataError } from "./validation.ts";

// These paths are verified against the *distributed* 5.0.6 declarations. Do not replace
// backendId with the numeric `selected` notification: local indices collide across bodies/kinds.
type Component = Viewer["cadTools"]["selectObject"]["selectedShapes"][number];
export interface SelectionHost {
  onAfterRender: Viewer["onAfterRender"];
  cadTools: { selectObject: { selectedShapes: readonly Pick<Component, "backendId" | "topo" | "fromSolid">[] } };
}
export interface SourceSelection {
  readonly backendId: string;
  readonly primitive: PrimitiveMetadata;
}

export function bindSelection(
  viewer: SelectionHost,
  sourceMap: ReadonlyMap<string, PrimitiveMetadata>,
  onChange: (selection: readonly SourceSelection[]) => void,
  onError: (error: PreviewDataError) => void,
): () => void {
  const previous = viewer.onAfterRender;
  let lastIds: string[] | undefined, lastError: string | undefined, active = true;
  const afterRender = (): void => {
    previous?.call(viewer);
    if (!active) return;
    const selected = viewer.cadTools.selectObject.selectedShapes;
    let next: SourceSelection[];
    try {
      const seen = new Set<string>();
      next = selected.map(s => {
        const primitive = sourceMap.get(s.backendId);
        check(!s.fromSolid && primitive && primitive.kind === s.topo && !seen.has(s.backendId), `Unmapped selection: ${s.backendId} (${s.topo})`);
        seen.add(s.backendId);
        return Object.freeze({ backendId: s.backendId, primitive });
      });
    } catch (error) {
      if (!(error instanceof PreviewDataError)) throw error;
      if (lastError !== error.message) {
        lastError = error.message; lastIds = undefined;
        onChange(Object.freeze([])); onError(error);
      }
      return;
    }
    const ids = next.map(s => s.backendId);
    if (lastError || !lastIds || ids.length !== lastIds.length || ids.some((id, i) => id !== lastIds![i])) {
      lastError = undefined; lastIds = ids; onChange(Object.freeze(next));
    }
  };
  viewer.onAfterRender = afterRender;
  return () => {
    active = false;
    if (viewer.onAfterRender === afterRender) viewer.onAfterRender = previous;
  };
}

/** The pinned picker's public typed filter field; detach its broader vertex/solid keyboard menu. */
export function setSelectionKind(display: Display, viewer: Viewer, kind: "face" | "edge"): void {
  display.shapeFilterDropDownMenu.show(false);
  display.shapeFilterDropDownMenu.currentFilter = [kind];
  viewer.update(true, false);
}
