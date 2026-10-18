// Saves a settings change, saying in a dialog when Rust refuses it.
import { message } from "@tauri-apps/plugin-dialog";
import { errorMessage, withDialog } from "#lib/api.js";
import { saveSettings } from "#lib/settings.svelte.js";

export async function save(patch: Parameters<typeof saveSettings>[0]) {
  try {
    await saveSettings(patch);
  } catch (err) {
    await withDialog(() => message(errorMessage(err), { kind: "error" }));
  }
}
