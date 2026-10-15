// Commit & push for every-push assignments, where each push to the default branch is graded.
// Rust commits every change with the student's message and pushes the default branch, and
// refuses anything that would need a merge; the refusals are explained here.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { api, errorKind, errorMessage, withDialog, type Project } from "$lib/api";
import { t } from "$lib/i18n";
import { askText } from "$lib/prompt.svelte";
import { fileList } from "$lib/files";

const REASONS = [
  "otherBranch",
  "remoteAhead",
  "gitInProgress",
  "nothingToPush",
  "noIdentity",
  "pushRejected",
];

/** Only every-push assignments; tag-mode ones are graded through Submit. */
export function gradedOnPush(project: Project): boolean {
  return Boolean(project.assignment) && project.assignment?.submissionMode !== "tag";
}

async function show(text: string, title: string, kind: "info" | "warning" | "error") {
  await withDialog(() => message(text, { title, kind }));
}

async function confirm(text: string, title: string, okLabel: string): Promise<boolean> {
  return withDialog(() =>
    ask(text, { title, kind: "warning", okLabel, cancelLabel: t("accept.cancel") }),
  );
}

/** Offers to commit as the signed-in account (the clone's own config only). */
async function setIdentity(project: Project, name: string): Promise<boolean> {
  let identity;
  try {
    identity = await api.suggestedGitIdentity();
  } catch {
    await show(t("push.noIdentity"), name, "warning");
    return false;
  }
  const use = await withDialog(() =>
    ask(t("push.identity", { name: identity.name, email: identity.email }), {
      title: name,
      kind: "info",
      okLabel: t("push.identityOk"),
      cancelLabel: t("accept.cancel"),
    }),
  );
  if (!use) return false;
  await api.setGitIdentity(project.id, identity.name, identity.email);
  return true;
}

/** Asks for the message, commits and pushes. Returns true when the push went through. */
export async function commitPush(project: Project, name: string): Promise<boolean> {
  const branch = project.remote?.defaultBranch ?? project.local?.branch ?? "main";
  try {
    const check = await api.pushCheck(project.id);
    if (check.tooLarge.length) {
      await show(t("push.fileTooLarge", { files: fileList(check.tooLarge) }), name, "error");
      return false;
    }
    if (check.files.length && !check.hasIdentity && !(await setIdentity(project, name))) {
      return false;
    }
    const due = project.assignment?.due ? new Date(project.assignment.due) : null;
    const late = due !== null && due < new Date();
    let commitMessage: string | null = null;
    if (check.files.length) {
      if (late && !(await confirm(t("push.lateConfirm"), name, t("push.continue")))) {
        return false;
      }
      const lines = [t("push.files", { count: check.files.length, files: fileList(check.files) })];
      if (check.untracked.length) {
        lines.push(t("push.untracked", { files: fileList(check.untracked) }));
      }
      if (check.large.length) lines.push(t("push.large", { files: fileList(check.large) }));
      if (check.ahead) lines.push(t("push.ahead", { count: check.ahead }));
      commitMessage = await askText({
        title: t("push.title"),
        message: lines.join("\n"),
        placeholder: t("push.placeholder"),
        okLabel: t("push.ok"),
      });
      if (!commitMessage) return false;
    } else {
      const lines = [t("push.pushOnly", { count: check.ahead, branch })];
      if (late) lines.push(t("push.late"));
      if (!(await confirm(lines.join("\n\n"), name, t("push.pushOk")))) return false;
    }
    await api.commitPush(project.id, commitMessage);
    return true;
  } catch (err) {
    const kind = errorKind(err) ?? "";
    const text = REASONS.includes(kind)
      ? t(`push.${kind}`, { branch })
      : t("push.failed", { error: errorMessage(err) });
    await show(text, name, "warning");
    return false;
  }
}
