// Giving classmates push access: `gh student team add` on a team assignment (they join the
// group), `gh student invite` otherwise. Asks for the usernames in the popover, then Rust
// adds them one by one in the background while the row shows each one's progress
// (`teammateJobs.svelte.ts`, `TeammateProgress.svelte`), with "Add another" at hand.
import { message } from "@tauri-apps/plugin-dialog";
import {
  api,
  errorMessage,
  withDialog,
  type Group,
  type Project,
  type TeammateEntry,
  type TeammateJob,
} from "#lib/api.js";
import { groupFull } from "#lib/group.js";
import { formatList, t } from "#lib/i18n/index.js";
import { askText } from "#lib/prompt.svelte.js";
import { session } from "#lib/session.svelte.js";

/** Whether adding goes through the group (team assignments) rather than an invitation. */
export function joinsGroup(project: Project): boolean {
  return Boolean(project.group) || project.assignment?.mode === "team";
}

/** The usernames typed in the prompt: separated by commas, semicolons or spaces, a leading
 *  `@` dropped, each once. Rust checks each is a GitHub login. */
export function parseUsernames(text: string): string[] {
  const seen = new Set<string>();
  const names: string[] = [];
  for (const word of text.split(/[\s,;]+/)) {
    const name = word.replace(/^@+/, "");
    if (!name || seen.has(name.toLowerCase())) continue;
    seen.add(name.toLowerCase());
    names.push(name);
  }
  return names;
}

/** Why the user can't add members to `group` (only its founder, the team's maintainer,
 *  can), or null when they can or it isn't known (roles not read yet). */
export function notFounderReason(
  group: Group | null,
  login: string | null,
  teacherForms: boolean,
): string | null {
  if (!group?.founders || !login) return null;
  const me = login.toLowerCase();
  if (group.founders.some((f) => f.toLowerCase() === me)) return null;
  if (teacherForms) return t("teammate.teacherForms");
  if (group.founders.length === 0) return t("teammate.notFounder");
  return t("teammate.askFounder", { founders: formatList(group.founders) });
}

/** Where classmates are added, and how the prompt words it. */
export interface Target {
  /** A project id, or the assignment of a group just founded. */
  id: string;
  /** `owner/repo`. */
  repo: string;
  /** The assignment's name. */
  assignment: string;
  /** Joins the group rather than inviting. */
  team: boolean;
  /** How many more the group takes, or null with no limit (or not a group). */
  room?: number | null;
}

/** How many more fit under `max`: the members plus the classmates already queued (each
 *  once, the user included), as Rust counts them (`teammates::Cap`). Null without a limit. */
export function roomLeft(
  max: number | null,
  members: string[],
  queued: string[] = [],
): number | null {
  if (max === null) return null;
  const taken = new Set([...members, ...queued].map((login) => login.toLowerCase()));
  return Math.max(0, max - taken.size);
}

/** The lines under the usernames field: how to type several and the room left, then who
 *  the answer adds; a warning when they don't fit. */
export function usernamesHint(
  value: string,
  team: boolean,
  room: number | null,
): { text: string; warning: string | null } {
  const names = parseUsernames(value);
  const full = room === 0 ? t("teammate.noRoom") : null;
  if (names.length === 0) {
    const fits = room === null || full ? null : t("teammate.room", { count: room });
    const text = [t("teammate.separate"), fits].filter((line) => line !== null).join("\n");
    return { text, warning: full };
  }
  const text = t(team ? "teammate.preview.adds" : "teammate.preview.invites", {
    users: formatList(names),
  });
  if (room === null || names.length <= room) return { text, warning: null };
  return { text, warning: full ?? t("teammate.tooMany", { count: room }) };
}

/** "Add to my group…" / "Invite a classmate…" from a row's menu. A full group says so
 *  before asking for anyone (Rust refuses past the limit too, `GroupFull`). */
export async function addTeammate(project: Project, name: string): Promise<void> {
  const team = joinsGroup(project);
  const max = team ? (project.assignment?.maxGroupSize ?? null) : null;
  if (team) {
    const reason =
      max !== null && groupFull(project.group, max)
        ? t("teammate.groupFull", { count: max })
        : notFounderReason(
            project.group,
            session.snapshot?.login ?? null,
            project.assignment?.teamFormation === "teacher",
          );
    if (reason) {
      await withDialog(() => message(reason, { title: t("teammate.teamTitle"), kind: "info" }));
      return;
    }
  }
  const members = project.group?.members.map((m) => m.login) ?? [];
  await addTeammates({
    id: project.id,
    repo: `${project.owner}/${project.repo}`,
    assignment: name,
    team,
    room: roomLeft(max, members.length ? members : [session.snapshot?.login ?? ""]),
  });
}

/** Right after founding a group: offers to add teammates before anything else. */
export function addToNewGroup(id: string, repo: string, assignment: string): Promise<void> {
  return addTeammates({ id, repo, assignment, team: true });
}

/** The row's "Add another": the same prompt, its room counting the classmates the queue
 *  holds (all but those that failed). */
export function addMore(job: TeammateJob): Promise<void> {
  const snapshot = session.snapshot;
  const project = snapshot?.projects.find((p) => p.id === job.project || p.id === job.id);
  const item = project ? null : snapshot?.unaccepted.find((u) => u.id === job.id);
  const max = job.team ? ((project ?? item)?.assignment?.maxGroupSize ?? null) : null;
  const members = project?.group?.members.map((m) => m.login) ?? [];
  const queued = job.entries.filter((e) => e.state !== "failed").map((e) => e.username);
  return addTeammates({
    id: job.id,
    repo: job.repo,
    assignment: job.assignment,
    team: job.team,
    room: roomLeft(max, members.length ? members : [snapshot?.login ?? ""], queued),
  });
}

/** Asks for usernames and queues them; Rust adds them in the background, the row shows
 *  how each one goes. Also the row's "Add another", which joins a queue still running. */
export async function addTeammates(target: Target): Promise<void> {
  const { team, repo, assignment } = target;
  const room = target.room ?? null;
  const title = t(team ? "teammate.teamTitle" : "teammate.inviteTitle");
  const text = await askText({
    title,
    message: t(team ? "teammate.teamHint" : "teammate.inviteHint", { assignment, repo }),
    fieldLabel: t("teammate.usernamesLabel"),
    placeholder: t("teammate.placeholder"),
    hint: (value) => usernamesHint(value, team, room),
    okLabel: t(team ? "teammate.teamOk" : "teammate.inviteOk"),
  });
  const usernames = text ? parseUsernames(text) : [];
  if (usernames.length === 0) return;
  try {
    await api.addTeammates(target.id, usernames);
  } catch (err) {
    // Nothing was queued (the row is gone, say): one line for all of them.
    const user = formatList(usernames);
    await withDialog(() =>
      message(t("teammate.failed", { user, error: errorMessage(err) }), { title, kind: "error" }),
    );
  }
}

/** What adding one classmate gave. */
type Outcome = Pick<TeammateEntry, "username" | "error">;

/** One line per username: added, or why not. */
export function resultsText(results: Outcome[], target: Omit<Target, "id">): string {
  return results
    .map(({ username: user, error }) =>
      error
        ? t("teammate.failed", { user, error: errorMessage(error) })
        : t(target.team ? "teammate.teamDone" : "teammate.inviteDone", {
            user,
            assignment: target.assignment,
            repo: target.repo,
          }),
    )
    .join("\n");
}
