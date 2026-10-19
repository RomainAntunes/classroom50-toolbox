// The webview bundle's size budget: measures the built frontend's JavaScript and CSS, raw
// and gzipped, against the limits in scripts/size-budget.json, prints a table (also to
// $GITHUB_STEP_SUMMARY in CI) and fails when one is over.
// Usage (ci.yml, after `pnpm build`): node scripts/size-budget.mjs [build]
import { appendFileSync, readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
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

/** `12.3 kB`, `4.47 MB` (1000-based, as Vite's and browsers' reports). */
export function formatBytes(bytes) {
  if (bytes < 1000) return `${bytes} B`;
  // Rounded on whole numbers first: toFixed rounds the binary value, and 1.045 is stored as
  // 1.04499..., so 1,045,000 bytes would print 1.04 MB.
  const tenthsOfKb = Math.round(bytes / 100);
  if (tenthsOfKb < 10_000) return `${(tenthsOfKb / 10).toFixed(1)} kB`;
  return `${(Math.round(bytes / 10_000) / 100).toFixed(2)} MB`;
}

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
  const budget = JSON.parse(readFileSync(new URL("./size-budget.json", import.meta.url), "utf8"));
  const rows = compare(measure(walk(dir)), budget);
  const table = formatTable(rows);
  console.log(table);
  if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `### Webview bundle size\n\n${table}\n`);
  }
  const over = rows.filter((row) => !row.ok);
  if (over.length > 0) {
    for (const { kind, metric, size, limit } of over) {
      console.error(
        `${kind} (${metric}) is ${formatBytes(size)}, over its ${formatBytes(limit)} budget`,
      );
    }
    console.error("Shrink the bundle, or raise the limit in scripts/size-budget.json and say why.");
    process.exit(1);
  }
}
