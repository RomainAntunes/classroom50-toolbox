import { describe, expect, it } from "vitest";
import type { Assignment, Group, Project } from "$lib/api";
import { askText, prompt } from "$lib/prompt.svelte";
import { joinsGroup } from "$lib/teammate";

describe("joinsGroup", () => {
  const project = (mode: Assignment["mode"] | null, group: Group | null = null) =>
    ({ group, assignment: mode ? ({ mode } as Assignment) : null }) as Project;

  it("adds to the group on team assignments", () => {
    expect(joinsGroup(project("team"))).toBe(true);
    expect(joinsGroup(project(null, {} as Group))).toBe(true);
  });

  it("invites on the others", () => {
    expect(joinsGroup(project("individual"))).toBe(false);
    expect(joinsGroup(project("group"))).toBe(false);
    expect(joinsGroup(project(null))).toBe(false);
  });
});

describe("askText", () => {
  const options = { title: "Add", okLabel: "Add" };

  it("resolves to the trimmed text and closes", async () => {
    const answer = askText(options);
    prompt.current?.resolve("  alice ");
    expect(await answer).toBe("alice");
    expect(prompt.current).toBeNull();
  });

  it("treats empty text as cancelled", async () => {
    const answer = askText(options);
    prompt.current?.resolve("   ");
    expect(await answer).toBeNull();
  });

  it("cancels the previous prompt when a new one opens", async () => {
    const first = askText(options);
    const second = askText(options);
    expect(await first).toBeNull();
    prompt.current?.resolve("bob");
    expect(await second).toBe("bob");
  });
});
