import { afterEach, describe, expect, it } from "vitest";
import { errorMessage } from "$lib/api";
import { catalogs, setLocale } from "$lib/i18n";

afterEach(() => {
  setLocale("en");
  delete catalogs.fr;
});

describe("errorMessage", () => {
  it("words a known kind in the UI's language, filling in its params", () => {
    catalogs.fr = { ...catalogs.en, "error.otherBranch": "le clone n'est pas sur {branch}" };
    setLocale("fr");
    const err = {
      kind: "otherBranch",
      message: "the clone isn't on main",
      params: { branch: "main" },
    };
    expect(errorMessage(err)).toBe("le clone n'est pas sur main");
  });

  it("keeps Rust's message for kinds the UI doesn't word", () => {
    const err = { kind: "commandFailed", message: "`gh` exited with code Some(1): boom" };
    expect(errorMessage(err)).toBe("`gh` exited with code Some(1): boom");
  });

  it("shows anything else as text", () => {
    expect(errorMessage("plain")).toBe("plain");
  });
});
