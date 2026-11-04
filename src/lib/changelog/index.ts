// Each version's notes, from `<version>.json` (drafted by `pnpm bump`, rewritten and
// translated in the bump PR). The What's new page shows them after an update. Each file is
// its own chunk, loaded only when the page shows it: the versions come from the file
// names alone.
import { compareVersions, type Changelog, type Entry } from "./versions.js";

// No type argument on the call: Vitest's module cache keeps a module whose source holds
// `import.meta.glob(` out of the cache, since its result depends on other files, and
// `glob<Changelog>(` would get cached with the files of the run that cached it, so a new
// changelog file wouldn't be seen.
const files = import.meta.glob("./*.json", { import: "default" }) as Record<
  string,
  () => Promise<Changelog>
>;

/** Each changelog's loader, by version (`0.10.0`). */
const loaders = new Map(
  Object.entries(files).map(([path, load]) => [path.slice(2, -".json".length), load]),
);
const loading = new Map<string, Promise<Changelog | null>>();

/** The versions with notes, newest first. */
export function versions(): string[] {
  return [...loaders.keys()].sort((a, b) => compareVersions(b, a));
}

/**
 * The changelogs of `wanted`, by version, each file loaded once. A version without notes,
 * or whose file can't load (logged, tried again next time), is left out.
 */
export async function loadChangelogs(
  wanted: readonly string[],
): Promise<Record<string, Changelog>> {
  const found = await Promise.all(
    wanted.map(async (version) => {
      const load = loaders.get(version);
      if (!load) return null;
      let changelog = loading.get(version);
      if (!changelog) {
        changelog = load().catch((err: unknown) => {
          console.error(`couldn't load the ${version} changelog`, err);
          loading.delete(version);
          return null;
        });
        loading.set(version, changelog);
      }
      const loaded = await changelog;
      return loaded ? ([version, loaded] as const) : null;
    }),
  );
  return Object.fromEntries(found.filter((entry) => entry !== null));
}

/** A changelog's entries in catalog `code`, else in English. */
export function notesFor(changelog: Changelog | undefined, code: string): Entry[] {
  const notes = changelog?.notes;
  if (!notes) return [];
  return notes[code]?.length ? notes[code] : (notes.en ?? []);
}
