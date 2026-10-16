import { describe, expect, it } from "vitest";
import pkg from "../../../package.json";
import i18nSource from "../i18n/index.ts?raw";
import { catalogs } from "$lib/i18n";
import { changelogs, notesFor, versions } from "./index";
import {
  KINDS,
  catalogCodes,
  compareVersions,
  draftChangelog,
  draftEntries,
  parseSubject,
  parseVersion,
  pendingVersions,
  previousTag,
  releaseBody,
} from "./versions.js";

describe("versions", () => {
  it("parses versions and pre-releases", () => {
    expect(parseVersion("0.10.0")).toEqual({ core: [0, 10, 0], pre: [] });
    expect(parseVersion("1.2.3-beta.2")).toEqual({ core: [1, 2, 3], pre: ["beta", "2"] });
    expect(parseVersion("v1.2.3")).toBeNull();
    expect(parseVersion("1.2")).toBeNull();
  });

  it("orders by semver precedence", () => {
    const sorted = [
      "0.11.0",
      "0.10.0",
      "0.11.0-beta.10",
      "0.9.1",
      "0.11.0-beta.2",
      "0.11.0-alpha",
      "0.11.0-beta",
    ].sort(compareVersions);
    expect(sorted).toEqual([
      "0.9.1",
      "0.10.0",
      "0.11.0-alpha",
      "0.11.0-beta",
      "0.11.0-beta.2",
      "0.11.0-beta.10",
      "0.11.0",
    ]);
    expect(compareVersions("junk", "0.0.1")).toBeLessThan(0);
  });

  it("starts a stable changelog at the previous stable tag, a pre-release at any", () => {
    const tags = ["v0.9.0", "v0.10.0", "v0.11.0-beta.1", "v0.11.0-beta.2", "nightly", "v0.12"];
    expect(previousTag(tags, "0.11.0")).toBe("v0.10.0");
    expect(previousTag(tags, "0.11.0-beta.3")).toBe("v0.11.0-beta.2");
    expect(previousTag(tags, "0.11.0-beta.1")).toBe("v0.10.0");
    expect(previousTag(tags, "0.10.0")).toBe("v0.9.0");
    expect(previousTag(tags, "0.1.0")).toBeNull();
  });
});

describe("drafting", () => {
  it("keeps feat, fix and perf, without prefix or scope, capitalized", () => {
    expect(parseSubject("feat: add a thing")).toEqual({ kind: "new", text: "Add a thing" });
    expect(parseSubject("fix(ui)!: stop a crash (#42)")).toEqual({
      kind: "fixed",
      text: "Stop a crash",
    });
    expect(parseSubject("perf: faster sync")).toEqual({ kind: "improved", text: "Faster sync" });
    for (const dropped of [
      "chore: bump version to 0.10.0",
      "docs: explain",
      "test: cover",
      "ci: cache",
      "refactor: split",
      "style: format",
      "build: deps",
      "Merge pull request #79 from x/y",
      'Revert "feat: add a thing"',
      "no prefix at all",
      "feat:",
    ]) {
      expect(parseSubject(dropped), dropped).toBeNull();
    }
  });

  it("groups by kind, drops duplicates and cancels reverted commits", () => {
    const entries = draftEntries([
      "fix: one",
      "feat: two",
      "feat: gone",
      "chore: noise",
      'Revert "feat: gone"',
      "perf: three",
      "feat: back",
      'Revert "feat: back"',
      'Revert "Revert "feat: back""',
      "feat: two",
      'Revert "feat: shipped long ago"',
    ]);
    expect(entries).toEqual([
      { kind: "new", text: "Two" },
      { kind: "new", text: "Back" },
      { kind: "improved", text: "Three" },
      { kind: "fixed", text: "One" },
    ]);
  });

  it("drafts English and leaves every other catalog empty", () => {
    expect(draftChangelog(["feat: x"], ["en", "fr", "pt-BR"], "2026-10-16")).toEqual({
      date: "2026-10-16",
      notes: { en: [{ kind: "new", text: "X" }], fr: [], "pt-BR": [] },
    });
  });

  it("reads the registered catalogs from the i18n module", () => {
    expect(catalogCodes(i18nSource)).toEqual(Object.keys(catalogs));
    const later = `export const catalogs: Record<string, Catalog> = {
      en,
      fr,
      "pt-BR": ptBR,
      'zh-Hans': zhHans,
    };`;
    expect(catalogCodes(later)).toEqual(["en", "fr", "pt-BR", "zh-Hans"]);
  });

  it("writes the release body by kind, then where to download", () => {
    const body = releaseBody({
      date: "2026-10-16",
      notes: {
        en: [
          { kind: "fixed", text: "A fix" },
          { text: "Plain" },
          { kind: "new", text: "A\nfeature" },
        ],
        fr: [{ text: "Ignoré" }],
      },
    });
    expect(body).toBe(
      "- Plain\n\n### New\n\n- A feature\n\n### Fixed\n\n- A fix\n\n" +
        "See the assets below to download and install this version.\n",
    );
    expect(releaseBody(null)).toBe("See the assets below to download and install this version.\n");
  });
});

describe("what's new", () => {
  const available = ["0.9.0", "0.10.0", "0.11.0-beta.1", "0.11.0-beta.2", "0.11.0", "0.12.0"];

  it("shows every newer version up to the running one, newest first", () => {
    expect(pendingVersions(available, "0.11.0", "0.9.0")).toEqual(["0.11.0", "0.10.0"]);
    expect(pendingVersions(available, "0.10.0", "0.10.0")).toEqual([]);
    expect(pendingVersions(available, "0.10.0", "0.11.0")).toEqual([]);
  });

  it("shows pre-releases only while running one", () => {
    expect(pendingVersions(available, "0.11.0-beta.2", "0.10.0")).toEqual([
      "0.11.0-beta.2",
      "0.11.0-beta.1",
    ]);
    expect(pendingVersions(available, "0.11.0", "0.11.0-beta.2")).toEqual(["0.11.0"]);
  });

  it("shows only the running version for settings from before the page", () => {
    expect(pendingVersions(available, "0.11.0", null)).toEqual(["0.11.0"]);
    expect(pendingVersions(available, "0.13.0", null)).toEqual([]);
  });

  it("falls back to English per version", () => {
    const [version] = versions();
    expect(notesFor(version, "xx")).toEqual(changelogs[version].notes.en);
    expect(notesFor("0.0.0", "en")).toEqual([]);
  });
});

describe("changelog files", () => {
  it("has notes for the current version in every registered language", () => {
    const current = changelogs[pkg.version];
    expect(current, `src/lib/changelog/${pkg.version}.json (run pnpm bump)`).toBeDefined();
    for (const code of Object.keys(catalogs)) {
      expect(current.notes[code]?.length, `${pkg.version}: notes in ${code}`).toBeGreaterThan(0);
    }
  });

  it("keeps every version's file in shape", () => {
    for (const [version, changelog] of Object.entries(changelogs)) {
      expect(parseVersion(version), `${version}.json: file name`).not.toBeNull();
      expect(changelog.date, `${version}: date`).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(Number.isNaN(Date.parse(changelog.date)), `${version}: date`).toBe(false);
      expect(changelog.notes.en?.length, `${version}: English notes`).toBeGreaterThan(0);
      for (const [code, entries] of Object.entries(changelog.notes)) {
        expect(Array.isArray(entries), `${version}/${code}`).toBe(true);
        for (const entry of entries) {
          expect(typeof entry.text, `${version}/${code}`).toBe("string");
          expect(entry.text.trim(), `${version}/${code}`).not.toBe("");
          expect(entry.text, `${version}/${code}: no em-dash`).not.toContain("\u2014");
          if (entry.kind !== undefined) expect(KINDS, `${version}/${code}`).toContain(entry.kind);
        }
      }
    }
  });
});
