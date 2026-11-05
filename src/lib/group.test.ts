import { describe, expect, it } from "vitest";
import type { Group, Snapshot } from "#lib/api.js";
import { groupDetails, groupFull, groupName } from "#lib/group.js";
import { session } from "#lib/session.svelte.js";

const group = (name: string | null): Group => ({
  org: "o",
  slug: "classroom50-group-0123456789abcdef-4",
  number: 4,
  name,
  founders: null,
  url: "",
  classroom: null,
  assignment: null,
  members: [
    { login: "Alice", name: "Alice Martin", email: null },
    { login: "bob", name: null, email: null },
  ],
});

describe("group details", () => {
  it("uses the founders' name, else the group number", () => {
    expect(groupName(group("The Sharks"))).toBe("The Sharks");
    expect(groupName(group(null))).toBe("Group 4");
  });

  it("lists one member per line, names first, the user marked", () => {
    session.snapshot = { login: "alice" } as Snapshot;
    const [[, name], [, members]] = groupDetails(group(null));
    expect(name).toBe("Group 4");
    expect(members).toBe("Alice Martin (you)\nbob");
  });

  it("shows the size against the assignment's limit, and flags a group past it", () => {
    session.snapshot = { login: "alice" } as Snapshot;
    expect(groupDetails(group(null))).toHaveLength(2);
    const [, , [label, size]] = groupDetails(group(null), 3);
    expect(label).toBe("Size");
    expect(size).toBe("2 of 3 members");
    const [, , [, over]] = groupDetails(group(null), 1);
    expect(over).toBe("2 of 1 members\nMore members than this assignment allows");
  });

  it("is full once its members reach the limit", () => {
    expect(groupFull(group(null), 2)).toBe(true);
    expect(groupFull(group(null), 3)).toBe(false);
    expect(groupFull(group(null), null)).toBe(false);
    expect(groupFull(null, 2)).toBe(false);
  });
});
