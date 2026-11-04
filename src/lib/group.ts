// How a group reads in a row's details: its name, one member per line, you included, and
// its size against the assignment's limit.
import type { Group } from "#lib/api.js";
import { t } from "#lib/i18n/index.js";
import { session } from "#lib/session.svelte.js";

export function groupName(group: Group): string {
  return group.name ?? t("details.groupNumber", { number: group.number });
}

/** Whether `group` has no room left under the assignment's `max` (`maxGroupSize`). */
export function groupFull(group: Group | null, max: number | null): boolean {
  return group !== null && max !== null && group.members.length >= max;
}

/** "2 of 3 members", and a line more for a group past the limit (a founder can pass it on
 *  GitHub); null without a limit. */
export function groupSize(group: Group, max: number | null): string | null {
  if (max === null) return null;
  const count = group.members.length;
  const size = t("details.groupSizeValue", { count, max });
  return count > max ? `${size}\n${t("details.groupOversized")}` : size;
}

export function groupDetails(group: Group, max: number | null = null): [string, string][] {
  const me = session.snapshot?.login?.toLowerCase();
  const members = group.members
    .map((m) =>
      m.login.toLowerCase() === me
        ? t("details.you", { name: m.name ?? m.login })
        : (m.name ?? m.login),
    )
    .join("\n");
  const size = groupSize(group, max);
  return [
    [t("details.group"), groupName(group)],
    [t("details.members"), members],
    ...(size ? [[t("details.groupSize"), size] as [string, string]] : []),
  ];
}
