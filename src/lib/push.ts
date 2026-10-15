// Commit, and Commit & push for every-push assignments, where each push to the default branch
// is graded. Rust commits every change with the student's message (any branch, any clone) and,
// when chosen, pushes the default branch, refusing anything that would need a merge; the
// refusals are explained here.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { api, errorKind, errorMessage, withDialog, type Project } from "$lib/api";
import { t } from "$lib/i18n";
import { askTextAction } from "$lib/prompt.svelte";
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

/** Commit & push (row button, menu): pushing is the main action, "Commit only" the second. */
export function commitPush(
  project: Project,
  name: string,
  onChoice?: (push: boolean) => void,
): Promise<boolean> {
  return commitWork(project, name, true, onChoice);
}

/** Commit (menu, any clone): committing is the main action, Commit & push the second when it
 *  applies. */
export function commitLocal(
  project: Project,
  name: string,
  onChoice?: (push: boolean) => void,
): Promise<boolean> {
  return commitWork(project, name, false, onChoice);
}

/** Why push isn't offered next to Commit, in one line. */
function noPushLine(reason: string, branch: string): string {
  const known = ["tagMode", "closed", "otherBranch", "remoteAhead", "notOnGitHub"];
  return t(`commit.noPush.${known.includes(reason) ? reason : "fallback"}`, { branch });
}

/** Asks for the message, then commits, and pushes when chosen. `onChoice` hears which right
 *  before the write, for the row's status. Returns true when it went through. */
async function commitWork(
  project: Project,
  name: string,
  preferPush: boolean,
  onChoice?: (push: boolean) => void,
): Promise<boolean> {
  const branch = project.remote?.defaultBranch ?? project.local?.branch ?? "main";
  let pushing = preferPush;
  try {
    let check;
    try {
      check = await api.commitCheck(project.id);
    } catch (err) {
      // Nothing to commit: Commit & push may still have commits to push.
      if (preferPush && errorKind(err) === "nothingToCommit") {
        return await pushOnly(project, name, branch, onChoice);
      }
      throw err;
    }
    if (check.tooLarge.length) {
      await show(t("push.fileTooLarge", { files: fileList(check.tooLarge) }), name, "error");
      return false;
    }
    if (!check.hasIdentity && !(await setIdentity(project, name))) return false;
    const canPush = check.pushBlocked === null;
    const lines = [t("push.files", { count: check.files.length, files: fileList(check.files) })];
    if (check.untracked.length) {
      lines.push(t("push.untracked", { files: fileList(check.untracked) }));
    }
    if (check.large.length) lines.push(t("push.large", { files: fileList(check.large) }));
    if (canPush && check.ahead) lines.push(t("push.ahead", { count: check.ahead }));
    if (!canPush) lines.push(noPushLine(check.pushBlocked ?? "", branch));
    const pushFirst = canPush && preferPush;
    const answer = await askTextAction({
      title: t(pushFirst ? "push.title" : "commit.title"),
      message: lines.join("\n"),
      placeholder: t("push.placeholder"),
      okLabel: t(pushFirst ? "push.ok" : "commit.ok"),
      altLabel: canPush ? t(pushFirst ? "commit.only" : "push.ok") : undefined,
    });
    if (!answer) return false;
    pushing = canPush && answer.alt !== preferPush;
    if (pushing) {
      if (late(project) && !(await confirm(t("push.lateConfirm"), name, t("push.continue")))) {
        return false;
      }
      onChoice?.(true);
      await api.commitPush(project.id, answer.text);
    } else {
      onChoice?.(false);
      await api.commitOnly(project.id, answer.text);
    }
    return true;
  } catch (err) {
    await refused(err, name, branch, pushing);
    return false;
  }
}

function late(project: Project): boolean {
  const due = project.assignment?.due ? new Date(project.assignment.due) : null;
  return due !== null && due < new Date();
}

/** No changes, only local commits GitHub lacks: one confirm, then the push. */
async function pushOnly(
  project: Project,
  name: string,
  branch: string,
  onChoice?: (push: boolean) => void,
): Promise<boolean> {
  const check = await api.pushCheck(project.id);
  const lines = [t("push.pushOnly", { count: check.ahead, branch })];
  if (late(project)) lines.push(t("push.late"));
  if (!(await confirm(lines.join("\n\n"), name, t("push.pushOk")))) return false;
  onChoice?.(true);
  await api.commitPush(project.id, null);
  return true;
}

async function refused(err: unknown, name: string, branch: string, pushing: boolean) {
  const kind = errorKind(err) ?? "";
  const text =
    kind === "nothingToCommit"
      ? t("commit.nothingToCommit")
      : REASONS.includes(kind)
        ? t(`push.${kind}`, { branch })
        : t(pushing ? "push.failed" : "commit.failed", { error: errorMessage(err) });
  await show(text, name, "warning");
}
