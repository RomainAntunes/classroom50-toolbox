// Giving a classmate push access: `gh student team add` on a team assignment (they join the
// group), `gh student invite` otherwise. Asks for the username in the popover first.
import { message } from "@tauri-apps/plugin-dialog";
import { api, errorMessage, withDialog, type Project } from "$lib/api";
import { t } from "$lib/i18n";
import { askText } from "$lib/prompt.svelte";

/** Whether adding goes through the group (team assignments) rather than an invitation. */
export function joinsGroup(project: Project): boolean {
  return Boolean(project.group) || project.assignment?.mode === "team";
}

export async function addTeammate(project: Project, name: string): Promise<void> {
  const team = joinsGroup(project);
  const repo = `${project.owner}/${project.repo}`;
  const username = await askText({
    title: t(team ? "teammate.teamTitle" : "teammate.inviteTitle"),
    message: t(team ? "teammate.teamHint" : "teammate.inviteHint", { assignment: name, repo }),
    placeholder: t("teammate.placeholder"),
    okLabel: t(team ? "teammate.teamOk" : "teammate.inviteOk"),
  });
  if (!username) return;
  const user = username.replace(/^@/, "");
  try {
    await api.addTeammate(project.id, user);
    const text = t(team ? "teammate.teamDone" : "teammate.inviteDone", {
      user,
      assignment: name,
      repo,
    });
    await withDialog(() => message(text, { kind: "info" }));
  } catch (err) {
    const text = t("teammate.failed", { user, error: errorMessage(err) });
    await withDialog(() => message(text, { kind: "error" }));
  }
}
