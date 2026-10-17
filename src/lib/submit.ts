// Submitting a project from the list. `gh student submit` pushes the clone's files as they
// are (not its commits) on top of GitHub's head and asks nothing, so the app only submits a
// clean clone (Rust refuses otherwise too) and says everything that could surprise the
// student before it runs. A clone without `.classroom50.yaml` submits the same way: gh-student
// finds its assignment from the repo's name.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  api,
  errorKind,
  errorMessage,
  withDialog,
  type CommandError,
  type Project,
  type SubmitCheck,
} from "$lib/api";
import { t } from "$lib/i18n";
import { announce } from "$lib/announce.svelte";
import { fileList } from "$lib/files";
import { commitLocal } from "$lib/push";
import { session } from "$lib/session.svelte";
import { settings } from "$lib/settings.svelte";

async function show(text: string, title: string, kind: "info" | "error" = "info") {
  await withDialog(() => message(text, { title, kind }));
}

/** Submit sends committed work only: says so, and offers Commit when the `commitActions`
 *  setting lets the app commit (else the editor or a terminal does). Never submits itself:
 *  the row offers Submit again once the clone is clean. */
async function commitFirst(project: Project, name: string, files: string[]) {
  const what = t("submit.uncommitted", { files: fileList(files) });
  if ((settings.current?.commitActions ?? "none") === "none") {
    await show(`${what}\n\n${t("submit.commitElsewhere")}`, name);
    return;
  }
  const commit = await withDialog(() =>
    ask(`${what}\n\n${t("submit.commitFirst")}`, {
      title: name,
      kind: "info",
      okLabel: t("commit.menu"),
      cancelLabel: t("accept.cancel"),
    }),
  );
  if (commit) await commitLocal(project, name);
}

/** A clone without a marker needs a gh-student that finds assignments by name: offers the
 *  update the Setup section and Settings run (`upgradeTool`). True once it's new enough. */
async function updateGhStudent(err: CommandError, name: string): Promise<boolean> {
  const text = t("submit.updateGhStudent", {
    version: err.params?.version ?? "",
    min: err.params?.min ?? "",
  });
  const update = await withDialog(() =>
    ask(text, {
      title: name,
      kind: "info",
      okLabel: t("setup.update"),
      cancelLabel: t("accept.cancel"),
    }),
  );
  if (!update) return false;
  const tool = t("setup.tool.gh-student");
  try {
    const next = await api.upgradeTool("gh-student");
    session.tools = session.tools.map((status) => (status.tool === next.tool ? next : status));
    announce(t("a11y.toolUpdated", { tool }));
    return !next.outdated;
  } catch (failure) {
    const kind = errorKind(failure);
    if (kind === "cancelled") return false;
    let text = t("setup.updateFailed", {
      tool,
      error: kind === "notSignedIn" ? t("setup.signInFirst") : errorMessage(failure),
    });
    if (kind !== "busy" && kind !== "notSignedIn") {
      text += t("setup.runYourself", { command: "gh extension upgrade gh-student" });
    }
    await show(text, name, "error");
    return false;
  }
}

/** Confirms, submits and reports. Returns true when the submission went through. */
export async function submitProject(project: Project, name: string): Promise<boolean> {
  try {
    let check: SubmitCheck;
    try {
      check = await api.submitCheck(project.id);
    } catch (err) {
      // No marker: gh-student finds the assignment from the repo's name, which needs the
      // app to know it too, and a recent enough gh-student (then the check runs again).
      if (errorKind(err) === "unknownAssignment") {
        await show(t("submit.noMarker"), name);
        return false;
      }
      if (errorKind(err) === "ghStudentOutdated") {
        return (await updateGhStudent(err as CommandError, name)) && submitProject(project, name);
      }
      throw err;
    }
    if (check.uncommitted.length) {
      await commitFirst(project, name, check.uncommitted);
      return false;
    }
    const local = project.local;
    const remote = project.remote;
    const lines = [
      t("submit.confirm", {
        folder: project.path ?? "",
        branch: remote?.defaultBranch ?? "main",
      }),
    ];
    if (check.editorFiles.length) {
      lines.push(t("submit.editorFiles", { files: fileList(check.editorFiles) }));
    }
    if (local?.remoteAhead) {
      lines.push(
        remote?.head?.author
          ? t("submit.remoteAheadBy", { author: remote.head.author })
          : t("submit.remoteAhead"),
      );
    } else if (local && local.head === remote?.head?.oid) {
      lines.push(t("submit.unchanged"));
    }
    const due = project.assignment?.due ? new Date(project.assignment.due) : null;
    if (due && due < new Date()) lines.push(t("submit.late"));
    const confirmed = await withDialog(() =>
      ask(lines.join("\n\n"), {
        title: name,
        kind: local?.remoteAhead ? "warning" : "info",
        okLabel: t("submit.ok"),
        cancelLabel: t("accept.cancel"),
      }),
    );
    if (!confirmed) return false;
    const { url } = await api.submitProject(project.id);
    announce(t("a11y.submitted", { name }));
    if (!url) {
      await show(t("submit.done"), name);
      return true;
    }
    const view = await withDialog(() =>
      ask(t("submit.done"), {
        title: name,
        kind: "info",
        okLabel: t("submit.view"),
        cancelLabel: t("submit.close"),
      }),
    );
    if (view) await openUrl(url);
    return true;
  } catch (err) {
    await show(t("submit.failed", { error: errorMessage(err) }), name, "error");
    return false;
  }
}
