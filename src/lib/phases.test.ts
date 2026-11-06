import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Project, ProjectPhase } from "#lib/api.js";
import {
  draftFrom,
  draftProblem,
  localDate,
  newDraft,
  newId,
  nextDate,
  nextPhase,
  openPhases,
  toPhases,
} from "#lib/phases.js";

// Local dates below are in Zurich time.
const zone = process.env.TZ;
beforeAll(() => {
  process.env.TZ = "Europe/Zurich";
});
afterAll(() => {
  process.env.TZ = zone;
});

const phase = (id: string, due: string, done = false): ProjectPhase => ({
  id,
  name: `Phase ${id}`,
  due,
  done,
});

function project(phases: ProjectPhase[], over: { closed?: boolean; archived?: boolean } = {}) {
  return {
    id: "acme/db-project-alex",
    assignment: { closed: over.closed ?? false, due: null },
    remote: { archived: over.archived ?? false },
    phases,
  } as unknown as Project;
}

describe("open phases", () => {
  const phases = [
    phase("2", "2026-12-04T22:59:00Z"),
    phase("1", "2026-11-20T22:59:00Z", true),
    phase("3", "2026-11-27T22:59:00Z"),
  ];

  it("leaves out done ones and picks the earliest", () => {
    expect(openPhases(project(phases)).map((p) => p.id)).toEqual(["2", "3"]);
    expect(nextPhase(project(phases))?.id).toBe("3");
  });

  it("has none once submissions are closed or the repo archived", () => {
    expect(nextPhase(project(phases, { closed: true }))).toBeNull();
    expect(openPhases(project(phases, { archived: true }))).toEqual([]);
    expect(nextPhase(project([]))).toBeNull();
  });
});

describe("drafts", () => {
  it("round-trips local dates and times", () => {
    const phases = [phase("a", "2026-11-20T22:59:00Z", true)];
    const drafts = draftFrom(phases);
    expect(drafts).toEqual([
      { id: "a", name: "Phase a", date: "2026-11-20", time: "23:59", done: true },
    ]);
    expect(toPhases(drafts)).toEqual([{ ...phases[0], due: "2026-11-20T22:59:00.000Z" }]);
  });

  it("sorts by date and trims names when saving", () => {
    const drafts = [
      newDraft("b", " Report ", "2026-12-01"),
      { ...newDraft("a", "Draft", "2026-11-15"), time: "08:30" },
    ];
    expect(toPhases(drafts).map((p) => [p.id, p.name, p.due])).toEqual([
      ["a", "Draft", "2026-11-15T07:30:00.000Z"],
      ["b", "Report", "2026-12-01T22:59:00.000Z"],
    ]);
  });

  it("refuses a blank name, then a missing or impossible date, naming the phase", () => {
    const ok = newDraft("a", "Phase 1", "2026-11-15");
    const date = (index: number) => ({ kind: "date", index });
    expect(draftProblem([ok])).toBeNull();
    expect(draftProblem([])).toBeNull();
    expect(draftProblem([ok, { ...ok, id: "b", name: "  " }])).toEqual({ kind: "name", index: 1 });
    // A name problem comes first, wherever the date problem is.
    expect(
      draftProblem([
        { ...ok, date: "" },
        { ...ok, name: "" },
      ]),
    ).toEqual({
      kind: "name",
      index: 1,
    });
    expect(draftProblem([{ ...ok, date: "" }])).toEqual(date(0));
    expect(draftProblem([ok, { ...ok, date: "2026-02-31" }])).toEqual(date(1));
    expect(draftProblem([{ ...ok, date: "15/11/2026" }])).toEqual(date(0));
    expect(draftProblem([{ ...ok, time: "25:00" }])).toEqual(date(0));
  });

  it("dates a new phase a week after the latest one, or after today", () => {
    const now = new Date(2026, 9, 28, 15, 0);
    expect(nextDate([], now)).toBe("2026-11-04");
    // Over a month's end and the change from summer time.
    expect(nextDate([newDraft("a", "A", "2026-10-29")], now)).toBe("2026-11-05");
    expect(nextDate([newDraft("a", "A", "2026-12-28")], now)).toBe("2027-01-04");
    // A past phase or one without a date leaves today as the start.
    expect(nextDate([newDraft("a", "A", "2026-09-01"), newDraft("b", "B", "")], now)).toBe(
      "2026-11-04",
    );
  });

  it("takes the end of the day when the time is cleared", () => {
    expect(localDate("2026-11-15", "")).toEqual(new Date(2026, 10, 15, 23, 59));
  });
});

it("makes ids Rust accepts", () => {
  const id = newId();
  expect(id).toMatch(/^[0-9a-f]{32}$/);
  expect(newId()).not.toBe(id);
});
