// Main-page state kept outside the page component, so going to Settings and back shows
// the last known values at once instead of an empty list and footer while they reload.
import {
  api,
  type AuthStatus,
  type Snapshot,
  type SyncProgress,
  type ToolStatus,
  type UpdateStatus,
} from "#lib/api.js";

export const session = $state<{
  tools: ToolStatus[];
  auth: AuthStatus | null;
  /** The project list: read once (`loadSnapshot`), then kept current by the layout's one
   *  `projects-updated` listener, whichever page shows. */
  snapshot: Snapshot | null;
  /** Self-update, kept current by the layout (`update-state`). */
  update: UpdateStatus | null;
  progress: SyncProgress | null;
  /** Ids with news not seen yet (a dot in the list). */
  unseen: string[];
  /** A sign-in code is on screen in the footer, which then needs the whole width. */
  signingIn: boolean;
  /** Avatar `data:` URLs by login, fetched once per run. */
  avatars: Record<string, string>;
  search: string;
  /**
   * The row a link pointed to: shown whatever the filters and hidden lists, its class
   * unfolded, scrolled to and opened. `seq` tells two links to the same row apart. Cleared
   * when the popover hides.
   */
  focus: { id: string; seq: number } | null;
  /** Assignments being accepted and projects being cloned, wherever it was started. */
  accepting: string[];
  cloning: string[];
}>({
  tools: [],
  auth: null,
  snapshot: null,
  update: null,
  progress: null,
  unseen: [],
  signingIn: false,
  avatars: {},
  search: "",
  focus: null,
  accepting: [],
  cloning: [],
});

let snapshotLoad: Promise<void> | null = null;

/** Reads Rust's project list once per run, for whoever needs it first (the layout at
 *  startup, the list, a link); `projects-updated` keeps it current after that. */
export function loadSnapshot(): Promise<void> {
  // An event got there first: nothing to read.
  if (session.snapshot && !snapshotLoad) return Promise.resolve();
  snapshotLoad ??= api.listProjects().then(
    (snapshot) => {
      // The startup sync may still be running; its event fills the list.
      if (snapshot.syncedAt || !session.snapshot) session.snapshot = snapshot;
    },
    (err: unknown) => {
      snapshotLoad = null;
      throw err;
    },
  );
  return snapshotLoad;
}

/** Runs `task` with `id` listed in `session[list]` meanwhile. */
export async function busy<T>(
  list: "accepting" | "cloning",
  id: string,
  task: () => Promise<T>,
): Promise<T> {
  session[list] = [...session[list], id];
  try {
    return await task();
  } finally {
    session[list] = session[list].filter((other) => other !== id);
  }
}
