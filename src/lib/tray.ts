// The tray icon's menu. The UI computes and words it (Rust has no catalogs) and Rust builds
// it (`tray.rs`), leaving out empty sections. The layout sends it again whenever an input
// or the language changes, and when Rust says its `validUntil` passed (`tray-expired`, on
// the background tick it runs anyway): the UI has no timer, so a label may only change at
// midnight or at a moment known in advance.
import { goto } from "$app/navigation";
import { page } from "$app/state";
import {
  api,
  type Project,
  type Settings,
  type Snapshot,
  type TrayAction,
  type TrayItem,
  type TrayMenu,
  type UpdateStatus,
} from "#lib/api.js";
import { focus } from "#lib/deeplink.js";
import { editorFor, openInEditor } from "#lib/editor.svelte.js";
import { classKey } from "#lib/filter.js";
import { relativeAt, t } from "#lib/i18n/index.js";
import { session } from "#lib/session.svelte.js";

/** What the menu is made of. The layout passes each one narrowed to what the menu reads,
 *  so a settings save or a download's progress that changes nothing here costs nothing. */
export interface TrayInput {
  snapshot: Snapshot | null;
  /** The version a downloaded update waits to install as (`readyVersion`). */
  update: string | null;
  /** Hidden ids and classes (`hiddenFrom(hiddenKey(settings))`). */
  hidden: Hidden;
  /** Whether an editor is installed: Open Recent needs one. */
  editor: boolean;
}

export interface Hidden {
  /** Project and unaccepted-assignment ids. */
  ids: readonly string[];
  /** `classKey`s. */
  classes: readonly string[];
}

/** The hidden lists as one string, for a `$derived` that only moves when they do. */
export function hiddenKey(settings: Settings | null): string {
  return JSON.stringify([settings?.hidden ?? [], settings?.hiddenClasses ?? []]);
}

export function hiddenFrom(key: string): Hidden {
  const [ids, classes] = JSON.parse(key) as [string[], string[]];
  return { ids, classes };
}

/** The version of a downloaded update waiting, null otherwise and once it installs. Its
 *  progress changes nothing, so the menu isn't recomputed during a download. */
export function readyVersion(status: UpdateStatus | null): string | null {
  return status?.state.phase === "ready" && !status.installing ? status.state.version : null;
}

/** The menu for `now`, worded in the UI's language. Pure but for the catalog. */
export function trayMenu(input: TrayInput, now: Date): TrayMenu {
  return {
    open: t("tray.open"),
    quit: t("tray.quit"),
    refresh: t("tray.refresh"),
    settings: t("tray.settings"),
    synced: synced(input, now),
    deadlines: deadlines(input, now),
    recentTitle: t("tray.recent"),
    recent: recent(input),
    update: update(input),
    validUntil: validUntil(input, now),
  };
}

/** "Synced today at 2:32 PM": GitHub's last read, a status check included, as the list's
 *  footer says it, but by the clock, so it holds until midnight. */
function synced({ snapshot }: TrayInput, now: Date): string | null {
  const at = snapshot?.checkedAt ?? snapshot?.syncedAt;
  return at ? t("projects.synced", { when: relativeAt(new Date(at), now, false) }) : null;
}

const DAY = 24 * 60 * 60 * 1000;
/** Deadlines the menu lists at most. */
const DEADLINES = 3;

interface Deadline {
  id: string;
  name: string;
  /** Epoch milliseconds. */
  due: number;
}

/** The project's deadline while it is open, as Rust's `alerts::open_due` reads it. */
function openDue(project: Project): string | null {
  const assignment = project.assignment;
  if (!assignment || assignment.closed || project.remote?.archived) return null;
  return assignment.due;
}

/** Every deadline of open work that isn't hidden, past ones included, in no order: what the
 *  menu picks from, and what `nextChange` watches. */
function candidates({ snapshot, hidden }: TrayInput): Deadline[] {
  const shown = (id: string, owner: string | null, classroom: string) =>
    !hidden.ids.includes(id) && !hidden.classes.includes(classKey(owner, classroom));
  const out: Deadline[] = [];
  for (const project of snapshot?.projects ?? []) {
    const due = openDue(project);
    if (due && project.needsWork && shown(project.id, project.owner, project.config.classroom)) {
      const name = project.assignment?.name ?? project.config.assignment;
      out.push({ id: project.id, name, due: Date.parse(due) });
    }
  }
  for (const item of snapshot?.unaccepted ?? []) {
    const { due, closed, name } = item.assignment;
    if (due && !closed && shown(item.id, item.classroom.org, item.classroom.slug)) {
      out.push({ id: item.id, name, due: Date.parse(due) });
    }
  }
  return out;
}

/** The nearest deadlines of unfinished work, nearest first: "Lab 3 · Due tomorrow at 10:30
 *  PM". One just past shows as "Was due ..." for a day, then drops. */
function deadlines(input: TrayInput, now: Date): TrayItem[] {
  return candidates(input)
    .filter(({ due }) => due > now.getTime() - DAY)
    .sort((a, b) => a.due - b.due)
    .slice(0, DEADLINES)
    .map(({ id, name, due }) => {
      const at = new Date(due);
      const when = relativeAt(at, now, false);
      return {
        id,
        label: t("tray.deadline", {
          name,
          due: t(at > now ? "projects.due" : "projects.wasDue", { when }),
        }),
      };
    });
}

/** How many clones Open Recent lists. */
const RECENT = 5;

/** The clones worked on last, for Open Recent: shown ones only, the latest `.git/index`
 *  change first, and only when an editor can open them. Two picks of the same name are
 *  told apart by their class. */
function recent({ snapshot, hidden, editor }: TrayInput): TrayItem[] {
  if (!editor) return [];
  const picked = (snapshot?.projects ?? [])
    .filter(
      (project) =>
        project.path &&
        !hidden.ids.includes(project.id) &&
        !hidden.classes.includes(classKey(project.owner, project.config.classroom)),
    )
    .map((project) => ({ project, at: activeAt(project) }))
    .sort((a, b) => b.at - a.at)
    .slice(0, RECENT)
    .map(({ project }) => ({
      id: project.id,
      name: project.assignment?.name ?? project.config.assignment,
      classroom: project.classroom?.name ?? project.config.classroom,
    }));
  return picked.map(({ id, name, classroom }) => ({
    id,
    label:
      picked.filter((other) => other.name === name).length > 1
        ? t("tray.recentItem", { name, classroom })
        : name,
  }));
}

/** When the clone was last worked in; never is oldest. */
function activeAt(project: Project): number {
  const at = project.local?.activeAt;
  return at ? new Date(at).getTime() : 0;
}

/** "Restart to Update to X" while a downloaded update waits. Not yet. */
function update(_input: TrayInput): string | null {
  return null;
}

/** The next local midnight: "today" becomes "yesterday". */
function nextMidnight(now: Date): number {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime();
}

/** Milliseconds from `now` until a label or what the menu picks would change: midnight, a
 *  deadline ("Due" becomes "Was due") or a day after one (it drops, and another may show).
 *  Null when nothing it shows or could pick depends on the time. */
export function nextChange(input: TrayInput, now: Date): number | null {
  const all = candidates(input);
  if (!synced(input, now) && !all.length) return null;
  const from = now.getTime();
  let next = nextMidnight(now);
  for (const { due } of all) {
    for (const moment of [due, due + DAY]) {
      if (moment > from && moment < next) next = moment;
    }
  }
  return next - from;
}

/** `nextChange` as the moment Rust asks for a new menu (`tray-expired`). */
function validUntil(input: TrayInput, now: Date): string | null {
  const next = nextChange(input, now);
  return next === null ? null : new Date(now.getTime() + next).toISOString();
}

let sent: string | null = null;

/** Sends the menu to Rust unless it is the one sent last. */
export function sendTrayMenu(menu: TrayMenu): void {
  const json = JSON.stringify(menu);
  if (json === sent) return;
  sent = json;
  api.setTrayMenu(menu).catch(() => {
    sent = null;
  });
}

/** A menu item Rust handed over (`tray-action`); Rust already showed the popover for the
 *  ones that need it. */
export async function handleTrayAction(action: TrayAction): Promise<void> {
  switch (action.kind) {
    case "settings":
      // The first-run guide keeps its place.
      if (page.url.pathname !== "/welcome") await goto("/settings");
      return;
    case "focus":
      // Pinned whatever the filters, scrolled to and opened, as a link's row is.
      await focus(action.id);
      return;
    case "open": {
      // The list as it is now (the layout keeps it current): the clone may have moved
      // since the menu was sent.
      const project = session.snapshot?.projects.find((other) => other.id === action.id);
      const editor = project && editorFor(project);
      if (project?.path && editor) await openInEditor(project.path, editor);
      return;
    }
    case "installUpdate":
      return;
  }
}
