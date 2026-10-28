// The tray icon's menu. The UI computes and words it (Rust has no catalogs) and Rust builds
// it (`tray.rs`), leaving out empty sections. The layout sends it again whenever an input
// or the language changes, and once more at `nextChange`: nothing ticks while the popover
// is hidden, so a label may only change at midnight or at a moment known in advance.
import { goto } from "$app/navigation";
import { page } from "$app/state";
import {
  api,
  type EditorInfo,
  type Settings,
  type Snapshot,
  type TrayAction,
  type TrayItem,
  type TrayMenu,
  type UpdateStatus,
} from "#lib/api.js";
import { relativeAt, t } from "#lib/i18n/index.js";

/** What the menu is made of; the layout keeps each one current. */
export interface TrayInput {
  snapshot: Snapshot | null;
  update: UpdateStatus | null;
  settings: Settings | null;
  /** The installed editors: Open Recent needs one. */
  editors: readonly EditorInfo[];
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

/** Milliseconds from `now` until a label or what the menu picks would change. */
export function nextChange(_input: TrayInput, now: Date): number {
  return nextMidnight(now) - now.getTime();
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
