// What a row shows about its actions, read at every render. The flows themselves (`accept`,
// `clone`, `pull`, `push`, `submit`, `teammate`) only run after a click, so the rows load
// them with `import()` the first time; these rules stay here, out of those chunks, so the
// list doesn't load them at startup.
import type { Project, Unaccepted } from "#lib/api.js";
import { t } from "#lib/i18n/index.js";

/** Every push to the default branch is graded (no `submit/*` tags): Commit & push applies. */
export function gradedOnPush(project: Project): boolean {
  // A project added by hand has nothing to submit: its pushes are the work.
  if (project.manual) return true;
  return Boolean(project.assignment) && project.assignment?.submissionMode !== "tag";
}

/** Adding a classmate joins a group (`team add`) rather than inviting a collaborator. */
export function joinsGroup(project: Project): boolean {
  return Boolean(project.group) || project.assignment?.mode === "team";
}

/** Why the row can't accept this assignment now (locked, or not open yet); null when it can. */
export function acceptBlocked(item: Unaccepted, now: Date): string | null {
  if (item.assignment.locked) return t("projects.lockedHint");
  const opens = item.assignment.availableFrom ? new Date(item.assignment.availableFrom) : null;
  return opens !== null && opens > now ? t("projects.upcomingHint") : null;
}

/** A student-formed team assignment with no group yet: "Create a group…" applies. */
export function canFoundGroup(item: Unaccepted): boolean {
  const a = item.assignment;
  return a.mode === "team" && a.teamFormation !== "teacher" && !item.group;
}
