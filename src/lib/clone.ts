// Cloning a project from the list: asks first, naming the folder the clone goes to (the
// clone folder, picked once and remembered in settings), runs the clone and reports failures
// in a dialog.
import { ask, message, open } from "@tauri-apps/plugin-dialog";
import { api, errorKind, errorMessage, withDialog } from "$lib/api";
import { t } from "$lib/i18n";
import { announce } from "$lib/announce.svelte";
import { defaultEditor, loadEditors, openInEditor } from "$lib/editor.svelte";
import { tildify } from "$lib/paths.svelte";
import { loadSettings, saveSettings, settings } from "$lib/settings.svelte";
import { busy, session } from "$lib/session.svelte";

/** How a clone starts: `ask` names the destination first, `choose` picks another folder. */
export type CloneMode = "ask" | "choose";

/** The words of the question `ask` shows, for callers that already had a question to ask
 *  (after an accept, from a link). */
export interface CloneQuestion {
  title: string;
  /** The question, given the destination as people read it, or null when no clone folder
   *  is set yet (the folder is then picked after a yes). */
  message: (target: string | null) => string;
  cancelLabel: string;
}

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

/** The folder Rust clones `repo` into (`clones::clone`: the parent joined with the repo
 *  name), spelled with the parent's own separator. */
export function cloneTarget(parent: string, repo: string): string {
  const separator = parent.includes("\\") && !parent.includes("/") ? "\\" : "/";
  return `${parent.replace(/[\\/]+$/, "")}${separator}${repo}`;
}

/**
 * Asks before cloning into `<cloneFolder>/<repo>`: Clone, Choose another folder… (picked and
 * remembered) or Cancel. With no clone folder yet the folder picker comes first, then this
 * question naming the destination; a caller's own question (already a yes to cloning) is
 * asked before the picker instead. Resolves to the parent folder, or null when cancelled.
 */
async function askParent(repo: string, question?: CloneQuestion): Promise<string | null> {
  let saved = await cloneFolder();
  if (!saved) {
    if (question) {
      const yes = await withDialog(() =>
        ask(question.message(null), {
          title: question.title,
          kind: "info",
          okLabel: t("projects.clone"),
          cancelLabel: question.cancelLabel,
        }),
      );
      if (!yes) return null;
      return pickCloneFolder();
    }
    saved = await pickCloneFolder();
    if (!saved) return null;
  }
  const target = tildify(cloneTarget(saved, repo));
  const yes = t("projects.clone");
  const no = t("clone.chooseOther");
  const answer = await withDialog(() =>
    message(question?.message(target) ?? t("clone.ask", { folder: target }), {
      title: question?.title ?? repo,
      kind: "info",
      buttons: { yes, no, cancel: question?.cancelLabel ?? t("clone.cancel") },
    }),
  );
  // Custom buttons resolve to their label on every OS; anything else (Escape) is a cancel.
  if (answer === yes) return saved;
  if (answer === no) return pickCloneFolder();
  return null;
}

/**
 * Clones project `id`, asking first (`ask`, worded by `question` when given) or picking
 * another folder (`choose`, the menu's "Clone to…"), then opens it in the default editor.
 * `repo` names the folder when the list doesn't hold the project yet. Returns the new folder,
 * or null when cancelled or failed. The row shows the clone (not the question) meanwhile.
 */
export async function cloneProject(
  id: string,
  mode: CloneMode = "ask",
  question?: CloneQuestion,
  repo?: string,
): Promise<string | null> {
  const name = session.snapshot?.projects.find((p) => p.id === id)?.repo ?? repo ?? id;
  const parent = mode === "choose" ? await pickCloneFolder() : await askParent(name, question);
  if (!parent || session.cloning.includes(id)) return null;
  return busy("cloning", id, () => clone(id, parent));
}

async function clone(id: string, parent: string): Promise<string | null> {
  try {
    const dir = await api.cloneProject(id, parent);
    announce(t("a11y.cloned", { folder: dir.split(/[\\/]/).pop() ?? dir }));
    await loadEditors();
    const editor = defaultEditor();
    if (editor) await openInEditor(dir, editor);
    return dir;
  } catch (err) {
    const key = errorKind(err) === "pathExists" ? "clone.exists" : "clone.failed";
    const text = t(key, { error: errorMessage(err) });
    await withDialog(() => message(text, { kind: "error" }));
    return null;
  }
}
