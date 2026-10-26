// Pure parsing for `pnpm profile:power`: macOS `top -l` output, `ps` CPU times, the app's
// log lines and the processes it started, turned into the data the report draws.

/** The four processes, in the report's order, keyed as `meta.json` names them. */
export const PROCESSES = [
  { key: "app", name: "App (Rust)" },
  { key: "webcontent", name: "WebContent" },
  { key: "gpu", name: "WebKit GPU" },
  { key: "networking", name: "WebKit Networking" },
];

/** `29M-`, `6577K+`, `1.2G` in MB. */
export function memMb(text) {
  const value = text.replace(/[+-]$/, "");
  const unit = value.at(-1);
  const factor = { B: 1 / 1048576, K: 1 / 1024, M: 1, G: 1024 }[unit];
  return factor === undefined ? Number(value) / 1048576 : Number(value.slice(0, -1)) * factor;
}

/** `00:09.97`, `1:02:03.5`, `0:13.09` in seconds. */
export function cpuSeconds(text) {
  return text
    .replace(/[+-]$/, "")
    .split(":")
    .reduce((total, part) => total * 60 + Number(part), 0);
}

/**
 * @typedef {{ cpu: number, mem: number, power: number, idlew: number, threads: number }} Stat
 * @typedef {{ t: number, procs: Record<number, Stat> }} Sample
 */

/**
 * Each `top -l` sample: its local time (ms) and a row per pid of `pids`. Rows are read from
 * the right, so a command name with spaces doesn't shift them; `#TH` may read `23/1`.
 * @param {string} text
 * @param {number[]} pids
 * @returns {Sample[]}
 */
export function parseTop(text, pids) {
  /** @type {Sample[]} */
  const samples = [];
  let current = null;
  for (const line of text.split("\n")) {
    const stamp = line.match(/^(\d{4})\/(\d\d)\/(\d\d) (\d\d):(\d\d):(\d\d)/);
    if (stamp) {
      const [y, mo, d, h, mi, s] = stamp.slice(1).map(Number);
      current = { t: new Date(y, mo - 1, d, h, mi, s).getTime(), procs: {} };
      samples.push(current);
      continue;
    }
    const parts = line.trim().split(/\s+/);
    const pid = Number(parts[0]);
    if (!current || !pids.includes(pid) || parts.length < 8) continue;
    const [cpu, mem, power, idlew, threads] = parts.slice(-6);
    current.procs[pid] = {
      cpu: Number(cpu),
      mem: memMb(mem),
      power: Number(power),
      idlew: Number(idlew.replace(/[+-]$/, "")),
      threads: Number(threads.split("/")[0]),
    };
  }
  return samples;
}

/**
 * One row per interval between samples holding every process (the first sample has no CPU
 * delta yet): CPU %, memory, energy impact and idle wakeups per second, per process name.
 * @param {Sample[]} samples
 * @param {Record<string, number>} byName pid per process name
 */
export function toRows(samples, byName) {
  const pids = Object.values(byName);
  const full = samples.filter((s) => pids.every((pid) => s.procs[pid]));
  if (!full.length) return [];
  const t0 = full[0].t;
  return full.slice(1).map((sample, i) => {
    const prev = full[i];
    const seconds = (sample.t - prev.t) / 1000 || 2;
    /** @type {Record<string, any>} */
    const row = { t: (sample.t - t0) / 1000, clock: clock(sample.t) };
    for (const [name, pid] of Object.entries(byName)) {
      const a = prev.procs[pid];
      const b = sample.procs[pid];
      row[name] = {
        cpu: b.cpu,
        mem: Math.round(b.mem * 10) / 10,
        power: b.power,
        wake: Math.round((Math.max(0, b.idlew - a.idlew) / seconds) * 100) / 100,
        threads: b.threads,
      };
    }
    return row;
  });
}

/** `HH:MM:SS` of a local time in ms. */
function clock(ms) {
  const d = new Date(ms);
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
}

/** `ps -o pid=,time=,rss=` lines: CPU seconds and RSS (MB) per pid. */
export function parsePs(text) {
  /** @type {Record<number, { cpu: number, rss: number }>} */
  const out = {};
  for (const line of text.split("\n")) {
    const [pid, time, rss] = line.trim().split(/\s+/);
    if (pid && time && rss) out[Number(pid)] = { cpu: cpuSeconds(time), rss: Number(rss) / 1024 };
  }
  return out;
}

/**
 * A tauri-plugin-log line: `[2026-10-26][07:39:57][crate::module][INFO] text`, its date UTC.
 * @returns {{ t: number, module: string, level: string, msg: string } | null}
 */
export function parseLogLine(line) {
  const m = line.match(
    /^\[(\d{4})-(\d\d)-(\d\d)\]\[(\d\d):(\d\d):(\d\d)\]\[([^\]]+)\]\[(\w+)\] (.*)$/,
  );
  if (!m) return null;
  const [y, mo, d, h, mi, s] = m.slice(1, 7).map(Number);
  return {
    t: Date.UTC(y, mo - 1, d, h, mi, s),
    module: m[7].split("::").at(-1) ?? m[7],
    level: m[8],
    msg: m[9],
  };
}

/**
 * The processes the app started, from `<epoch s> <pid> <ppid> <command>` lines polled every
 * 0.5 s: once per pid, the first time seen, with the fullest command (`(gh)` is one exiting).
 */
export function parseChildren(text) {
  /** @type {Map<string, { at: number, cmd: string }>} */
  const seen = new Map();
  for (const line of text.split("\n")) {
    const m = line.match(/^(\d+)\s+(\d+)\s+\d+\s+(.+)$/);
    if (!m) continue;
    const [, at, pid, cmd] = m;
    const known = seen.get(pid);
    if (!known) seen.set(pid, { at: Number(at) * 1000, cmd: cmd.trim() });
    else if (known.cmd.startsWith("(") && !cmd.startsWith("(")) known.cmd = cmd.trim();
  }
  return [...seen.values()].sort((a, b) => a.at - b.at);
}

/** The last sample of `top -o power -stats pid,command,cpu,power,idlew`, top first. */
export function parseContext(text) {
  const blocks = text.split(/^PID\s/m);
  if (blocks.length < 3) return [];
  return blocks
    .at(-1)
    .split("\n")
    .slice(1)
    .map((line) => line.trim().split(/\s+/))
    .filter((parts) => parts.length >= 5 && /^\d+$/.test(parts[0]))
    .map((parts) => ({
      pid: Number(parts[0]),
      name: parts.slice(1, -3).join(" "),
      cpu: Number(parts.at(-3)),
      power: Number(parts.at(-2)),
    }));
}

/**
 * Syncs per local hour from the log's `<Kind> sync: N projects in Xs` lines, over the day
 * before `end` (ms).
 */
export function syncHistory(lines, end) {
  /** @type {Map<string, { hour: string, Status: number, Full: number, Local: number }>} */
  const hours = new Map();
  for (const line of lines) {
    const entry = parseLogLine(line);
    const m = entry?.msg.match(/^(Status|Full|Local) sync: /);
    if (!entry || !m || entry.t < end - 86_400_000 || entry.t > end) continue;
    const d = new Date(entry.t);
    const hour = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")} ${String(d.getHours()).padStart(2, "0")}:00`;
    const bucket = hours.get(hour) ?? { hour, Status: 0, Full: 0, Local: 0 };
    bucket[/** @type {"Status" | "Full" | "Local"} */ (m[1])] += 1;
    hours.set(hour, bucket);
  }
  return [...hours.values()].sort((a, b) => a.hour.localeCompare(b.hour));
}

/**
 * Averages over the rows, summed over the processes: what the comparison table shows.
 * @param {Record<string, any>[]} rows
 * @param {string[]} names
 */
export function summarize(rows, names) {
  const mean = (/** @type {(r: any) => number} */ f) =>
    rows.reduce((sum, row) => sum + f(row), 0) / (rows.length || 1);
  const sum = (/** @type {any} */ row, /** @type {string} */ field) =>
    names.reduce((total, name) => total + row[name][field], 0);
  return {
    cpuAvg: mean((row) => sum(row, "cpu")),
    power: mean((row) => sum(row, "power")),
    wake: mean((row) => sum(row, "wake")),
  };
}
