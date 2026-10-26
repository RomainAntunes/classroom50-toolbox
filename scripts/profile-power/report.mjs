// Builds report.html from a `pnpm profile:power` capture: stat tiles, CPU, energy impact
// and memory over time with the app's log events, idle wakeups, the rest of the Mac, syncs
// per hour, what the app ran, and tables. With --compare, a before/after table against an
// older capture (the previous release, a build without a fix).
// Usage: node scripts/profile-power/report.mjs <capture dir> [--compare <older capture dir>]
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  PROCESSES,
  parseChildren,
  parseContext,
  parseLogLine,
  parsePs,
  parseTop,
  summarize,
  syncHistory,
  toRows,
} from "./parse.mjs";

const read = (/** @type {string} */ dir, /** @type {string} */ file) =>
  existsSync(join(dir, file)) ? readFileSync(join(dir, file), "utf8") : "";

/** Everything the report draws, from one capture folder. */
export function load(/** @type {string} */ dir) {
  const meta = JSON.parse(read(dir, "meta.json"));
  /** @type {Record<string, number>} */
  const byName = {};
  for (const { key, name } of PROCESSES) if (meta[key]) byName[name] = meta[key];
  const pids = Object.values(byName);
  const samples = parseTop(read(dir, "top.txt"), pids);
  const rows = toRows(samples, byName);
  if (!rows.length) throw new Error(`${dir}: no complete top sample`);
  const t0 = samples.find((s) => pids.every((pid) => s.procs[pid]))?.t ?? 0;
  const end = t0 + rows.at(-1).t * 1000;

  const before = parsePs(read(dir, "cputime_start.txt"));
  const after = parsePs(read(dir, "cputime_end.txt"));
  /** @type {Record<string, any>} */
  const totals = {};
  for (const [name, pid] of Object.entries(byName)) {
    // A process gone by the end (the app quit): its last top sample stands in for ps.
    const last = samples.findLast((s) => s.procs[pid])?.procs[pid];
    totals[name] = {
      cpuSeconds: after[pid] ? after[pid].cpu - (before[pid]?.cpu ?? 0) : null,
      rssStart: before[pid]?.rss ?? null,
      rssEnd: after[pid]?.rss ?? last?.mem ?? null,
    };
  }

  const events = read(dir, "app.log")
    .split("\n")
    .map(parseLogLine)
    .filter((e) => e && e.t >= t0 - 5000 && e.t <= end + 5000)
    .map((e) => ({
      t: Math.round((e.t - t0) / 100) / 10,
      module: e.module,
      level: e.level,
      msg: e.msg.replace(/\/Users\/[^/"]+/g, "~"),
    }));
  const children = parseChildren(read(dir, "children.txt"))
    .filter((c) => c.at >= t0 - 2000)
    .map((c) => ({ t: Math.round((c.at - t0) / 100) / 10, cmd: c.cmd }));
  const battery = (/** @type {string} */ file) => read(dir, file).replace(/\s+/g, " ").trim();
  const stamp = (/** @type {number} */ ms) => new Date(ms).toLocaleString("sv-SE");

  return {
    version: meta.version,
    pids,
    names: Object.keys(byName),
    start: stamp(t0),
    end: stamp(end).slice(11),
    duration: Math.round((end - t0) / 1000),
    endedEarly: existsSync(join(dir, "ended_early.txt")),
    rows,
    totals,
    events,
    children,
    history: syncHistory(read(dir, "sync-history.log").split("\n"), end),
    context: parseContext(read(dir, "context.txt")),
    battStart: battery("batt_start.txt"),
    battEnd: battery("batt_end.txt"),
  };
}

/** The comparison table's row for one capture. */
function compareRow(/** @type {ReturnType<typeof load>} */ data) {
  return {
    version: data.version,
    start: data.start,
    duration: data.duration,
    cpuSeconds: Object.values(data.totals).reduce((sum, t) => sum + (t.cpuSeconds ?? 0), 0),
    ...summarize(data.rows, data.names),
    syncs: data.events.filter((e) => / sync: /.test(e.msg)).length,
  };
}

function main() {
  const args = process.argv.slice(2);
  const at = args.indexOf("--compare");
  const older = at >= 0 ? args.splice(at, 2)[1] : null;
  const dir = args[0];
  if (!dir) {
    console.error("Usage: node scripts/profile-power/report.mjs <dir> [--compare <older dir>]");
    process.exit(1);
  }
  const data = load(dir);
  const compare = older ? [compareRow(load(older)), compareRow(data)] : [];
  const template = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "template.html"),
    "utf8",
  );
  // `<` escaped so a log line can't close the script element.
  const json = JSON.stringify({ ...data, compare }).replace(/</g, "\\u003c");
  const out = join(dir, "report.html");
  writeFileSync(out, template.replace("/*DATA*/ null", json));
  console.log(resolve(out));
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) main();
