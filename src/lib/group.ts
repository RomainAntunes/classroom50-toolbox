// How a group reads in a row's details: its name and one member per line, you included.
import type { Group } from "$lib/api";
import { t } from "$lib/i18n";
import { session } from "$lib/session.svelte";

export function groupName(group: Group): string {
  return group.name ?? t("details.groupNumber", { number: group.number });
}

export function groupDetails(group: Group): [string, string][] {
  const me = session.snapshot?.login?.toLowerCase();
  const members = group.members
    .map((m) =>
      m.login.toLowerCase() === me
        ? t("details.you", { name: m.name ?? m.login })
        : (m.name ?? m.login),
    )
    .join("\n");
  return [
    [t("details.group"), groupName(group)],
    [t("details.members"), members],
  ];
}
