// The tray icon's menu. The UI computes and words it (Rust has no catalogs) and Rust builds
// it (`tray.rs`), leaving out empty sections. The layout sends it again whenever an input
// or the language changes, and when Rust says its `validUntil` passed (`tray-expired`, on
// the background tick it runs anyway): the UI has no timer, so a label may only change at
// midnight or at a moment known in advance.
import { goto } from "$app/navigation";
import { page } from "$app/state";
import {
  api,
  type Settings,
  type Snapshot,
  type TrayAction,
  type TrayItem,
  type TrayMenu,
  type UpdateStatus,
} from "#lib/api.js";
import { relativeAt, t } from "#lib/i18n/index.js";

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

/** The nearest deadlines of unfinished work, nearest first. None yet. */
function deadlines(_input: TrayInput, _now: Date): TrayItem[] {
  return [];
}

/** The clones worked on last, for Open Recent. None yet. */
function recent(_input: TrayInput): TrayItem[] {
  return [];
}

/** "Restart to Update to X" while a downloaded update waits. Not yet. */
function update(_input: TrayInput): string | null {
  return null;
}

/** The next local midnight: "today" becomes "yesterday". */
function nextMidnight(now: Date): number {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime();
}

/** Milliseconds from `now` until a label or what the menu picks would change; null when
 *  nothing it shows depends on the time. */
export function nextChange(input: TrayInput, now: Date): number | null {
  if (!synced(input, now)) return null;
  return nextMidnight(now) - now.getTime();
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
      return;
    case "open":
      return;
    case "installUpdate":
      return;
  }
}
