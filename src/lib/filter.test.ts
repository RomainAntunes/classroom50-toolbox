import { describe, expect, it } from "vitest";
import type { Assignment, Group, Project, Unaccepted } from "#lib/api.js";
import { inRange, showsProject, showsUnaccepted, type ListQuery } from "#lib/filter.js";

const now = new Date("2026-10-15T12:00:00Z");
const day = 24 * 60 * 60 * 1000;
const at = (days: number) => new Date(now.getTime() + days * day).toISOString();

const group: Group = {
  org: "o",
  slug: "classroom50-group-0123456789abcdef-1",
  number: 1,
  name: "The Sharks",
  founders: null,
  url: "",
  classroom: null,
  assignment: null,
  members: [{ login: "jmaendly", name: "Julie Maendly", email: null }],
};

function project(id: string, due: string | null, withGroup = false): Project {
  return {
    id,
    owner: "o",
    repo: id,
    config: { classroom: "poo", assignment: id },
    classroom: null,
    assignment: { name: `Lab ${id}`, due, closed: false } as Assignment,
    remote: null,
    group: withGroup ? group : null,
  } as unknown as Project;
}

const list = (patch: Partial<ListQuery> = {}): ListQuery => ({
  filter: "all",
  query: "",
  hidden: [],
  hiddenClasses: [],
  now,
  ...patch,
});

describe("date filter", () => {
  it("sorts by open, this week and past", () => {
    expect(inRange("open", at(10), false, now)).toBe(true);
    expect(inRange("open", null, false, now)).toBe(true);
    expect(inRange("open", at(1), true, now)).toBe(false);
    expect(inRange("week", at(3), false, now)).toBe(true);
    expect(inRange("week", at(10), false, now)).toBe(false);
    expect(inRange("past", at(-1), false, now)).toBe(true);
    expect(inRange("past", at(1), true, now)).toBe(true);
    expect(inRange("all", at(-100), true, now)).toBe(true);
  });
});

describe("search and hidden assignments", () => {
  it("finds an assignment by teammate or group name", () => {
    const shared = project("lab2", null, true);
    expect(showsProject(shared, list({ query: "maendly" }))).toBe(true);
    expect(showsProject(shared, list({ query: "sharks" }))).toBe(true);
    expect(showsProject(project("lab3", null), list({ query: "maendly" }))).toBe(false);
  });

  it("leaves hidden ones out unless a search matches them", () => {
    const hidden = project("lab1", null);
    expect(showsProject(hidden, list({ hidden: ["lab1"] }))).toBe(false);
    expect(showsProject(hidden, list({ hidden: ["lab1"], query: "lab1" }))).toBe(true);
    expect(showsProject(hidden, list({ hidden: ["lab1"], query: "other" }))).toBe(false);
  });

  it("leaves a hidden class out unless a search matches", () => {
    const lab = project("lab1", null);
    expect(showsProject(lab, list({ hiddenClasses: ["o/poo"] }))).toBe(false);
    expect(showsProject(lab, list({ hiddenClasses: ["o/other"] }))).toBe(true);
    expect(showsProject(lab, list({ hiddenClasses: ["o/poo"], query: "lab" }))).toBe(true);
  });

  it("applies the same rules to assignments not accepted yet", () => {
    const item = {
      id: "o/poo/lab4",
      classroom: { org: "o", slug: "poo", name: null },
      assignment: { name: "Lab 4", slug: "lab4", due: null, closed: false },
      group,
    } as unknown as Unaccepted;
    expect(showsUnaccepted(item, list())).toBe(true);
    expect(showsUnaccepted(item, list({ hidden: [item.id] }))).toBe(false);
    expect(showsUnaccepted(item, list({ hidden: [item.id], query: "julie" }))).toBe(true);
    expect(showsUnaccepted(item, list({ hiddenClasses: ["o/poo"] }))).toBe(false);
  });
});

describe("projects added by hand", () => {
  const manual = (due: string | null, done = false): Project =>
    ({
      ...project("manual:1", null),
      owner: "alice",
      config: { classroom: "Writing", assignment: "Essay" },
      assignment: null,
      manual: { due, notes: null, done, repo: null },
    }) as unknown as Project;

  it("go by their own due date and Done box", () => {
    expect(showsProject(manual(at(3)), list({ filter: "week" }))).toBe(true);
    expect(showsProject(manual(at(3), true), list({ filter: "week" }))).toBe(false);
    expect(showsProject(manual(at(3), true), list({ filter: "past" }))).toBe(true);
    expect(showsProject(manual(null), list({ filter: "open" }))).toBe(true);
  });

  it("are filed under their class alone, whoever owns the repo", () => {
    expect(showsProject(manual(null), list({ hiddenClasses: ["/writing"] }))).toBe(false);
    expect(showsProject(manual(null), list({ hiddenClasses: ["alice/writing"] }))).toBe(true);
  });
});
