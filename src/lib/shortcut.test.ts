import { describe, expect, it } from "vitest";
import {
  formatAccelerator,
  formatShortcut,
  KEYS,
  parseShortcut,
  recordKey,
  type KeyPress,
} from "#lib/shortcut.js";

function press(code: string, key: string, mods: Partial<KeyPress> = {}): KeyPress {
  return { code, key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods };
}

// English words, as `t("shortcut.key." + name)` gives them.
const word = (name: string) =>
  ({ ctrl: "Ctrl", alt: "Alt", shift: "Shift", win: "Win", super: "Super", space: "Space" })[
    name
  ] ?? name;

describe("recordKey", () => {
  it("records a key with a modifier in the stored order", () => {
    expect(recordKey(press("KeyC", "ç", { altKey: true, metaKey: true }), "macos")).toEqual({
      kind: "shortcut",
      accelerator: "Alt+Super+KeyC",
    });
    expect(
      recordKey(press("F5", "F5", { shiftKey: true, ctrlKey: true, altKey: true }), "windows"),
    ).toEqual({ kind: "shortcut", accelerator: "Control+Alt+Shift+F5" });
  });

  it("waits while only modifiers are held", () => {
    expect(recordKey(press("AltLeft", "Alt", { altKey: true }), "linux")).toEqual({
      kind: "modifiers",
      held: ["Alt"],
    });
    expect(recordKey(press("MetaRight", "Meta", { metaKey: true }), "macos").kind).toBe(
      "modifiers",
    );
  });

  it("asks for a modifier other than Shift", () => {
    expect(recordKey(press("KeyA", "a"), "macos").kind).toBe("needsModifier");
    expect(recordKey(press("KeyA", "A", { shiftKey: true }), "windows")).toEqual({
      kind: "needsModifier",
      held: ["Shift"],
    });
  });

  it("refuses keys the OS keeps", () => {
    for (const code of ["Escape", "Tab", "CapsLock", "MediaPlayPause", "Numpad1"]) {
      expect(recordKey(press(code, code, { ctrlKey: true }), "macos").kind).toBe("unsupported");
    }
  });

  it("takes letters from the layout on Windows and Linux, by position on macOS", () => {
    // The A key of an AZERTY keyboard sits where QWERTY has Q.
    const azertyA = press("KeyQ", "a", { ctrlKey: true, altKey: true });
    expect(recordKey(azertyA, "windows")).toEqual({
      kind: "shortcut",
      accelerator: "Control+Alt+KeyA",
    });
    expect(recordKey(azertyA, "linux")).toEqual({
      kind: "shortcut",
      accelerator: "Control+Alt+KeyA",
    });
    expect(recordKey(azertyA, "macos")).toEqual({
      kind: "shortcut",
      accelerator: "Control+Alt+KeyQ",
    });
    // AltGr typed a character: the key's position stands.
    expect(recordKey(press("KeyE", "€", { ctrlKey: true, altKey: true }), "windows")).toEqual({
      kind: "shortcut",
      accelerator: "Control+Alt+KeyE",
    });
  });
});

describe("parseShortcut", () => {
  it("reads stored accelerators", () => {
    expect(parseShortcut("Control+Alt+KeyC")).toEqual({
      modifiers: ["Control", "Alt"],
      key: "KeyC",
    });
  });

  it("rejects what Rust would", () => {
    for (const bad of [
      "",
      "KeyC",
      "Shift+KeyC",
      "Control+Control+KeyC",
      "Control+Escape",
      "Hyper+KeyC",
      "Control+KeyC+Alt",
    ]) {
      expect(parseShortcut(bad), bad).toBeNull();
    }
  });

  it("knows every key Rust accepts", () => {
    // Rust's `KEYS`: 26 letters, 10 digits, F1 to F24, 9 named keys, 4 arrows, 11 marks.
    expect(KEYS.size).toBe(26 + 10 + 24 + 9 + 4 + 11);
  });
});

describe("formatShortcut", () => {
  it("writes macOS's symbols", () => {
    expect(formatAccelerator("Alt+Super+KeyC", "macos", word)).toBe("⌥⌘C");
    expect(formatAccelerator("Control+Alt+Shift+Super+Digit1", "macos", word)).toBe("⌃⌥⇧⌘1");
    expect(formatAccelerator("Super+Space", "macos", word)).toBe("⌘Space");
    expect(formatAccelerator("Control+Enter", "macos", word)).toBe("⌃↩");
    expect(formatAccelerator("Super+F12", "macos", word)).toBe("⌘F12");
  });

  it("writes words joined by + on Windows and Linux", () => {
    expect(formatAccelerator("Control+Alt+KeyC", "windows", word)).toBe("Ctrl+Alt+C");
    expect(formatAccelerator("Shift+Super+ArrowUp", "windows", word)).toBe("Shift+Win+↑");
    expect(formatAccelerator("Control+Super+Slash", "linux", word)).toBe("Ctrl+Super+/");
    expect(formatAccelerator("Alt+Space", "linux", word)).toBe("Alt+Space");
  });

  it("writes modifiers alone while recording", () => {
    expect(formatShortcut(["Super", "Alt"], null, "macos", word)).toBe("⌥⌘");
    expect(formatShortcut(["Shift", "Control"], null, "windows", word)).toBe("Ctrl+Shift");
  });

  it("gives up on what it can't read", () => {
    expect(formatAccelerator("Shift+KeyC", "macos", word)).toBeNull();
  });
});
