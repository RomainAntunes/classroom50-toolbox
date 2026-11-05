// The phases editor drawn inside the popover (`PhasesEditor.svelte`), opened from a row's
// menu. It saves through `set_phases` itself, so a refusal shows in it with the edits kept.

import type { Project } from "#lib/api.js";

export const phasesEditor = $state<{
  current: { project: Project; resolve: (saved: boolean) => void } | null;
}>({ current: null });

/** Opens the editor on `project`'s phases; resolves to whether they were saved. */
export function editPhases(project: Project): Promise<boolean> {
  phasesEditor.current?.resolve(false);
  return new Promise((resolve) => {
    phasesEditor.current = {
      project,
      resolve: (saved) => {
        phasesEditor.current = null;
        resolve(saved);
      },
    };
  });
}
