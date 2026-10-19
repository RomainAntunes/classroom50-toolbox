// Shared settings state for the main and settings pages. Rust is the source of truth:
// every change goes through `update_settings` and the normalized result is stored back.
import { open } from "@tauri-apps/plugin-dialog";
import { api, withDialog, type Settings } from "#lib/api.js";

// Before settings moved to Rust (v0.1.0), watched folders lived in localStorage.
const LEGACY_FOLDERS_KEY = "watchedFolders";

export const settings = $state<{ current: Settings | null }>({ current: null });

export async function loadSettings(): Promise<Settings> {
  let loaded = await api.getSettings();
  const legacy = localStorage.getItem(LEGACY_FOLDERS_KEY);
  if (legacy) {
    const folders: string[] = JSON.parse(legacy);
    loaded = await api.updateSettings({
      ...loaded,
      watchedFolders: [...loaded.watchedFolders, ...folders],
    });
    localStorage.removeItem(LEGACY_FOLDERS_KEY);
  }
  settings.current = loaded;
  return loaded;
}

export async function saveSettings(patch: Partial<Settings>): Promise<void> {
  const base = settings.current ?? (await loadSettings());
  settings.current = await api.updateSettings({ ...base, ...patch });
}

/** Opens the native folder picker; returns true when a new folder was added. */
export async function pickWatchedFolder(): Promise<boolean> {
  const picked = await withDialog(() => open({ directory: true, multiple: false }));
  const folders = settings.current?.watchedFolders ?? [];
  if (typeof picked !== "string" || folders.includes(picked)) return false;
  await saveSettings({ watchedFolders: [...folders, picked] });
  return true;
}
