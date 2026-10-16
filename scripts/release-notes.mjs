// Prints a version's GitHub release body (Markdown, English) from its changelog file; the
// release workflow passes it to tauri-action, which also puts it in latest.json's `notes`.
// Usage: node scripts/release-notes.mjs 0.11.0
import { existsSync, readFileSync } from "node:fs";
import { releaseBody } from "../src/lib/changelog/versions.js";

const version = process.argv[2] ?? "";
const path = `src/lib/changelog/${version}.json`;
if (!existsSync(path)) {
  console.error(`no changelog at ${path}; the release body only says where to download`);
}
console.log(
  releaseBody(existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : null).trimEnd(),
);
