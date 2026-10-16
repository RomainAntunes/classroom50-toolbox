// Versions and changelog drafts, shared by the UI (which notes to show after an update) and
// `scripts/bump-version.mjs` (which drafts them from the commits). Plain JavaScript with
// JSDoc types, so Node runs it as is. Everything here is pure.

/** @typedef {"new" | "improved" | "fixed"} EntryKind */
/** @typedef {{ kind?: EntryKind, text: string }} Entry */
/**
 * One version's notes: the release date (`YYYY-MM-DD`) and, per catalog code, short entries
 * in students' words. English is required; a missing or empty list falls back to it.
 * @typedef {{ date: string, notes: Record<string, Entry[]> }} Changelog
 */

/** @type {readonly EntryKind[]} */
export const KINDS = ["new", "improved", "fixed"];

const VERSION =
  /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z.-]+)?$/;

/**
 * `1.2.3-beta.2` as numbers and pre-release identifiers, or null when it isn't a version.
 * @param {string} text
 * @returns {{ core: number[], pre: string[] } | null}
 */
export function parseVersion(text) {
  const match = VERSION.exec(text.trim());
  if (!match) return null;
  return {
    core: [Number(match[1]), Number(match[2]), Number(match[3])],
    pre: match[4] ? match[4].split(".") : [],
  };
}

/**
 * Semver precedence: negative when `a` comes first. A pre-release comes before its
 * version (`0.11.0-beta.2` < `0.11.0`); anything that isn't a version comes before all.
 * @param {string} a
 * @param {string} b
 * @returns {number}
 */
export function compareVersions(a, b) {
  const x = parseVersion(a);
  const y = parseVersion(b);
  if (!x || !y) return (x ? 1 : 0) - (y ? 1 : 0);
  for (let i = 0; i < 3; i++) {
    if (x.core[i] !== y.core[i]) return x.core[i] - y.core[i];
  }
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const p = x.pre[i];
    const q = y.pre[i];
    if (p === undefined || q === undefined) return p === undefined ? -1 : 1;
    if (p === q) continue;
    const numeric = /^\d+$/;
    if (numeric.test(p) && numeric.test(q)) return Number(p) - Number(q);
    if (numeric.test(p) !== numeric.test(q)) return numeric.test(p) ? -1 : 1;
    return p < q ? -1 : 1;
  }
  return 0;
}

/**
 * Whether `version` is a pre-release (`0.11.0-beta.2`).
 * @param {string} version
 */
export function isPrerelease(version) {
  return Boolean(parseVersion(version)?.pre.length);
}

/**
 * The tag a changelog for `version` starts from: the newest `v*` tag before it. A stable
 * version starts from the previous stable one, so its notes include its betas; a
 * pre-release starts from the previous tag of any kind.
 * @param {readonly string[]} tags
 * @param {string} version
 * @returns {string | null}
 */
export function previousTag(tags, version) {
  const stable = !isPrerelease(version);
  /** @type {string | null} */
  let best = null;
  for (const tag of tags) {
    const name = tag.trim();
    if (!name.startsWith("v") || !parseVersion(name.slice(1))) continue;
    const candidate = name.slice(1);
    if (compareVersions(candidate, version) >= 0) continue;
    if (stable && isPrerelease(candidate)) continue;
    if (best === null || compareVersions(candidate, best.slice(1)) > 0) best = name;
  }
  return best;
}

/**
 * Conventional commit types students notice, and how the draft files them.
 * @type {Record<string, EntryKind>}
 */
const KIND_OF = { feat: "new", fix: "fixed", perf: "improved" };

const CONVENTIONAL = /^([A-Za-z]+)(?:\([^)]*\))?!?:\s*(.+)$/;
const REVERT = /^Revert "(.*)"$/;

/**
 * A commit subject as a draft entry: `feat(ui): add x (#12)` is a new "Add x". Other types
 * (chore, docs, test, ci, refactor, style, build), merges and reverts give null.
 * @param {string} subject
 * @returns {Entry | null}
 */
export function parseSubject(subject) {
  const match = CONVENTIONAL.exec(subject.trim());
  if (!match) return null;
  const kind = KIND_OF[match[1].toLowerCase()];
  const text = match[2].replace(/\s*\(#\d+\)$/, "").trim();
  if (!kind || !text) return null;
  return { kind, text: text[0].toUpperCase() + text.slice(1) };
}

/**
 * The draft's entries from commit subjects, oldest first: a revert cancels the commit it
 * reverts (a revert of that revert brings it back), duplicates go, and the entries are
 * grouped new, improved, then fixed, each in commit order.
 * @param {readonly string[]} subjects
 * @returns {Entry[]}
 */
export function draftEntries(subjects) {
  /** @type {string[]} */
  const kept = [];
  for (const raw of subjects) {
    const subject = raw.trim();
    const reverted = REVERT.exec(subject)?.[1];
    if (reverted === undefined) {
      kept.push(subject);
      continue;
    }
    const at = kept.lastIndexOf(reverted);
    if (at >= 0) {
      kept.splice(at, 1);
      continue;
    }
    // Reverting a revert whose commit was already cancelled: the commit is back.
    const restored = REVERT.exec(reverted)?.[1];
    if (restored !== undefined) kept.push(restored);
  }
  const seen = new Set();
  /** @type {Entry[]} */
  const entries = [];
  for (const subject of kept) {
    const entry = parseSubject(subject);
    if (!entry || seen.has(entry.text.toLowerCase())) continue;
    seen.add(entry.text.toLowerCase());
    entries.push(entry);
  }
  return KINDS.flatMap((kind) => entries.filter((entry) => entry.kind === kind));
}

/**
 * A new changelog: the English draft, and an empty list for every other catalog, to
 * fill in the bump PR.
 * @param {readonly string[]} subjects
 * @param {readonly string[]} codes
 * @param {string} date
 * @returns {Changelog}
 */
export function draftChangelog(subjects, codes, date) {
  /** @type {Record<string, Entry[]>} */
  const notes = { en: draftEntries(subjects) };
  for (const code of codes) {
    if (code !== "en") notes[code] = [];
  }
  return { date, notes };
}

/**
 * The catalog codes registered in `src/lib/i18n/index.ts`, read from its source: the keys
 * of `catalogs = { en, fr, "pt-BR": ptBR }`, in order.
 * @param {string} source
 * @returns {string[]}
 */
export function catalogCodes(source) {
  const body = /\bcatalogs\b[^=]*=\s*\{([^}]*)\}/.exec(source)?.[1];
  if (!body) return ["en"];
  const codes = body
    .split(",")
    .map((part) =>
      part
        .split(":")[0]
        .trim()
        .replace(/^["']|["']$/g, ""),
    )
    .filter(Boolean);
  return codes.includes("en") ? codes : ["en", ...codes];
}

/** @type {Record<EntryKind, string>} */
const HEADINGS = { new: "New", improved: "Improved", fixed: "Fixed" };

export const DOWNLOAD_LINE = "See the assets below to download and install this version.";

/**
 * The GitHub release body (Markdown, English): the entries under a heading per kind,
 * those without a kind first, then where to download.
 * @param {Changelog | null} changelog
 * @returns {string}
 */
export function releaseBody(changelog) {
  const entries = changelog?.notes.en ?? [];
  const list = (/** @type {Entry[]} */ items) =>
    items.map((entry) => `- ${entry.text.replace(/\s+/g, " ").trim()}`).join("\n");
  /** @type {string[]} */
  const parts = [];
  const plain = entries.filter((entry) => !entry.kind);
  if (plain.length) parts.push(list(plain));
  for (const kind of KINDS) {
    const items = entries.filter((entry) => entry.kind === kind);
    if (items.length) parts.push(`### ${HEADINGS[kind]}\n\n${list(items)}`);
  }
  parts.push(DOWNLOAD_LINE);
  return parts.join("\n\n") + "\n";
}

/**
 * The versions whose notes to show, newest first: those after `lastSeen` up to `current`
 * that have notes. A stable version leaves pre-releases out (its notes include them).
 * With no `lastSeen` (settings from before the page) only `current`'s show.
 * @param {readonly string[]} available
 * @param {string} current
 * @param {string | null} lastSeen
 * @returns {string[]}
 */
export function pendingVersions(available, current, lastSeen) {
  if (!parseVersion(current)) return [];
  if (lastSeen === null) return available.includes(current) ? [current] : [];
  const stable = !isPrerelease(current);
  return available
    .filter(
      (version) =>
        parseVersion(version) &&
        compareVersions(version, lastSeen) > 0 &&
        compareVersions(version, current) <= 0 &&
        !(stable && isPrerelease(version)),
    )
    .sort((a, b) => compareVersions(b, a));
}
