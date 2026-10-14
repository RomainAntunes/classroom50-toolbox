// Main-page state kept outside the page component, so going to Settings and back shows
// the last known values at once instead of an empty list and footer while they reload.
import type { AuthStatus, Snapshot, SyncProgress, ToolStatus, UpdateInfo } from "$lib/api";

export const session = $state<{
  tools: ToolStatus[];
  auth: AuthStatus | null;
  snapshot: Snapshot | null;
  update: UpdateInfo | null;
  progress: SyncProgress | null;
  /** Ids with news not seen yet (a dot in the list). */
  unseen: string[];
  /** A sign-in code is on screen in the footer, which then needs the whole width. */
  signingIn: boolean;
  /** Avatar `data:` URLs by login, fetched once per run. */
  avatars: Record<string, string>;
  search: string;
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
});
