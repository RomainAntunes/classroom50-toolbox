// Hidden assignments and classes: left out of the list unless a search matches them, and never
// notified about (Rust filters them out of the alerts). Ids are project or unaccepted-assignment
// ids; classes go by `classKey`.
import { saveSettings, settings } from "$lib/settings.svelte";

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

export async function setClassHidden(key: string, hide: boolean): Promise<void> {
  const rest = settings.current?.hiddenClasses.filter((other) => other !== key) ?? [];
  await saveSettings({ hiddenClasses: hide ? [...rest, key] : rest });
}

// Folded classes show their header only.
export function isCollapsed(key: string): boolean {
  return settings.current?.collapsedClasses.includes(key) ?? false;
}

export async function setCollapsed(key: string, collapse: boolean): Promise<void> {
  const rest = settings.current?.collapsedClasses.filter((other) => other !== key) ?? [];
  await saveSettings({ collapsedClasses: collapse ? [...rest, key] : rest });
}
