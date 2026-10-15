// Cloning a project from the list: picks the folder (once, then remembered in settings),
// runs the clone and reports failures in a dialog.
import { message, open } from "@tauri-apps/plugin-dialog";
import { api, errorKind, errorMessage, withDialog } from "$lib/api";
import { t } from "$lib/i18n";
import { loadSettings, saveSettings, settings } from "$lib/settings.svelte";

async function cloneFolder(): Promise<string | null> {
  return (settings.current ?? (await loadSettings())).cloneFolder;
}

/** Asks where clones go and remembers the answer. Null when cancelled. */
export async function pickCloneFolder(): Promise<string | null> {
  const current = await cloneFolder();
  const picked = await withDialog(() =>
    open({ directory: true, title: t("clone.pick"), defaultPath: current ?? undefined }),
  );
  if (typeof picked !== "string") return null;
  await saveSettings({ cloneFolder: picked });
  return picked;
}

/**
 * Clones project `id` into the clone folder, asking for one when none is set or `choose`
 * is true. Returns the new folder, or null when cancelled or failed.
 */
export async function cloneProject(id: string, choose = false): Promise<string | null> {
  try {
    const saved = await cloneFolder();
    const parent = choose || !saved ? await pickCloneFolder() : saved;
    if (!parent) return null;
    return await api.cloneProject(id, parent);
  } catch (err) {
    const key = errorKind(err) === "pathExists" ? "clone.exists" : "clone.failed";
    const text = t(key, { error: errorMessage(err) });
    await withDialog(() => message(text, { kind: "error" }));
    return null;
  }
}
