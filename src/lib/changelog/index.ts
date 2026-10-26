// Each version's notes, bundled from `<version>.json` (drafted by `pnpm bump`, rewritten
// and translated in the bump PR). The What's new page shows them after an update.
import { compareVersions, type Changelog, type Entry } from "./versions.js";

// No type argument on the call: Vitest's module cache keeps a module whose source holds
// `import.meta.glob(` out of the cache, since its result depends on other files, and
// `glob<Changelog>(` would get cached with the files of the run that cached it, so a new
// changelog file wouldn't be seen.
const files = import.meta.glob("./*.json", { eager: true, import: "default" }) as Record<
  string,
  Changelog
>;

/** Every changelog, by version (`0.10.0`). */
export const changelogs: Record<string, Changelog> = Object.fromEntries(
  Object.entries(files).map(([path, changelog]) => [path.slice(2, -".json".length), changelog]),
);

/** The versions with notes, newest first. */
export function versions(): string[] {
  return Object.keys(changelogs).sort((a, b) => compareVersions(b, a));
}

/** `version`'s entries in catalog `code`, else in English. */
export function notesFor(version: string, code: string): Entry[] {
  const notes = changelogs[version]?.notes;
  if (!notes) return [];
  return notes[code]?.length ? notes[code] : (notes.en ?? []);
}
