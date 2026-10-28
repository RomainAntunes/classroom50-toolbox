// Installing a downloaded update, from the banner's Restart now or the tray menu's
// "Restart to Update". Rust installs and restarts the app; only a failure comes back.
import { message } from "@tauri-apps/plugin-dialog";
import { api, errorKind, errorMessage, withDialog } from "#lib/api.js";

/** Installs the waiting update and restarts, or shows why it couldn't. A click while an
 *  install already runs (`busy`) says nothing: that one goes on. */
export async function installUpdate(): Promise<void> {
  try {
    await api.installUpdate();
  } catch (err) {
    if (errorKind(err) === "busy") return;
    await withDialog(() => message(errorMessage(err), { kind: "error" }));
  }
}
