// Writes a version's changelog in students' words, in every registered catalog, with Claude:
// it reads the draft `pnpm bump` made from the commit subjects, the commits and pull requests
// behind it and the two newest finished changelogs, and asks for the notes as JSON, checked
// before they are written. A changelog already filled in every catalog (written by hand) is
// kept. Usage: node scripts/changelog-ai.mjs 0.13.0, with ANTHROPIC_API_KEY set (and GH_TOKEN
// or a gh login for the pull requests, optional). The Bump workflow runs it.
import { execFileSync } from "node:child_process";
import { appendFileSync, existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import {
  KINDS,
  catalogCodes,
  compareVersions,
  parseVersion,
  previousTag,
} from "../src/lib/changelog/versions.js";

/** @typedef {import("../src/lib/changelog/versions.js").Changelog} Changelog */
/** @typedef {import("../src/lib/changelog/versions.js").Entry} Entry */

export const MODEL = "claude-opus-5";
const CHANGELOG_DIR = "src/lib/changelog";

/**
 * Whether every catalog has entries, so there is nothing to write.
 * @param {Changelog} changelog
 * @param {readonly string[]} codes
 */
export function isComplete(changelog, codes) {
  return codes.every((code) => (changelog.notes[code]?.length ?? 0) > 0);
}

/**
 * The JSON schema the model's answer must follow: one list of entries per catalog.
 * @param {readonly string[]} codes
 */
export function outputSchema(codes) {
  const entry = {
    type: "object",
    properties: {
      kind: { type: "string", enum: [...KINDS] },
      text: { type: "string" },
    },
    required: ["kind", "text"],
    additionalProperties: false,
  };
  return {
    type: "object",
    properties: Object.fromEntries(codes.map((code) => [code, { type: "array", items: entry }])),
    required: [...codes],
    additionalProperties: false,
  };
}

// What gives away an internal name: code formatting, source files, identifiers written
// camelCase or snake_case, issue numbers. Product names that look like one are allowed.
const INTERNAL = [
  { pattern: /`/, why: "a backtick" },
  { pattern: /\.(rs|ts|js|mjs|svelte|json|ya?ml|toml)\b/, why: "a file name" },
  { pattern: /\b[a-z]+[A-Z][A-Za-z]*\b/, why: "a camelCase name", allow: /^(macOS|iOS|iPadOS)$/ },
  { pattern: /\b[a-z]+_[a-z_]+\b/, why: "a snake_case name" },
  { pattern: /#\d+/, why: "an issue number" },
];

/**
 * What is wrong with the model's notes, empty when they can be written: every catalog, the
 * same kinds in the same order as English, at least one entry, no empty text, no em-dash
 * and nothing that reads like an internal name.
 * @param {unknown} notes
 * @param {readonly string[]} codes
 * @returns {string[]}
 */
export function validateNotes(notes, codes) {
  if (typeof notes !== "object" || notes === null || Array.isArray(notes)) {
    return ["the answer is not an object of catalogs"];
  }
  const byCode = /** @type {Record<string, unknown>} */ (notes);
  /** @type {string[]} */
  const problems = [];
  const extra = Object.keys(byCode).filter((code) => !codes.includes(code));
  if (extra.length > 0) problems.push(`unknown catalogs: ${extra.join(", ")}`);
  const english = Array.isArray(byCode.en) ? byCode.en : [];
  if (english.length === 0) problems.push("en has no entries");
  const kinds = english.map((entry) => entry?.kind);
  for (const code of codes) {
    const entries = byCode[code];
    if (!Array.isArray(entries)) {
      problems.push(`${code} is missing`);
      continue;
    }
    if (entries.length !== english.length) {
      problems.push(`${code} has ${entries.length} entries, en has ${english.length}`);
    }
    entries.forEach((entry, index) => {
      const where = `${code}[${index}]`;
      if (!KINDS.includes(entry?.kind)) problems.push(`${where} has no valid kind`);
      else if (code !== "en" && index < kinds.length && entry.kind !== kinds[index]) {
        problems.push(`${where} is ${entry.kind}, en[${index}] is ${kinds[index]}`);
      }
      const text = typeof entry?.text === "string" ? entry.text.trim() : "";
      if (!text) {
        problems.push(`${where} has no text`);
        return;
      }
      if (text.includes("—")) problems.push(`${where} has an em-dash`);
      for (const { pattern, why, allow } of INTERNAL) {
        const found = text.match(new RegExp(pattern, "g")) ?? [];
        const named = found.filter((/** @type {string} */ match) => !allow?.test(match));
        if (named.length > 0) problems.push(`${where} has ${why}: ${named[0]}`);
      }
    });
  }
  return problems;
}

/**
 * The notes as the changelog file holds them: the catalogs in registered order, texts trimmed.
 * @param {Record<string, Entry[]>} notes
 * @param {readonly string[]} codes
 */
export function orderedNotes(notes, codes) {
  return Object.fromEntries(
    codes.map((code) => [code, notes[code].map(({ kind, text }) => ({ kind, text: text.trim() }))]),
  );
}

/**
 * The request's text: the version, the draft, the commits and pull requests behind it, and
 * the earlier changelogs to follow. A rejected answer comes back with what was wrong with it.
 * @param {{
 *   version: string,
 *   draft: Changelog,
 *   commits: string[],
 *   pulls: { number: number, title: string, body: string }[],
 *   examples: { version: string, changelog: Changelog }[],
 *   rejected?: { answer: string, problems: string[] },
 * }} input
 */
export function buildPrompt({ version, draft, commits, pulls, examples, rejected }) {
  const sections = [
    `# Version ${version}${parseVersion(version)?.pre.length ? " (a beta)" : ""}`,
    "## Draft from the commit subjects (English)",
    JSON.stringify(draft.notes.en ?? [], null, 2),
    "## Commits since the previous version, oldest first",
    commits.length > 0 ? commits.join("\n\n---\n\n") : "(none)",
    "## Pull requests merged since the previous version",
    pulls.length > 0
      ? pulls.map((pull) => `### #${pull.number} ${pull.title}\n\n${pull.body.trim()}`).join("\n\n")
      : "(none found)",
    "## Earlier versions' notes, to follow for tone, length and wording",
    ...examples.map(
      ({ version: earlier, changelog }) =>
        `### ${earlier}\n\n${JSON.stringify(changelog.notes, null, 2)}`,
    ),
  ];
  if (rejected) {
    sections.push(
      "## Your previous answer was rejected",
      rejected.answer,
      "Fix these problems and answer again in full:",
      rejected.problems.map((problem) => `- ${problem}`).join("\n"),
    );
  }
  return sections.join("\n\n");
}

/**
 * Markdown for the workflow's summary: English as a list, then each other catalog.
 * @param {string} version
 * @param {Changelog} changelog
 * @param {readonly string[]} codes
 */
export function summaryMarkdown(version, changelog, codes) {
  const list = (/** @type {string} */ code) =>
    (changelog.notes[code] ?? []).map(({ kind, text }) => `- **${kind}** ${text}`).join("\n");
  const others = codes
    .filter((code) => code !== "en")
    .map((code) => `<details><summary>${code}</summary>\n\n${list(code)}\n\n</details>`);
  return [`## Changelog ${version}`, list("en"), ...others].join("\n\n") + "\n";
}

/**
 * Pull request numbers from merge commit subjects (`Merge pull request #12 from ...`).
 * @param {readonly string[]} subjects
 */
export function pullNumbers(subjects) {
  return subjects
    .map((subject) => subject.match(/^Merge pull request #(\d+) /)?.[1])
    .filter((number) => number !== undefined)
    .map(Number);
}

/**
 * The newest finished changelogs before `version`, to show the model the house style.
 * @param {string} version
 * @param {readonly string[]} codes
 * @param {number} count
 */
function examples(version, codes, count) {
  return readdirSync(CHANGELOG_DIR)
    .filter((name) => name.endsWith(".json"))
    .map((name) => name.slice(0, -".json".length))
    .filter((other) => parseVersion(other) && compareVersions(other, version) < 0)
    .sort(compareVersions)
    .reverse()
    .map((other) => ({
      version: other,
      changelog: /** @type {Changelog} */ (
        JSON.parse(readFileSync(`${CHANGELOG_DIR}/${other}.json`, "utf8"))
      ),
    }))
    .filter(({ changelog }) => isComplete(changelog, codes))
    .slice(0, count);
}

/** @param {string[]} args */
function git(args) {
  return execFileSync("git", args, { encoding: "utf8" });
}

/**
 * The pull requests' titles and bodies through gh; none when gh can't read them.
 * @param {number[]} numbers
 */
function pulls(numbers) {
  return numbers.flatMap((number) => {
    try {
      const json = execFileSync(
        "gh",
        ["pr", "view", String(number), "--json", "number,title,body"],
        { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
      );
      return [JSON.parse(json)];
    } catch {
      console.warn(`could not read pull request #${number}; going on without it`);
      return [];
    }
  });
}

/**
 * One request: the notes as parsed JSON and the raw answer.
 * @param {import("@anthropic-ai/sdk").default} client
 * @param {string} system
 * @param {string} prompt
 * @param {readonly string[]} codes
 */
async function ask(client, system, prompt, codes) {
  const message = await client.beta.messages
    .stream({
      model: MODEL,
      max_tokens: 64000,
      betas: ["server-side-fallback-2026-07-01"],
      fallbacks: "default",
      thinking: { type: "adaptive" },
      output_config: {
        effort: "high",
        format: { type: "json_schema", schema: outputSchema(codes) },
      },
      system,
      messages: [{ role: "user", content: prompt }],
    })
    .finalMessage();
  if (message.stop_reason === "refusal") throw new Error("the model declined the request");
  if (message.stop_reason === "max_tokens") throw new Error("the answer was cut off");
  const answer = message.content
    .flatMap((block) => (block.type === "text" ? [block.text] : []))
    .join("");
  console.log(
    `${message.model}: ${message.usage.input_tokens} tokens in, ${message.usage.output_tokens} out`,
  );
  try {
    return { answer, notes: JSON.parse(answer) };
  } catch {
    return { answer, notes: null };
  }
}

async function main() {
  const version = process.argv[2] ?? "";
  if (!/^\d+\.\d+\.\d+(-beta\.\d+)?$/.test(version)) {
    console.error("usage: node scripts/changelog-ai.mjs <major.minor.patch>[-beta.<n>]");
    process.exit(1);
  }
  const path = `${CHANGELOG_DIR}/${version}.json`;
  if (!existsSync(path)) {
    console.error(`no ${path}; run pnpm bump ${version} first`);
    process.exit(1);
  }
  /** @type {Changelog} */
  const draft = JSON.parse(readFileSync(path, "utf8"));
  const codes = catalogCodes(readFileSync("src/lib/i18n/index.ts", "utf8"));
  const summary = process.env.GITHUB_STEP_SUMMARY;

  if (isComplete(draft, codes)) {
    console.log(`${path} already has entries in every catalog; kept it`);
    if (summary) appendFileSync(summary, summaryMarkdown(version, draft, codes));
    return;
  }
  if (!process.env.ANTHROPIC_API_KEY) {
    console.error("ANTHROPIC_API_KEY is not set");
    process.exit(1);
  }

  const since = previousTag(git(["tag", "--list", "v*"]).split("\n").filter(Boolean), version);
  const range = since ? `${since}..HEAD` : "HEAD";
  const commits = git(["log", "--no-merges", "--reverse", "--format=%B%x00", range])
    .split("\0")
    .map((commit) => commit.trim())
    .filter(Boolean);
  const merges = git(["log", "--merges", "--reverse", "--format=%s", range])
    .split("\n")
    .filter(Boolean);
  const input = {
    version,
    draft,
    commits,
    pulls: pulls(pullNumbers(merges)),
    examples: examples(version, codes, 2),
  };

  const { default: Anthropic } = await import("@anthropic-ai/sdk");
  const client = new Anthropic();
  const system = readFileSync("scripts/changelog-prompt.md", "utf8");
  /** @type {{ answer: string, problems: string[] } | undefined} */
  let rejected;
  for (let attempt = 1; attempt <= 2; attempt++) {
    const { answer, notes } = await ask(client, system, buildPrompt({ ...input, rejected }), codes);
    const problems = notes === null ? ["the answer is not JSON"] : validateNotes(notes, codes);
    if (problems.length === 0) {
      /** @type {Changelog} */
      const changelog = { date: draft.date, notes: orderedNotes(notes, codes) };
      writeFileSync(path, JSON.stringify(changelog, null, 2) + "\n");
      console.log(
        `wrote ${path}: ${changelog.notes.en.length} entries in ${codes.length} catalogs`,
      );
      if (summary) appendFileSync(summary, summaryMarkdown(version, changelog, codes));
      return;
    }
    console.warn(`attempt ${attempt} rejected:\n${problems.map((p) => `  ${p}`).join("\n")}`);
    rejected = { answer, problems };
  }
  console.error("no usable changelog after 2 attempts; write it by hand in a PR");
  process.exit(1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
