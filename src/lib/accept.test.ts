import { describe, expect, it } from "vitest";
import type { Assignment, Classroom, Group, Unaccepted } from "#lib/api.js";
import { canFoundGroup, notes, repoName } from "#lib/accept.js";

const item = (assignment: Partial<Assignment>, group: Group | null = null): Unaccepted => ({
  id: "heigvd-poo/26-poo-c/labo-3",
  classroom: { org: "HEIGVD-POO", slug: "26-POO-c", name: "POO" } as Classroom,
  assignment: {
    slug: "Labo-3",
    name: "Labo 3",
    mode: "individual",
    due: null,
    submissionMode: null,
    feedbackPr: false,
    teamFormation: null,
    repoVisibility: null,
    ...assignment,
  } as Assignment,
  acceptUrl: "",
  group,
});

describe("repoName", () => {
  it("names the student's repo in lowercase, under the classroom's org", () => {
    expect(repoName(item({}), "Romain-A")).toBe("HEIGVD-POO/26-poo-c-labo-3-romain-a");
  });

  it("uses the group's number on team assignments", () => {
    const group = { number: 4 } as Group;
    expect(repoName(item({ mode: "team" }, group), "me")).toBe(
      "HEIGVD-POO/26-poo-c-labo-3-group-4",
    );
    expect(repoName(item({ mode: "team" }), "me")).toBe("HEIGVD-POO/26-poo-c-labo-3-group-<n>");
  });
});

describe("notes", () => {
  it("summarizes a private every-push assignment", () => {
    const text = notes(item({}), "me");
    expect(text).toContain("HEIGVD-POO/26-poo-c-labo-3-me, private");
    expect(text).toContain("Every push to the default branch");
    expect(text).not.toContain("Feedback");
    expect(text).toContain("offers to clone");
    expect(text).not.toMatch(/[{}]/);
  });

  it("covers public repos, Feedback PRs, submit tags, due dates and groups", () => {
    const text = notes(
      item({
        repoVisibility: "public",
        feedbackPr: true,
        submissionMode: "tag",
        due: "2026-10-20T21:59:00Z",
        mode: "team",
        teamFormation: "student",
      }),
      "me",
    );
    expect(text).toContain("public");
    expect(text).toContain("Feedback pull request");
    expect(text).toContain("Only what you submit counts");
    expect(text).toContain("marked late, never refused");
    expect(text).toContain("Name your new group below");
    expect(text).not.toMatch(/[{}]/);
  });
});

describe("canFoundGroup", () => {
  it("offers to found a group only on student-formed team assignments without one", () => {
    expect(canFoundGroup(item({ mode: "team", teamFormation: "student" }))).toBe(true);
    expect(canFoundGroup(item({ mode: "team", teamFormation: "teacher" }))).toBe(false);
    expect(canFoundGroup(item({ mode: "team" }, { number: 1 } as Group))).toBe(false);
    expect(canFoundGroup(item({ mode: "group" }))).toBe(false);
    expect(canFoundGroup(item({}))).toBe(false);
  });
});
