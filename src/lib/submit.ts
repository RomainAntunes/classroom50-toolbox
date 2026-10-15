// Submitting a project from the list. `gh student submit` pushes the clone's files as they
// are (not its commits) on top of GitHub's head and asks nothing, so everything that could
// surprise the student is said before it runs.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { api, errorMessage, withDialog, type Project } from "$lib/api";
import { t } from "$lib/i18n";

/** At most `max` names, then "and N more". */
function fileList(files: string[], max = 5): string {
  const shown = files.slice(0, max).join(", ");
  return files.length > max
    ? t("submit.andMore", { files: shown, count: files.length - max })
    : shown;
}

async function show(text: string, title: string, kind: "info" | "error" = "info") {
  await withDialog(() => message(text, { title, kind }));
}

/** Confirms, submits and reports. Returns true when the submission went through. */
export async function submitProject(project: Project, name: string): Promise<boolean> {
  try {
    const check = await api.submitCheck(project.id);
    if (!check.hasMarker) {
      await show(t("submit.noMarker"), name);
      return false;
    }
    if (check.deleted.length) {
      await show(t("submit.deleted", { files: fileList(check.deleted) }), name, "error");
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
    if (check.untracked.length) {
      lines.push(t("submit.untracked", { files: fileList(check.untracked) }));
    }
    if (local?.remoteAhead) {
      lines.push(
        remote?.head?.author
          ? t("submit.remoteAheadBy", { author: remote.head.author })
          : t("submit.remoteAhead"),
      );
    } else if (
      local &&
      local.changedFiles === 0 &&
      !check.untracked.length &&
      local.head === remote?.head?.oid
    ) {
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
    await api.submitProject(project.id);
    await show(t("submit.done"), name);
    return true;
  } catch (err) {
    await show(t("submit.failed", { error: errorMessage(err) }), name, "error");
    return false;
  }
}
