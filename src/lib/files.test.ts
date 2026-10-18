import { describe, expect, it } from "vitest";
import { changedLines } from "#lib/files.js";

describe("changedLines", () => {
  it("lists the paths one per line", () => {
    expect(changedLines({ changedFiles: 2, changedPaths: ["a.c", "src/"] })).toBe("a.c\nsrc/");
  });

  it("counts the paths past those shown", () => {
    const changedPaths = Array.from({ length: 20 }, (_, i) => `f${i}.c`);
    const lines = changedLines({ changedFiles: 23, changedPaths }, 10).split("\n");
    expect(lines).toHaveLength(11);
    expect(lines[10]).toBe("and 13 more");
    expect(changedLines({ changedFiles: 21, changedPaths }).split("\n")[20]).toBe("and 1 more");
  });

  it("falls back to the count when no path is known", () => {
    expect(changedLines({ changedFiles: 3, changedPaths: [] })).toBe("3");
    expect(changedLines({ changedFiles: 3 })).toBe("3");
  });
});
