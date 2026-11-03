// Opening clones in an editor: the installed editors (looked up once per run), which one a
// clone opens in, and the launch with failures reported in a dialog. Rust remembers the
// editor each clone was opened in from here and sends it back as the project's `lastEditor`.
import { CheckMenuItem, Menu } from "@tauri-apps/api/menu";
import { message } from "@tauri-apps/plugin-dialog";
import { api, errorMessage, withDialog, type EditorInfo, type Project } from "#lib/api.js";
import { t } from "#lib/i18n/index.js";
import { popupMenu } from "#lib/keyboard.js";
import { saveSettings, settings } from "#lib/settings.svelte.js";

export const editors = $state<{ installed: EditorInfo[]; loaded: boolean }>({
  installed: [],
  loaded: false,
});

let loading: Promise<void> | null = null;

/** Looks the installed editors up once; `force` checks again (after an install). */
export function loadEditors(force = false): Promise<void> {
  if (!loading || force) {
    loading = api.installedEditors().then((list) => {
      editors.installed = list;
      editors.loaded = true;
    });
  }
  return loading;
}

function installed(id: string | null | undefined): EditorInfo | null {
  return editors.installed.find((editor) => editor.id === id) ?? null;
}

/** The editor new clones open in: the chosen default, else the first one installed. */
export function defaultEditor(): EditorInfo | null {
  return installed(settings.current?.defaultEditor) ?? editors.installed[0] ?? null;
}

/** The editor the clone opens in: the one last picked for it in the app, else the one the
 *  editors' recent lists say opened it, each while installed, else the default. */
export function editorFor(project: Project): EditorInfo | null {
  return installed(project.lastEditor) ?? installed(project.editor) ?? defaultEditor();
}

export async function openInEditor(path: string, editor: EditorInfo): Promise<void> {
  try {
    await api.openInEditor(path, editor.id);
  } catch (err) {
    const text = t("editor.failed", { editor: editor.name, error: errorMessage(err) });
    await withDialog(() => message(text, { kind: "error" }));
  }
}

/** A native menu of the installed editors, the default one checked; picking one saves it. */
export async function chooseDefaultEditor(anchor?: Element | null): Promise<void> {
  const current = defaultEditor();
  const items = await Promise.all(
    editors.installed.map((other) =>
      CheckMenuItem.new({
        text: other.name,
        checked: other.id === current?.id,
        action: () => void saveSettings({ defaultEditor: other.id }),
      }),
    ),
  );
  const menu = await Menu.new({ items });
  await popupMenu(menu, anchor);
}
