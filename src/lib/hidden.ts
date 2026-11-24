// Hidden assignments and classes: left out of the list unless a search matches them, and never
// notified about (Rust filters them out of the alerts). Ids are project or unaccepted-assignment
// ids; classes go by `classKey`.
import { saveSettings, settings } from "#lib/settings.svelte.js";

export function isHidden(id: string): boolean {
  return settings.current?.hidden.includes(id) ?? false;
}

export async function setHidden(id: string, hide: boolean): Promise<void> {
  const rest = settings.current?.hidden.filter((other) => other !== id) ?? [];
  await saveSettings({ hidden: hide ? [...rest, id] : rest });
}

export function isClassHidden(key: string): boolean {
  return settings.current?.hiddenClasses.includes(key) ?? false;
}

/** Hides or shows the classes `keys` names: one section can hold a Classroom 50 class and
 *  the projects added by hand under its name, each with its own key. */
export async function setClassHidden(keys: string | string[], hide: boolean): Promise<void> {
  const list = typeof keys === "string" ? [keys] : keys;
  const rest = settings.current?.hiddenClasses.filter((other) => !list.includes(other)) ?? [];
  await saveSettings({ hiddenClasses: hide ? [...rest, ...list] : rest });
}

// Folded classes show their header only.
export function isCollapsed(key: string): boolean {
  return settings.current?.collapsedClasses.includes(key) ?? false;
}

export async function setCollapsed(key: string, collapse: boolean): Promise<void> {
  const rest = settings.current?.collapsedClasses.filter((other) => other !== key) ?? [];
  await saveSettings({ collapsedClasses: collapse ? [...rest, key] : rest });
}
