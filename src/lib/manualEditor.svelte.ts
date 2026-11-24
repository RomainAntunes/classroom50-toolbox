// The form for a project the student adds by hand (`ManualEditor.svelte`), drawn inside the
// popover and opened from a class's "..." menu, the empty list or a manual row's menu. It
// saves through `save_manual_project` itself, so a refusal shows in it with the edits kept.

import type { Project } from "#lib/api.js";

/** What the form starts from: a new project (no `id`) or the one being edited. */
export interface ManualInit {
  id: string | null;
  name: string;
  class: string;
  /** ISO date, or null for none. */
  due: string | null;
  notes: string;
  done: boolean;
  path: string | null;
  repo: string;
}

export const manualEditor = $state<{
  current: { init: ManualInit; resolve: (saved: boolean) => void } | null;
}>({ current: null });

/** A manual row's values, for editing it. */
export function manualInitFrom(project: Project): ManualInit {
  return {
    id: project.id,
    name: project.config.assignment,
    class: project.config.classroom,
    due: project.manual?.due ?? null,
    notes: project.manual?.notes ?? "",
    done: project.manual?.done ?? false,
    path: project.path,
    repo: project.manual?.repo ?? "",
  };
}

/** Opens the form, on a new project (with what `start` sets, such as the class) or on the
 *  one `start.id` names; resolves to whether it was saved. */
export function editManual(start: Partial<ManualInit> = {}): Promise<boolean> {
  manualEditor.current?.resolve(false);
  const init: ManualInit = {
    id: null,
    name: "",
    class: "",
    due: null,
    notes: "",
    done: false,
    path: null,
    repo: "",
    ...start,
  };
  return new Promise((resolve) => {
    manualEditor.current = {
      init,
      resolve: (saved) => {
        manualEditor.current = null;
        resolve(saved);
      },
    };
  });
}
