import { afterEach, describe, expect, it } from "vitest";
import en from "./en.json";
import { catalogs, languageName, resolveLocale, setLocale, t } from "./index";

afterEach(() => setLocale("en"));

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

describe("plurals", () => {
  it("picks English's one and other forms", () => {
    expect(t("projects.gradeTests", { passed: 1, count: 1 })).toBe("1 of 1 test passed");
    expect(t("projects.gradeTests", { passed: 0, count: 3 })).toBe("0 of 3 tests passed");
    expect(t("projects.gradeTests", { passed: 0, count: 0 })).toBe("0 of 0 tests passed");
  });

  it("uses a plain key when a count has no plural forms", () => {
    expect(t("projects.comments", { count: 1 })).toBe("Feedback (1)");
  });

  describe("in a language with few and many", () => {
    // Polish: 1 is `one`, 2-4 `few`, 5-21 `many`, fractions `other`.
    const synthetic = {
      ...en,
      "language.name": "Polski",
      "test.files.one": "{count} plik",
      "test.files.few": "{count} pliki",
      "test.files.many": "{count} plików",
      "test.files.other": "{count} pliku",
      "test.partial.one": "jeden",
      "test.partial.other": "inne {count}",
      "editor.open": "Otwórz w {editor}",
    } as Record<string, string>;
    delete synthetic["editor.openWith"];

    afterEach(() => {
      delete catalogs.pl;
    });

    it("picks the locale's category", () => {
      catalogs.pl = synthetic;
      setLocale("pl");
      expect(t("test.files", { count: 1 })).toBe("1 plik");
      expect(t("test.files", { count: 3 })).toBe("3 pliki");
      expect(t("test.files", { count: 5 })).toBe("5 plików");
      expect(t("test.files", { count: 1.5 })).toBe("1.5 pliku");
    });

    it("falls back to other, then to English", () => {
      catalogs.pl = synthetic;
      setLocale("pl");
      expect(t("test.partial", { count: 3 })).toBe("inne 3");
      expect(t("editor.open", { editor: "Zed" })).toBe("Otwórz w Zed");
      expect(t("editor.openWith")).toBe("Open with");
      expect(t("missing.key", {}, "editor.openWith")).toBe("Open with");
    });
  });
});

describe("locales", () => {
  afterEach(() => {
    delete catalogs.fr;
    delete catalogs["pt-BR"];
  });

  it("resolves the system language to the best catalog, else English", () => {
    catalogs.fr = { "language.name": "Français" };
    catalogs["pt-BR"] = { "language.name": "Português (Brasil)" };
    expect(resolveLocale("system", ["fr-CH", "en"])).toBe("fr");
    expect(resolveLocale("system", ["de-DE", "FR"])).toBe("fr");
    expect(resolveLocale("system", ["pt-br"])).toBe("pt-BR");
    expect(resolveLocale("system", ["de", "ja"])).toBe("en");
    expect(resolveLocale("system", [])).toBe("en");
    expect(resolveLocale("fr", ["en"])).toBe("fr");
    expect(resolveLocale("xx", ["fr"])).toBe("en");
  });

  it("names each language in itself", () => {
    catalogs.fr = { "language.name": "Français" };
    expect(languageName("en")).toBe("English");
    expect(languageName("fr")).toBe("Français");
  });
});

const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
const CATEGORIES = ["zero", "one", "two", "few", "many", "other"];

/** A catalog's keys with plural forms folded into their base key. */
function groups(catalog: Record<string, string>): Map<string, Map<string, string>> {
  const result = new Map<string, Map<string, string>>();
  for (const [key, value] of Object.entries(catalog)) {
    const dot = key.lastIndexOf(".");
    const category = key.slice(dot + 1);
    const isForm = CATEGORIES.includes(category);
    const base = isForm ? key.slice(0, dot) : key;
    if (!result.has(base)) result.set(base, new Map());
    result.get(base)!.set(isForm ? category : "", value);
  }
  return result;
}

/** Every category Intl.PluralRules can produce for a locale, integers and fractions. */
function categoriesOf(code: string): string[] {
  return new Intl.PluralRules(code).resolvedOptions().pluralCategories as string[];
}

describe("catalogs", () => {
  const reference = groups(en);

  for (const [code, catalog] of Object.entries(catalogs)) {
    describe(code, () => {
      const own = groups(catalog);

      it("has exactly English's keys", () => {
        expect([...own.keys()].sort()).toEqual([...reference.keys()].sort());
      });

      it("has every plural form its language uses, and other", () => {
        const needed = categoriesOf(code);
        for (const [base, forms] of own) {
          if (forms.has("")) {
            expect(forms.size, `${base} mixes a plain string and plural forms`).toBe(1);
            expect(reference.get(base)?.has(""), `${base} has plural forms in English`).toBe(true);
            continue;
          }
          expect(reference.get(base)?.has(""), `${base} is plain in English`).not.toBe(true);
          expect(forms.has("other"), `${base}.other`).toBe(true);
          for (const category of needed) {
            expect(forms.has(category), `${base}.${category}`).toBe(true);
          }
        }
      });

      it("uses the same placeholders per key", () => {
        for (const [base, forms] of own) {
          const expected = placeholders(
            reference.get(base)?.get("") ?? reference.get(base)?.get("other") ?? "",
          );
          for (const [category, value] of forms) {
            // A `one` form may spell the count out ("a comment"); the others name it.
            const found = placeholders(value);
            const wanted =
              category && category !== "other" ? expected.filter((p) => p !== "count") : expected;
            expect(
              found.filter((p) => category === "" || category === "other" || p !== "count"),
              `${base}.${category}`,
            ).toEqual(wanted);
          }
        }
      });

      it("has no em-dashes", () => {
        for (const [key, value] of Object.entries(catalog)) {
          expect(value, key).not.toContain("—");
        }
      });
    });
  }
});
