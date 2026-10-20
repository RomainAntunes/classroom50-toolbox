import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { catalogCodes } from "../src/lib/changelog/versions.js";
import {
  buildPrompt,
  isComplete,
  orderedNotes,
  outputSchema,
  pullNumbers,
  summaryMarkdown,
  validateNotes,
} from "./changelog-ai.mjs";

const codes = ["en", "fr", "de"];
const entry = (kind, text) => ({ kind, text });
const good = () => ({
  en: [entry("new", "Add a thing."), entry("fixed", "A thing no longer breaks.")],
  fr: [entry("new", "Ajout d'une chose."), entry("fixed", "Une chose ne casse plus.")],
  de: [entry("new", "Neu: ein Ding."), entry("fixed", "Ein Ding geht nicht mehr kaputt.")],
});

describe("changelog with Claude", () => {
  it("asks for one list of entries per catalog, nothing else", () => {
    const schema = outputSchema(codes);
    expect(schema.required).toEqual(codes);
    expect(schema.additionalProperties).toBe(false);
    expect(schema.properties.fr.items.properties.kind.enum).toEqual(["new", "improved", "fixed"]);
  });

  it("accepts notes that match English in every catalog", () => {
    expect(validateNotes(good(), codes)).toEqual([]);
  });

  it("accepts every changelog shipped so far", () => {
    const all = catalogCodes(readFileSync("src/lib/i18n/index.ts", "utf8"));
    for (const name of readdirSync("src/lib/changelog").filter((n) => /^\d.*\.json$/.test(n))) {
      const { notes } = JSON.parse(readFileSync(`src/lib/changelog/${name}`, "utf8"));
      if (!isComplete({ date: "", notes }, all)) continue;
      expect(validateNotes(notes, all), name).toEqual([]);
    }
  });

  it("rejects a missing catalog, a different count or kind, and an unknown catalog", () => {
    const notes = good();
    delete notes.de;
    notes.fr = [entry("improved", "Mieux."), entry("fixed", "Réparé."), entry("fixed", "Encore.")];
    notes.xx = [];
    expect(validateNotes(notes, codes)).toEqual([
      "unknown catalogs: xx",
      "fr has 3 entries, en has 2",
      "fr[0] is improved, en[0] is new",
      "de is missing",
    ]);
  });

  it("rejects empty text, em-dashes and internal names", () => {
    const notes = good();
    notes.en = [entry("new", "Run `gh student` now."), entry("fixed", "Sync — faster.")];
    notes.fr = [entry("new", "  "), entry("fixed", "Le réglage commitActions marche.")];
    notes.de = [entry("new", "Siehe #12."), entry("fixed", "In push.rs und last_seen.")];
    expect(validateNotes(notes, codes)).toEqual([
      "en[0] has a backtick: `",
      "en[1] has an em-dash",
      "fr[0] has no text",
      "fr[1] has a camelCase name: commitActions",
      "de[0] has an issue number: #12",
      "de[1] has a file name: .rs",
      "de[1] has a snake_case name: last_seen",
    ]);
  });

  it("allows product names that look like identifiers", () => {
    const notes = good();
    notes.en[0] = entry("new", "On macOS and iOS, a thing.");
    expect(validateNotes(notes, codes)).toEqual([]);
  });

  it("wants at least one entry and an object of catalogs", () => {
    expect(validateNotes({ en: [], fr: [], de: [] }, codes)).toEqual(["en has no entries"]);
    expect(validateNotes([], codes)).toEqual(["the answer is not an object of catalogs"]);
  });

  it("writes the catalogs in registered order, trimmed", () => {
    const notes = { de: good().de, en: [entry("new", " Add. ")], fr: good().fr };
    const ordered = orderedNotes(notes, codes);
    expect(Object.keys(ordered)).toEqual(codes);
    expect(ordered.en[0].text).toBe("Add.");
  });

  it("knows a changelog filled by hand", () => {
    expect(isComplete({ date: "", notes: good() }, codes)).toBe(true);
    expect(isComplete({ date: "", notes: { ...good(), fr: [] } }, codes)).toBe(false);
  });

  it("finds the pull requests in merge subjects", () => {
    expect(
      pullNumbers([
        "Merge pull request #115 from RomainAntunes/fix/login-item",
        "Merge branch 'main' into x",
        "Merge pull request #116 from RomainAntunes/fix/update-interval",
      ]),
    ).toEqual([115, 116]);
  });

  it("puts the draft, the commits, the pull requests and the examples in the prompt", () => {
    const prompt = buildPrompt({
      version: "0.13.0-beta.1",
      draft: { date: "2026-10-20", notes: { en: [entry("new", "Draft entry")] } },
      commits: ["feat: a thing\n\nBecause."],
      pulls: [{ number: 7, title: "A thing", body: "What changes for students." }],
      examples: [{ version: "0.12.1", changelog: { date: "", notes: good() } }],
    });
    expect(prompt).toContain("# Version 0.13.0-beta.1 (a beta)");
    expect(prompt).toContain("Draft entry");
    expect(prompt).toContain("Because.");
    expect(prompt).toContain("### #7 A thing\n\nWhat changes for students.");
    expect(prompt).toContain("### 0.12.1");
    expect(prompt).not.toContain("rejected");

    const retry = buildPrompt({
      version: "0.13.0",
      draft: { date: "", notes: { en: [] } },
      commits: [],
      pulls: [],
      examples: [],
      rejected: { answer: "{}", problems: ["en has no entries"] },
    });
    expect(retry).toContain("# Version 0.13.0\n");
    expect(retry).toContain("## Your previous answer was rejected\n\n{}");
    expect(retry).toContain("- en has no entries");
  });

  it("summarizes English first, then each catalog folded", () => {
    const markdown = summaryMarkdown("0.13.0", { date: "", notes: good() }, codes);
    expect(markdown).toMatch(/^## Changelog 0.13.0\n\n- \*\*new\*\* Add a thing\./);
    expect(markdown).toContain("<details><summary>fr</summary>\n\n- **new** Ajout d'une chose.");
  });
});
