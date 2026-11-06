import { describe, expect, it } from "vitest";
import type { Assignment, Group, Project } from "#lib/api.js";
import { askText, prompt } from "#lib/prompt.svelte.js";
import {
  joinsGroup,
  notFounderReason,
  parseUsernames,
  resultsText,
  roomLeft,
  usernamesHint,
} from "#lib/teammate.js";

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

describe("parseUsernames", () => {
  it("splits on commas, semicolons and spaces, drops @ and repeats", () => {
    expect(parseUsernames(" @alice, bob;carol\n  dave  ALICE ,, ")).toEqual([
      "alice",
      "bob",
      "carol",
      "dave",
    ]);
    expect(parseUsernames(" , @ ")).toEqual([]);
  });
});

describe("notFounderReason", () => {
  const group = (founders: string[] | null) => ({ founders }) as Group;

  it("lets the founder add, and doesn't guess when roles are unknown", () => {
    expect(notFounderReason(group(["Me"]), "me", false)).toBeNull();
    expect(notFounderReason(group(null), "me", false)).toBeNull();
    expect(notFounderReason(null, "me", false)).toBeNull();
    expect(notFounderReason(group(["alice"]), null, false)).toBeNull();
  });

  it("says who can add instead", () => {
    expect(notFounderReason(group(["alice"]), "me", false)).toContain("ask alice");
    expect(notFounderReason(group([]), "me", false)).toContain("didn't found");
    expect(notFounderReason(group([]), "me", true)).toContain("teacher assigns");
  });
});

describe("resultsText", () => {
  it("gives one line per username", () => {
    const text = resultsText(
      [
        { username: "alice", error: null },
        {
          username: "x--y",
          error: { kind: "invalidUsername", message: "", params: { username: "x--y" } },
        },
      ],
      { repo: "o/r", assignment: "Lab", team: true },
    );
    expect(text.split("\n")).toEqual([
      "alice is in your group now. They should accept Lab next.",
      "Couldn't add x--y: \"x--y\" isn't a GitHub username",
    ]);
  });
});

describe("roomLeft", () => {
  it("counts members and queued classmates once each, whatever their case", () => {
    expect(roomLeft(3, ["me"])).toBe(2);
    expect(roomLeft(3, ["me", "Bob"], ["bob", "carol"])).toBe(0);
    expect(roomLeft(2, ["me", "bob", "carol"])).toBe(0);
  });

  it("is null without a limit", () => {
    expect(roomLeft(null, ["me"])).toBeNull();
  });
});

describe("usernamesHint", () => {
  it("says how to type several, and the room left, before anything is typed", () => {
    expect(usernamesHint("", true, null)).toEqual({
      text: "Separate several usernames with commas or spaces.",
      warning: null,
    });
    expect(usernamesHint(" , ", true, 2)).toEqual({
      text: "Separate several usernames with commas or spaces.\nYour group has room for 2 more.",
      warning: null,
    });
    expect(usernamesHint("", true, 0)).toEqual({
      text: "Separate several usernames with commas or spaces.",
      warning: "Your group is full.",
    });
  });

  it("names who the answer adds or invites", () => {
    expect(usernamesHint("@bob carol", true, 2)).toEqual({
      text: "Adds bob and carol.",
      warning: null,
    });
    expect(usernamesHint("bob", false, null).text).toBe("Invites bob.");
  });

  it("warns when more are typed than fit", () => {
    expect(usernamesHint("bob carol dave", true, 1)).toEqual({
      text: "Adds bob, carol, and dave.",
      warning: "Only 1 more fits in your group.",
    });
    expect(usernamesHint("bob", true, 0).warning).toBe("Your group is full.");
  });
});
