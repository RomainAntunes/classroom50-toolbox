import { describe, expect, it } from "vitest";
import en from "./en.json";
import { t } from "./index";

describe("t", () => {
  it("fills in parameters", () => {
    expect(t("editor.open", { editor: "Zed" })).toBe("Open in Zed");
  });

  it("keeps an unknown parameter visible", () => {
    expect(t("editor.open")).toBe("Open in {editor}");
  });

  it("falls back to another key, then to the key itself", () => {
    expect(t("missing.key", {}, "editor.openWith")).toBe("Open with");
    expect(t("missing.key")).toBe("missing.key");
  });
});

describe("en.json", () => {
  it("has no em-dashes", () => {
    for (const [key, value] of Object.entries(en)) {
      expect(value, key).not.toContain("—");
    }
  });
});
