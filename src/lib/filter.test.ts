import { describe, expect, it } from "vitest";
import type { Assignment, Group, Project, Unaccepted } from "$lib/api";
import { inRange, showsProject, showsUnaccepted, type ListQuery } from "$lib/filter";

const now = new Date("2026-10-15T12:00:00Z");
const day = 24 * 60 * 60 * 1000;
const at = (days: number) => new Date(now.getTime() + days * day).toISOString();

const group: Group = {
  org: "o",
  slug: "classroom50-group-0123456789abcdef-1",
  number: 1,
  name: "The Sharks",
  url: "",
  members: [{ login: "jmaendly", name: "Julie Maendly" }],
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
  });
});
