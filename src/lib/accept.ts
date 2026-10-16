// Accepting an assignment from the list. `gh student accept` never asks anything, so the
// confirmations it would need (public repo, groups) are asked here first.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { api, errorKind, errorMessage, withDialog, type Accepted, type Unaccepted } from "$lib/api";
import { cloneProject } from "$lib/clone";
import { formatDate, t } from "$lib/i18n";
import { groupName } from "$lib/group";
import { announce } from "$lib/announce.svelte";
import { busy, session } from "$lib/session.svelte";
import { askText } from "$lib/prompt.svelte";

/** The longest group name, as `student::MAX_TEAM_NAME` (upstream's web app) allows. */
export const MAX_GROUP_NAME = 80;

/** Why `item` can't be accepted from the app now (locked, or not open yet), else null. */
export function acceptBlocked(item: Unaccepted, now: Date): string | null {
  if (item.assignment.locked) return t("projects.lockedHint");
  const opens = item.assignment.availableFrom ? new Date(item.assignment.availableFrom) : null;
  return opens !== null && opens > now ? t("projects.upcomingHint") : null;
}

function confirm(text: string, title: string, okLabel: string): Promise<boolean> {
  return withDialog(() =>
    ask(text, { title, kind: "info", okLabel, cancelLabel: t("accept.cancel") }),
  );
}

/** The repo accept creates: `<org>/<classroom>-<assignment>-<login>`, or `group-<n>` in the
 *  login's place on a team assignment (`<n>` until the group exists). All lowercase. */
export function repoName(item: Unaccepted, login: string | null): string {
  const owner =
    item.assignment.mode === "team" ? `group-${item.group?.number ?? "<n>"}` : (login ?? "<login>");
  const name = `${item.classroom.slug}-${item.assignment.slug}-${owner}`.toLowerCase();
  return `${item.classroom.org}/${name}`;
}

/** What accepting does, one point per line: the repo, who sees and owns it, the Feedback
 *  PR, how submitting works, the due date, groups, and the clone offer that follows. */
export function notes(item: Unaccepted, login: string | null): string {
  const a = item.assignment;
  const repo = repoName(item, login);
  const org = item.classroom.org;
  const points = [
    t(a.repoVisibility === "public" ? "accept.repoPublic" : "accept.repoPrivate", { repo, org }),
  ];
  if (a.feedbackPr) points.push(t("accept.feedbackPr"));
  points.push(t(a.submissionMode === "tag" ? "accept.tagMode" : "accept.everyPush"));
  if (a.due) points.push(t("accept.due", { date: formatDate(new Date(a.due)) }));
  if (a.mode === "group") points.push(t("accept.group"));
  if (a.mode === "team") {
    if (item.group) points.push(t("accept.inGroup", { group: groupName(item.group) }));
    else if (a.teamFormation === "teacher") points.push(t("accept.teamTeacher"));
    else points.push(t("accept.teamStudent"));
  }
  points.push(t("accept.thenClone"));
  const intro = t("accept.confirm", { classroom: item.classroom.name ?? item.classroom.slug });
  return [intro, ...points.map((point) => `• ${point}`)].join("\n");
}

/** Asks the new group's name (required: OK stays disabled while it's blank). Null when
 *  cancelled. */
export function askGroupName(message: string): Promise<string | null> {
  return askText({
    title: t("accept.groupNameTitle"),
    message,
    placeholder: t("accept.groupNamePlaceholder"),
    okLabel: t("accept.newTeam"),
    maxLength: MAX_GROUP_NAME,
  });
}

/** Runs the accept, founding a group when the student names one. Null when cancelled. */
async function run(item: Unaccepted): Promise<Accepted | null> {
  try {
    return await api.acceptAssignment(item.id, null);
  } catch (err) {
    if (errorKind(err) !== "needsTeam") throw err;
  }
  const name = await askGroupName(t("accept.needsTeam"));
  return name ? api.acceptAssignment(item.id, name) : null;
}

/** Asks, accepts, then offers to clone the new repo. The row shows it meanwhile. */
export function acceptAssignment(item: Unaccepted): Promise<void> {
  return busy("accepting", item.id, () => accept(item));
}

async function accept(item: Unaccepted): Promise<void> {
  const name = item.assignment.name;
  if (!(await confirm(notes(item, session.snapshot?.login ?? null), name, t("accept.ok")))) return;
  let accepted: Accepted | null;
  try {
    accepted = await run(item);
  } catch (err) {
    await withDialog(() =>
      message(t("accept.failed", { error: errorMessage(err) }), { title: name, kind: "error" }),
    );
    return;
  }
  if (!accepted) return;
  announce(t("a11y.accepted", { name }));
  const { repo, projectId } = accepted;
  if (!projectId) {
    await withDialog(() => message(t("accept.done", { repo }), { title: name }));
    return;
  }
  await cloneProject(
    projectId,
    "ask",
    {
      title: name,
      message: (folder) =>
        folder ? t("accept.cloneNowTo", { repo, folder }) : t("accept.cloneNow", { repo }),
      cancelLabel: t("accept.later"),
    },
    repo.split("/").pop(),
  );
}
