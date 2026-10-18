import { afterEach, describe, expect, it } from "vitest";
import { errorMessage } from "#lib/api.js";
import { setLocale } from "#lib/i18n/index.js";

afterEach(() => {
  setLocale("en");
});

describe("errorMessage", () => {
  it("words a known kind in the UI's language, filling in its params", () => {
    setLocale("fr");
    const err = {
      kind: "otherBranch",
      message: "the clone isn't on main",
      params: { branch: "main" },
    };
    expect(errorMessage(err)).toBe("le clone n'est pas sur main");
  });

  it("keeps Rust's message when a param is missing", () => {
    const err = { kind: "notInstalled", message: "`gh` is not installed or not on PATH" };
    expect(errorMessage(err)).toBe("`gh` is not installed or not on PATH");
  });

  it("keeps Rust's message for kinds the UI doesn't word", () => {
    const err = { kind: "commandFailed", message: "`gh` exited with code Some(1): boom" };
    expect(errorMessage(err)).toBe("`gh` exited with code Some(1): boom");
  });

  it("shows anything else as text", () => {
    expect(errorMessage("plain")).toBe("plain");
  });
});
