import { afterEach, describe, expect, it } from "vitest";
import en from "./en.json";
import { isolate, textDirection } from "./direction";
import { catalogs, currentDirection, fill, formatList, isolated, setLocale, t } from "./index";
import { locale } from "./locale.svelte";

const FSI = "\u2068";
const PDI = "\u2069";

describe("textDirection", () => {
  it("reads Arabic, Hebrew, Persian and Urdu right to left, with any region", () => {
    for (const code of ["ar", "he", "fa", "ur", "ar-EG", "he-IL", "fa_IR.UTF-8", "UR-pk", "iw"]) {
      expect(textDirection(code), code).toBe("rtl");
    }
  });

  it("reads everything else left to right", () => {
    for (const code of ["en", "fr-CH", "es-419", "de", "", "system", "arn", "her"]) {
      expect(textDirection(code), code).toBe("ltr");
    }
  });

  it("follows a script subtag over the language", () => {
    expect(textDirection("az-Arab")).toBe("rtl");
    expect(textDirection("pa-Arab-PK")).toBe("rtl");
    expect(textDirection("sd-Deva")).toBe("ltr");
    expect(textDirection("ar-Latn")).toBe("ltr");
  });
});

describe("isolate", () => {
  it("wraps a value in first strong and pop directional isolates", () => {
    expect(isolate("~/code/cs50")).toBe(`${FSI}~/code/cs50${PDI}`);
    expect(isolate("")).toBe(`${FSI}${PDI}`);
  });
});

describe("fill", () => {
  it("leaves values as they are left to right", () => {
    expect(fill("Open in {editor}", { editor: "Zed" })).toBe("Open in Zed");
    expect(fill("{n} of {count}", { n: 1, count: 2 }, "ltr")).toBe("1 of 2");
  });

  it("isolates each value right to left, and keeps unknown placeholders visible", () => {
    expect(fill("افتح في {editor} {missing}", { editor: "VS Code" }, "rtl")).toBe(
      `افتح في ${FSI}VS Code${PDI} {missing}`,
    );
    expect(fill("{a}/{b}", { a: 3, b: 4 }, "rtl")).toBe(`${FSI}3${PDI}/${FSI}4${PDI}`);
  });

  it("lets a placeholder pass through for Rust to fill, isolated when right to left", () => {
    expect(fill("Class: {name}", { name: "{name}" }, "ltr")).toBe("Class: {name}");
    expect(fill("Class: {name}", { name: "{name}" }, "rtl")).toBe(`Class: ${FSI}{name}${PDI}`);
  });
});

describe("t in a right-to-left UI", () => {
  afterEach(() => {
    delete catalogs.ar;
    locale.rtlPreview = false;
    setLocale("en");
  });

  it("isolates parameters in a right-to-left catalog only", () => {
    catalogs.ar = { ...en, "editor.open": "افتح في {editor}" };
    expect(currentDirection()).toBe("ltr");
    expect(t("editor.open", { editor: "Zed" })).toBe("Open in Zed");
    setLocale("ar");
    expect(currentDirection()).toBe("rtl");
    expect(t("editor.open", { editor: "Zed" })).toBe(`افتح في ${FSI}Zed${PDI}`);
  });

  it("isolates them in the dev preview too, and each left-to-right sentence as a whole", () => {
    locale.rtlPreview = true;
    expect(currentDirection()).toBe("rtl");
    expect(t("editor.open", { editor: "Zed" })).toBe(`${FSI}Open in ${FSI}Zed${PDI}${PDI}`);
    catalogs.ar = { ...en, "editor.open": "افتح في {editor}" };
    setLocale("ar");
    expect(t("editor.open", { editor: "Zed" })).toBe(`افتح في ${FSI}Zed${PDI}`);
  });

  it("isolates values built into text, and list items, right to left only", () => {
    expect(isolated("alice")).toBe("alice");
    expect(formatList(["Ada", "Bob"])).toBe("Ada and Bob");
    locale.rtlPreview = true;
    expect(isolated("alice")).toBe(`${FSI}alice${PDI}`);
    expect(formatList(["Ada", "Bob"])).toBe(`${FSI}Ada${PDI} and ${FSI}Bob${PDI}`);
  });
});
