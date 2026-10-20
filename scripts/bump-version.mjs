// Sets the app version everywhere it lives: package.json (read by tauri.conf.json and the
// release workflow), src-tauri/Cargo.toml and its Cargo.lock entry, then drafts the
// version's changelog from the commits since the previous tag. Usage: pnpm bump 0.2.0 [--force], or pnpm bump
// 0.3.0-beta.1 for a pre-release (beta channel only; the release workflow accepts no other
// pre-release label).
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { catalogCodes, draftChangelog, previousTag } from "../src/lib/changelog/versions.js";
import { withVersion } from "./ci-scope.mjs";

const args = process.argv.slice(2);
const force = args.includes("--force");
const version = args.find((arg) => !arg.startsWith("--"));
if (!/^\d+\.\d+\.\d+(-beta\.\d+)?$/.test(version ?? "")) {
  console.error("usage: pnpm bump <major.minor.patch>[-beta.<n>] [--force]");
  process.exit(1);
}

const paths = {
  pkg: "package.json",
  cargo: "src-tauri/Cargo.toml",
  lock: "src-tauri/Cargo.lock",
};
const bumped = withVersion(
  {
    pkg: readFileSync(paths.pkg, "utf8"),
    cargo: readFileSync(paths.cargo, "utf8"),
    lock: readFileSync(paths.lock, "utf8"),
  },
  version,
);
for (const key of /** @type {const} */ (["pkg", "cargo", "lock"])) {
  writeFileSync(paths[key], bumped[key]);
}

console.log(`version set to ${version} in package.json, Cargo.toml and Cargo.lock`);

const notesPath = `src/lib/changelog/${version}.json`;
if (existsSync(notesPath) && !force) {
  console.log(`${notesPath} already exists; kept it (--force drafts it again)`);
  process.exit(0);
}

const git = (/** @type {string[]} */ gitArgs) =>
  execFileSync("git", gitArgs, { encoding: "utf8" }).split("\n").filter(Boolean);
const since = previousTag(git(["tag", "--list", "v*"]), version);
// Oldest first, so a revert comes after the commit it cancels.
const subjects = git([
  "log",
  "--no-merges",
  "--reverse",
  "--format=%s",
  since ? `${since}..HEAD` : "HEAD",
]);
const codes = catalogCodes(readFileSync("src/lib/i18n/index.ts", "utf8"));
const today = new Date();
const date = [today.getFullYear(), today.getMonth() + 1, today.getDate()]
  .map((part) => String(part).padStart(2, "0"))
  .join("-");
const changelog = draftChangelog(subjects, codes, date);
writeFileSync(notesPath, JSON.stringify(changelog, null, 2) + "\n");

const others = codes.filter((code) => code !== "en");
console.log(`
drafted ${notesPath}: ${changelog.notes.en.length} entries from ${subjects.length} commits since ${since ?? "the first commit"}
next, either run node scripts/changelog-ai.mjs ${version} (ANTHROPIC_API_KEY set; the Bump
workflow does it), or by hand in this PR:
  1. rewrite the English entries in students' words: short, plain, what changes for them,
     no internal names (modules, settings keys, commands); drop what they won't notice
  2. translate them into ${others.join(", ") || "every other catalog"} (same entries, same order)
  3. check the date (the day it ships), then run pnpm test, which fails until every
     catalog has entries`);
