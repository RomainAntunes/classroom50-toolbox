import { describe, expect, it } from "vitest";
import { isMenuKey, moveIndex } from "$lib/keys";

describe("moveIndex", () => {
  it("moves one row up or down and stops at the ends", () => {
    expect(moveIndex("ArrowDown", 0, 3)).toBe(1);
    expect(moveIndex("ArrowUp", 1, 3)).toBe(0);
    expect(moveIndex("ArrowDown", 2, 3)).toBe(2);
    expect(moveIndex("ArrowUp", 0, 3)).toBe(0);
  });

  it("jumps to the first and last rows", () => {
    expect(moveIndex("Home", 2, 5)).toBe(0);
    expect(moveIndex("End", 0, 5)).toBe(4);
  });

  it("starts at an end when nothing has the focus", () => {
    expect(moveIndex("ArrowDown", -1, 3)).toBe(0);
    expect(moveIndex("ArrowUp", -1, 3)).toBe(2);
  });

  it("ignores other keys, Left and Right in a list, and empty lists", () => {
    expect(moveIndex("Enter", 0, 3)).toBeNull();
    expect(moveIndex("ArrowRight", 0, 3)).toBeNull();
    expect(moveIndex("ArrowDown", 0, 0)).toBeNull();
  });

  it("moves sideways and wraps around when asked", () => {
    const radios = { horizontal: true, wrap: true };
    expect(moveIndex("ArrowRight", 3, 4, radios)).toBe(0);
    expect(moveIndex("ArrowLeft", 0, 4, radios)).toBe(3);
    expect(moveIndex("ArrowDown", 1, 4, radios)).toBe(2);
  });

  it("moves the way the arrow points when laid out right to left", () => {
    const radios = { horizontal: true, wrap: true, rtl: true };
    expect(moveIndex("ArrowLeft", 1, 4, radios)).toBe(2);
    expect(moveIndex("ArrowRight", 1, 4, radios)).toBe(0);
    expect(moveIndex("ArrowLeft", 3, 4, radios)).toBe(0);
    expect(moveIndex("ArrowDown", 1, 4, radios)).toBe(2);
    expect(moveIndex("ArrowLeft", 1, 4, { rtl: true })).toBeNull();
  });
});

describe("isMenuKey", () => {
  it("knows the Menu key and Shift+F10", () => {
    expect(isMenuKey({ key: "ContextMenu", shiftKey: false })).toBe(true);
    expect(isMenuKey({ key: "F10", shiftKey: true })).toBe(true);
    expect(isMenuKey({ key: "F10", shiftKey: false })).toBe(false);
    expect(isMenuKey({ key: "Enter", shiftKey: true })).toBe(false);
  });
});
