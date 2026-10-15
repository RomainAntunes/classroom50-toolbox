// Hidden assignments: left out of the list unless a search matches them, and never notified
// about (Rust filters them out of the alerts). Ids are project or unaccepted-assignment ids.
import { saveSettings, settings } from "$lib/settings.svelte";

export function isHidden(id: string): boolean {
  return settings.current?.hidden.includes(id) ?? false;
}

export async function setHidden(id: string, hide: boolean): Promise<void> {
  const rest = settings.current?.hidden.filter((other) => other !== id) ?? [];
  await saveSettings({ hidden: hide ? [...rest, id] : rest });
}
