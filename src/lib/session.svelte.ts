// Main-page state kept outside the page component, so going to Settings and back shows
// the last known values at once instead of an empty list and footer while they reload.
import type { AuthStatus, Snapshot, ToolStatus, UpdateInfo } from "$lib/api";

export const session = $state<{
  tools: ToolStatus[];
  auth: AuthStatus | null;
  snapshot: Snapshot | null;
  update: UpdateInfo | null;
  search: string;
}>({ tools: [], auth: null, snapshot: null, update: null, search: "" });
