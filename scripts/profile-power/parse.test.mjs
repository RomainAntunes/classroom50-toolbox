import { describe, expect, it } from "vitest";
import {
  cpuSeconds,
  memMb,
  parseChildren,
  parseContext,
  parseLogLine,
  parsePs,
  parseTop,
  summarize,
  syncHistory,
  toRows,
} from "./parse.mjs";

// Two samples of `top -l` as macOS writes them, header lines included.
const TOP = `Processes: 873 total, 3 running, 870 sleeping, 4156 threads
2026/10/26 08:45:20
Load Avg: 1.96, 2.10, 2.14
CPU usage: 5.19% user, 10.81% sys, 83.99% idle
Networks: packets: 7241127/1222M in, 6985219/1082M out.

PID    COMMAND          %CPU MEM   POWER IDLEW #TH TIME
62562  com.apple.WebKit 0.0  36M   0.0   241   3   00:00.55
62550  classroom50-tool 0.0  24M   0.0   447   16  00:00.71
Processes: 876 total, 2 running, 874 sleeping, 4163 threads
2026/10/26 08:45:22
Load Avg: 1.96, 2.10, 2.14

PID    COMMAND          %CPU MEM   POWER IDLEW #TH TIME
62562  com.apple.WebKit 0.4  57M+  0.6   251+  4   00:00.56
62550  classroom50-tool 5.6  6577K- 1.2  487+  23/1 00:00.83
`;

describe("profile-power parsing", () => {
  it("reads memory units and CPU times", () => {
    expect(memMb("29M-")).toBe(29);
    expect(memMb("6577K+")).toBeCloseTo(6.42, 2);
    expect(memMb("1.5G")).toBe(1536);
    expect(cpuSeconds("00:09.97")).toBeCloseTo(9.97);
    expect(cpuSeconds("1:02:03.5")).toBeCloseTo(3723.5);
  });

  it("reads each top sample for the given pids, threads written 23/1 included", () => {
    const samples = parseTop(TOP, [62550, 62562]);
    expect(samples).toHaveLength(2);
    expect(samples[1].t - samples[0].t).toBe(2000);
    expect(samples[1].procs[62550]).toEqual({
      cpu: 5.6,
      mem: 6577 / 1024,
      power: 1.2,
      idlew: 487,
      threads: 23,
    });
    expect(parseTop(TOP, [1])[0].procs).toEqual({});
  });

  it("turns samples into rows with wakeups per second", () => {
    const rows = toRows(parseTop(TOP, [62550, 62562]), { App: 62550, WebContent: 62562 });
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ t: 2, clock: "08:45:22" });
    expect(rows[0].App).toMatchObject({ cpu: 5.6, power: 1.2, wake: 20, threads: 23 });
    expect(rows[0].WebContent).toMatchObject({ mem: 57, wake: 5 });
    const summary = summarize(rows, ["App", "WebContent"]);
    expect(summary.cpuAvg).toBeCloseTo(6);
    expect(summary.power).toBeCloseTo(1.8);
    expect(summary.wake).toBeCloseTo(25);
  });

  it("drops samples missing a process", () => {
    expect(toRows(parseTop(TOP, [62550]), { App: 62550, Gone: 99 })).toEqual([]);
  });

  it("reads ps lines", () => {
    expect(parsePs("62550   0:13.09 107680\n")).toEqual({ 62550: { cpu: 13.09, rss: 105.15625 } });
  });

  it("reads log lines in UTC", () => {
    expect(
      parseLogLine(
        "[2026-10-26][07:39:57][classroom50_toolbox_lib::projects::sync][INFO] Status sync: 7 projects in 1.3s",
      ),
    ).toEqual({
      t: Date.UTC(2026, 9, 26, 7, 39, 57),
      module: "sync",
      level: "INFO",
      msg: "Status sync: 7 projects in 1.3s",
    });
    expect(parseLogLine("not a log line")).toBeNull();
  });

  it("keeps each child once, with its fullest command", () => {
    const text = [
      "1793000833 64611 62550 (gh)",
      "1793000833 64611 62550 gh auth token --hostname github.com",
      "1793000834 64635 62550 gh api --include user",
      "1793000834 64635 62550 gh api --include user",
    ].join("\n");
    expect(parseChildren(text)).toEqual([
      { at: 1793000833000, cmd: "gh auth token --hostname github.com" },
      { at: 1793000834000, cmd: "gh api --include user" },
    ]);
  });

  it("reads the last sample of the system-wide snapshot", () => {
    const text = `PID    COMMAND          %CPU POWER IDLEW
403    WindowServer     40.0 45.0  12+
Processes: 1 total
PID    COMMAND          %CPU POWER IDLEW
403    WindowServer     5.0  11.3  2253+
80480  bridge-gui       0.8  6.2   900
`;
    expect(parseContext(text)).toEqual([
      { pid: 403, name: "WindowServer", cpu: 5, power: 11.3 },
      { pid: 80480, name: "bridge-gui", cpu: 0.8, power: 6.2 },
    ]);
    expect(parseContext("")).toEqual([]);
  });

  it("counts syncs per hour over the last day only", () => {
    const end = Date.UTC(2026, 9, 26, 8, 0, 0);
    const line = (/** @type {string} */ at, /** @type {string} */ kind) =>
      `[2026-10-${at}][classroom50_toolbox_lib::projects::sync][INFO] ${kind} sync: 7 projects in 1.0s`;
    const history = syncHistory(
      [
        line("24][07:00:00", "Status"),
        line("26][07:01:00", "Status"),
        line("26][07:02:00", "Full"),
        line("26][07:03:00", "Status"),
        "[2026-10-26][07:40:00][classroom50_toolbox_lib::watch][INFO] rescanning",
      ],
      end,
    );
    expect(history).toHaveLength(1);
    expect(history[0]).toMatchObject({ Status: 2, Full: 1, Local: 0 });
  });
});
