import { afterEach, describe, expect, it } from "vitest";
import { textDirection } from "./direction";
import { currentDirection, setLocale } from "./index";
import { locale } from "./locale.svelte";

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

describe("currentDirection", () => {
  afterEach(() => {
    locale.rtlPreview = false;
    setLocale("en");
  });

  it("follows the catalog, or the dev preview", () => {
    expect(currentDirection()).toBe("ltr");
    locale.rtlPreview = true;
    expect(currentDirection()).toBe("rtl");
  });
});
