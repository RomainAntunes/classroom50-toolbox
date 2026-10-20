import { describe, expect, it } from "vitest";
import { releaseNotesUrl } from "#lib/releaseNotes.js";

describe("releaseNotesUrl", () => {
  it("links gh to its release, with or without the v", () => {
    expect(releaseNotesUrl("gh", "2.102.0")).toBe(
      "https://github.com/cli/cli/releases/tag/v2.102.0",
    );
    expect(releaseNotesUrl("gh", "v2.102.0")).toBe(
      "https://github.com/cli/cli/releases/tag/v2.102.0",
    );
  });

  it("links gh-student to the Classroom 50 cli release", () => {
    expect(releaseNotesUrl("gh-student", "v1.58.2")).toBe(
      "https://github.com/foundation50/classroom50/releases/tag/cli-v1.58.2",
    );
  });

  it("has nothing for git, a missing version or anything that isn't one", () => {
    expect(releaseNotesUrl("git", "2.50.1")).toBeNull();
    expect(releaseNotesUrl("gh", null)).toBeNull();
    expect(releaseNotesUrl("gh", "2.83")).toBeNull();
    expect(releaseNotesUrl("gh-student", "v1.58.2/../../x")).toBeNull();
    expect(releaseNotesUrl("gh", "2.83.0-rc.1")).toBeNull();
  });
});
