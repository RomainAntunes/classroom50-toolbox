// When the What's new page shows: once the guide is done, after an update, until "Got it".
// `lastSeenVersion` (Rust's settings) says how far the user has read.
import { api, type Settings } from "#lib/api.js";
import { versions } from "#lib/changelog/index.js";
import { compareVersions, pendingVersions } from "#lib/changelog/versions.js";

let version: Promise<string> | null = null;

/** The running app's version, asked once. */
export function appVersion(): Promise<string> {
  version ??= api.appInfo().then((info) => info.version);
  return version;
}

/** The versions whose notes wait for the user, newest first; none before the guide is done. */
export async function pendingNotes(settings: Settings): Promise<string[]> {
  if (!settings.onboarded) return [];
  return pendingVersions(versions(), await appVersion(), settings.lastSeenVersion);
}

/** `lastSeenVersion` once the running version's notes are read: never moves back. */
export async function seenVersion(settings: Settings | null): Promise<string> {
  const current = await appVersion();
  const seen = settings?.lastSeenVersion;
  return seen && compareVersions(seen, current) > 0 ? seen : current;
}
