import { afterEach, describe, expect, it, vi } from "vitest";
import { duration, scrollBehavior } from "#lib/motion.js";

function reduceMotion(reduce: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: reduce && query.includes("reduce"),
  }));
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("motion", () => {
  it("keeps smooth scrolling and transitions by default", () => {
    reduceMotion(false);
    expect(scrollBehavior()).toBe("smooth");
    expect(duration(150)).toBe(150);
  });

  it("jumps and skips transitions when the system asks for less motion", () => {
    reduceMotion(true);
    expect(scrollBehavior()).toBe("auto");
    expect(duration(150)).toBe(0);
  });

  it("animates when the setting can't be read", () => {
    expect(scrollBehavior()).toBe("smooth");
  });
});
