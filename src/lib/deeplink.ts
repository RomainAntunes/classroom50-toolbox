// Acting on a `classroom50-toolbox://` link. Rust has validated it and worked out where it
// leads; this checks the app is ready (set up, tools, signed in), shows the row and asks
// before anything happens. Any web page can fire a link, so nothing runs unconfirmed.
import { tick } from "svelte";
import { goto } from "$app/navigation";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { api, errorMessage, withDialog, type DeepLink, type DeepLinkTarget } from "#lib/api.js";
import { acceptAssignment, acceptBlocked } from "#lib/accept.js";
import { cloneProject } from "#lib/clone.js";
import { t } from "#lib/i18n/index.js";
import { loadSnapshot, session } from "#lib/session.svelte.js";
import { loadSettings } from "#lib/settings.svelte.js";

async function tell(text: string, kind: "info" | "warning" | "error" = "info"): Promise<void> {
  await withDialog(() => message(text, { title: t("link.title"), kind }));
}

/** Whether the app can act on `target`; otherwise says what is missing and shows where. */
async function ready(target: DeepLinkTarget): Promise<boolean> {
  if (!(await loadSettings()).onboarded) {
    await tell(t("link.finishSetup"));
    return false;
  }
  const accepting = target.kind === "accept" || target.kind === "unlisted";
  const tools = await api.toolStatuses();
  session.tools = tools;
  const needed = accepting ? ["gh", "gh-student"] : ["gh"];
  if (tools.some((tool) => needed.includes(tool.tool) && !tool.installed)) {
    await goto("/");
    await tell(t("link.needsTools"), "warning");
    return false;
  }
  const auth = await api.authStatus();
  session.auth = auth;
  if (!auth.loggedIn) {
    await goto("/");
    await tell(t("link.needsSignIn"), "warning");
    return false;
  }
  return true;
}

let seq = 0;

/** Shows row `id` on the main page: unfolded, scrolled to and opened. */
async function focus(id: string): Promise<void> {
  await goto("/");
  await loadSnapshot();
  session.focus = { id, seq: ++seq };
  await tick();
}

function missing(link: DeepLink): string {
  return link.kind === "open"
    ? t("link.noProject", { repo: `${link.owner}/${link.repo}` })
    : t("link.noAssignment", { assignment: link.assignment, classroom: link.classroom });
}

async function act(target: DeepLinkTarget): Promise<void> {
  switch (target.kind) {
    case "unlisted": {
      // Nothing was requested yet: the link's own names are all there is to show.
      const { link } = target;
      if (link.kind !== "accept") return;
      await goto("/");
      const open = await withDialog(() =>
        ask(
          t("link.openAssignment", {
            assignment: link.assignment,
            classroom: `${link.org}/${link.classroom}`,
          }),
          {
            title: t("link.title"),
            kind: "info",
            okLabel: t("link.open"),
            cancelLabel: t("accept.cancel"),
          },
        ),
      );
      if (!open) return;
      let fetched: DeepLinkTarget | null;
      try {
        fetched = await api.fetchLinkedAssignment();
      } catch (err) {
        await tell(t("link.failed", { error: errorMessage(err) }), "error");
        return;
      }
      if (fetched && fetched.kind !== "unlisted") await act(fetched);
      return;
    }
    case "missing":
      await goto("/");
      await tell(missing(target.link));
      return;
    case "project": {
      await focus(target.id);
      const project = session.snapshot?.projects.find((p) => p.id === target.id);
      if (!project || project.path || !project.owner || !project.repo) return;
      const repo = `${project.owner}/${project.repo}`;
      await cloneProject(project.id, "ask", {
        title: project.assignment?.name ?? repo,
        message: (folder) =>
          folder ? t("link.cloneNowTo", { repo, folder }) : t("link.cloneNow", { repo }),
        cancelLabel: t("accept.later"),
      });
      return;
    }
    case "accept": {
      const { item, listed } = target;
      if (listed) await focus(item.id);
      else await goto("/");
      const blocked = acceptBlocked(item, new Date());
      if (blocked) {
        await tell(`${item.assignment.name}: ${blocked}`);
        return;
      }
      // Asks first, like the row's button.
      await acceptAssignment(item);
    }
  }
}

let running = false;
let again = false;
// A link is being acted on (taken, not yet done): pages leave navigation to it.
let active = false;

/** Whether a link is being handled, so the What's new page doesn't take its place. */
export function linkActive(): boolean {
  return active;
}

/**
 * Takes the waiting link, if any, and acts on it. Rust drops other links until
 * `deepLinkDone`; one let through just after is taken by the next round.
 */
export async function handleDeepLink(): Promise<void> {
  if (running) {
    again = true;
    return;
  }
  running = true;
  try {
    do {
      again = false;
      const target = await api.takeDeepLink();
      if (!target) continue;
      active = true;
      try {
        if (await ready(target)) await act(target);
      } finally {
        active = false;
        await api.deepLinkDone();
      }
    } while (again);
  } finally {
    running = false;
  }
}
