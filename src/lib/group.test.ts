import { describe, expect, it } from "vitest";
import type { Group, Snapshot } from "#lib/api.js";
import { groupDetails, groupName } from "#lib/group.js";
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
});
