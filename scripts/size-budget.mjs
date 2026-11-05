// The webview bundle's size budget: measures the built frontend's JavaScript and CSS, raw
// and gzipped, against the limits in scripts/size-budget.json, twice: everything under
// build/ (`total`), and what the app loads at start (`startup`: Kit's entries, the root
// layout and the main page, with every chunk they import statically, read from Vite's
// client manifest), prints a table for each (also to $GITHUB_STEP_SUMMARY in CI) and
// fails when one is over.
// Usage (ci.yml, after `pnpm build`): node scripts/size-budget.mjs [build] [manifest]
import { appendFileSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";

export const KINDS = ["js", "css"];
export const METRICS = ["raw", "gzip"];

/** The kind a built file counts under, or null for what the budget leaves out. */
export function kindOf(path) {
  if (/\.(m?js)$/.test(path)) return "js";
  if (/\.css$/.test(path)) return "css";
  return null;
}

/** Bytes after gzip at its highest level, about what a compressed transfer would be. */
export function gzipSize(bytes) {
  return gzipSync(bytes, { level: 9 }).length;
}

/**
 * Totals per kind for a list of built files.
 * @param {{ path: string, bytes: Uint8Array }[]} files
 * @returns {Record<string, { files: number, raw: number, gzip: number }>}
 */
export function measure(files) {
  const totals = Object.fromEntries(KINDS.map((kind) => [kind, { files: 0, raw: 0, gzip: 0 }]));
  for (const { path, bytes } of files) {
    const kind = kindOf(path);
    if (!kind) continue;
    totals[kind].files += 1;
    totals[kind].raw += bytes.length;
    totals[kind].gzip += gzipSize(bytes);
  }
  return totals;
}

/**
 * One row per kind and metric: the measured size, its limit and whether it fits.
 * @param {ReturnType<typeof measure>} totals
 * @param {Record<string, Record<string, number>>} budget
 */
export function compare(totals, budget) {
  return KINDS.flatMap((kind) =>
    METRICS.map((metric) => {
      const size = totals[kind][metric];
      const limit = budget[kind]?.[metric];
      if (typeof limit !== "number") throw new Error(`size-budget.json has no ${kind}.${metric}`);
      return { kind, metric, size, limit, ok: size <= limit };
    }),
  );
}

/** The components whose nodes load at start: the root layout and the main page (`/`). */
export const STARTUP_COMPONENTS = ["src/routes/+layout.svelte", "src/routes/+page.svelte"];

/**
 * The component a generated SvelteKit node file exports (`export { default as component }
 * from "../../src/routes/+page.svelte"`), as a path from the project root; null for none.
 * @param {string} key the node's manifest key, a path from the project root
 * @param {string} source that file's text
 */
export function componentOf(key, source) {
  const from = /export\s*\{\s*default as component\s*\}\s*from\s*["']([^"']+)["']/.exec(
    source,
  )?.[1];
  if (!from) return null;
  return relative(".", resolve(dirname(key), from))
    .split("\\")
    .join("/");
}

/**
 * The manifest keys the app starts from: every entry but SvelteKit's nodes (its start, app
 * and payload modules), plus the nodes whose component `component(key)` names is in
 * `STARTUP_COMPONENTS`.
 * @param {Record<string, { isEntry?: boolean }>} manifest Vite's client manifest
 * @param {(key: string) => string | null} component
 */
export function startupRoots(manifest, component) {
  return Object.entries(manifest)
    .filter(([key, chunk]) => {
      if (!chunk.isEntry) return false;
      if (!/(^|\/)nodes\/\d+\.js$/.test(key)) return true;
      return STARTUP_COMPONENTS.includes(component(key) ?? "");
    })
    .map(([key]) => key);
}

/**
 * The built files (JavaScript and CSS, paths under build/) loaded with `roots`: each one's
 * file and CSS, then the same for everything it imports statically, never what it imports
 * dynamically (a language, a changelog, another page).
 * @param {Record<string, { file: string, imports?: string[], css?: string[] }>} manifest
 * @param {string[]} roots
 * @returns {string[]}
 */
export function startupFiles(manifest, roots) {
  const files = new Set();
  const seen = new Set();
  const queue = [...roots];
  while (queue.length > 0) {
    const key = queue.shift();
    if (seen.has(key)) continue;
    seen.add(key);
    const chunk = manifest[key];
    if (!chunk) throw new Error(`the manifest has no ${key}`);
    files.add(chunk.file);
    for (const css of chunk.css ?? []) files.add(css);
    queue.push(...(chunk.imports ?? []));
  }
  return [...files].sort();
}

/** `12.3 kB`, `4.47 MB` (1000-based, as Vite's and browsers' reports). */
export function formatBytes(bytes) {
  if (bytes < 1000) return `${bytes} B`;
  // Rounded on whole numbers first: toFixed rounds the binary value, and 1.045 is stored as
  // 1.04499..., so 1,045,000 bytes would print 1.04 MB.
  const tenthsOfKb = Math.round(bytes / 100);
  if (tenthsOfKb < 10_000) return `${(tenthsOfKb / 10).toFixed(1)} kB`;
  return `${(Math.round(bytes / 10_000) / 100).toFixed(2)} MB`;
}

/** The two budgets, as their tables' headings name them. */
export const SECTIONS = { total: "Total", startup: "At startup" };

/** The rows as a Markdown table, with how much of each limit is used. */
export function formatTable(rows) {
  const lines = [
    "| Bundle | Size | Limit | Used |",
    "| --- | ---: | ---: | ---: |",
    ...rows.map(({ kind, metric, size, limit, ok }) => {
      const used = `${Math.round((size / limit) * 100)}%`;
      return `| ${kind.toUpperCase()} (${metric}) | ${formatBytes(size)} | ${formatBytes(limit)} | ${ok ? used : `**${used}**, over`} |`;
    }),
  ];
  return lines.join("\n");
}

/** Every file under `dir`, with its path relative to it. */
function walk(dir, root = dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return walk(path, root);
    return [{ path: relative(root, path), bytes: readFileSync(path) }];
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const dir = process.argv[2] ?? "build";
  const manifestPath = process.argv[3] ?? ".svelte-kit/output/client/.vite/manifest.json";
  const budget = JSON.parse(readFileSync(new URL("./size-budget.json", import.meta.url), "utf8"));
  const all = walk(dir);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const roots = startupRoots(manifest, (key) => componentOf(key, readFileSync(key, "utf8")));
  if (roots.filter((key) => /nodes\/\d+\.js$/.test(key)).length !== STARTUP_COMPONENTS.length) {
    console.error(`${manifestPath} has no node for one of ${STARTUP_COMPONENTS.join(", ")}`);
    process.exit(1);
  }
  const startup = new Set(startupFiles(manifest, roots));
  const measured = {
    total: measure(all),
    startup: measure(all.filter((file) => startup.has(file.path.split("\\").join("/")))),
  };
  const over = [];
  let summary = "### Webview bundle size\n";
  for (const [section, title] of Object.entries(SECTIONS)) {
    if (!budget[section]) throw new Error(`size-budget.json has no ${section}`);
    const rows = compare(measured[section], budget[section]);
    const table = formatTable(rows);
    console.log(`${title}\n${table}\n`);
    summary += `\n#### ${title}\n\n${table}\n`;
    over.push(...rows.filter((row) => !row.ok).map((row) => ({ ...row, title })));
  }
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
  if (over.length > 0) {
    for (const { title, kind, metric, size, limit } of over) {
      console.error(
        `${title}: ${kind} (${metric}) is ${formatBytes(size)}, over its ${formatBytes(limit)} budget`,
      );
    }
    console.error("Shrink the bundle, or raise the limit in scripts/size-budget.json and say why.");
    process.exit(1);
  }
}
