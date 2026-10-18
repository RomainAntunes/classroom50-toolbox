// Giving classmates push access: `gh student team add` on a team assignment (they join the
// group), `gh student invite` otherwise. Asks for the usernames in the popover first, adds
// them one by one and offers to add more.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { api, errorMessage, withDialog, type Added, type Group, type Project } from "#lib/api.js";
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
interface Target {
  /** A project id, or the assignment of a group just founded. */
  id: string;
  /** `owner/repo`. */
  repo: string;
  /** The assignment's name. */
  assignment: string;
  /** Joins the group rather than inviting. */
  team: boolean;
}

/** "Add to my group…" / "Invite a classmate…" from a row's menu. */
export async function addTeammate(project: Project, name: string): Promise<void> {
  const team = joinsGroup(project);
  if (team) {
    const reason = notFounderReason(
      project.group,
      session.snapshot?.login ?? null,
      project.assignment?.teamFormation === "teacher",
    );
    if (reason) {
      await withDialog(() => message(reason, { title: t("teammate.teamTitle"), kind: "info" }));
      return;
    }
  }
  await addTeammates({
    id: project.id,
    repo: `${project.owner}/${project.repo}`,
    assignment: name,
    team,
  });
}

/** Right after founding a group: offers to add teammates before anything else. */
export function addToNewGroup(id: string, repo: string, assignment: string): Promise<void> {
  return addTeammates({ id, repo, assignment, team: true });
}

/** Asks for usernames, adds them, shows each result, and offers to add more until the
 *  student is done or cancels. */
async function addTeammates(target: Target): Promise<void> {
  const { team, repo, assignment } = target;
  const title = t(team ? "teammate.teamTitle" : "teammate.inviteTitle");
  for (;;) {
    const text = await askText({
      title,
      message: t(team ? "teammate.teamHint" : "teammate.inviteHint", { assignment, repo }),
      placeholder: t("teammate.placeholder"),
      okLabel: t(team ? "teammate.teamOk" : "teammate.inviteOk"),
    });
    const usernames = text ? parseUsernames(text) : [];
    if (usernames.length === 0) return;
    let results: Added[];
    try {
      results = await api.addTeammates(target.id, usernames);
    } catch (err) {
      const error = { kind: "", message: errorMessage(err) };
      results = usernames.map((username) => ({ username, error }));
    }
    const again = await withDialog(() =>
      ask(resultsText(results, target), {
        title,
        kind: results.every((r) => r.error) ? "error" : "info",
        okLabel: t("teammate.again"),
        cancelLabel: t("teammate.done"),
      }),
    );
    if (!again) return;
  }
}

/** One line per username: added, or why not. */
export function resultsText(results: Added[], target: Omit<Target, "id">): string {
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
