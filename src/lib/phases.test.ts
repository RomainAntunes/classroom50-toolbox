import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Project, ProjectPhase } from "#lib/api.js";
import {
  draftFrom,
  draftProblem,
  localDate,
  newDraft,
  newId,
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
      { ...newDraft("b", " Report "), date: "2026-12-01" },
      { ...newDraft("a", "Draft"), date: "2026-11-15", time: "08:30" },
    ];
    expect(toPhases(drafts).map((p) => [p.id, p.name, p.due])).toEqual([
      ["a", "Draft", "2026-11-15T07:30:00.000Z"],
      ["b", "Report", "2026-12-01T22:59:00.000Z"],
    ]);
  });

  it("refuses a blank name, then a missing or impossible date", () => {
    const ok = { ...newDraft("a", "Phase 1"), date: "2026-11-15" };
    expect(draftProblem([ok])).toBeNull();
    expect(draftProblem([])).toBeNull();
    expect(draftProblem([ok, { ...ok, id: "b", name: "  " }])).toBe("name");
    expect(draftProblem([{ ...ok, date: "" }])).toBe("date");
    expect(draftProblem([{ ...ok, date: "2026-02-31" }])).toBe("date");
    expect(draftProblem([{ ...ok, date: "15/11/2026" }])).toBe("date");
    expect(draftProblem([{ ...ok, time: "25:00" }])).toBe("date");
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
