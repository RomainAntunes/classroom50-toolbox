import { afterEach, describe, expect, it } from "vitest";
import en from "./en.json";
import { catalogs, formatNumber, languageName, resolveLocale, setLocale, t } from "./index";

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
  it("resolves the system language to the best catalog, else English", () => {
    expect(resolveLocale("system", ["fr-CH", "en"])).toBe("fr");
    expect(resolveLocale("system", ["sv-SE", "FR"])).toBe("fr");
    expect(resolveLocale("system", ["pt-br"])).toBe("pt-BR");
    expect(resolveLocale("system", ["sv", "fi"])).toBe("en");
    expect(resolveLocale("system", [])).toBe("en");
    expect(resolveLocale("fr", ["en"])).toBe("fr");
    expect(resolveLocale("xx", ["fr"])).toBe("en");
  });

  it("tries each OS language by full tag, then by base language", () => {
    const shipped = ["en", "fr", "es", "de"];
    const pick = (...preferred: string[]) => resolveLocale("system", preferred, shipped);
    expect(pick("fr-CH", "en-US")).toBe("fr");
    expect(pick("es-419")).toBe("es");
    expect(pick("de-AT", "fr")).toBe("de");
    expect(pick("zh-Hans-CN", "it-IT", "es-ES")).toBe("es");
    expect(pick("en-GB", "fr-FR")).toBe("en");
    expect(pick("fr_CA.UTF-8")).toBe("fr");
    expect(pick("C", "POSIX")).toBe("en");
    expect(pick("ja-JP")).toBe("en");
    // A regional catalog beats its base language.
    expect(resolveLocale("system", ["pt-BR"], ["en", "pt", "pt-BR"])).toBe("pt-BR");
    expect(resolveLocale("system", ["pt-PT"], ["en", "pt", "pt-BR"])).toBe("pt");
  });

  it("only offers registered catalogs", () => {
    expect(resolveLocale("system", ["pt-PT", "it"], ["en", "fr"])).toBe("en");
    expect(resolveLocale("ja", [], ["en", "fr"])).toBe("en");
    expect(resolveLocale("sv")).toBe("en");
  });

  it("maps regions and scripts to the catalog that serves them", () => {
    const pick = (...preferred: string[]) => resolveLocale("system", preferred);
    expect(pick("pt")).toBe("pt-BR");
    expect(pick("pt-BR")).toBe("pt-BR");
    expect(pick("pt-PT")).toBe("pt-PT");
    expect(pick("pt-AO")).toBe("pt-PT");
    expect(pick("pt_MZ.UTF-8")).toBe("pt-PT");
    expect(pick("zh")).toBe("zh-Hans");
    expect(pick("zh-CN")).toBe("zh-Hans");
    expect(pick("zh-Hans-SG")).toBe("zh-Hans");
    // Traditional Chinese readers get their next language, not Simplified.
    expect(pick("zh-TW", "ja")).toBe("ja");
    expect(pick("zh-Hant-HK")).toBe("en");
    expect(pick("iw-IL")).toBe("he");
    expect(pick("ar-EG")).toBe("ar");
    expect(pick("ko-KR")).toBe("ko");
  });

  it("formats numbers in the UI's language", () => {
    setLocale("fr");
    expect(formatNumber(1.54, 1)).toBe("1,5");
    setLocale("en");
    expect(formatNumber(2, 1)).toBe("2.0");
  });

  it("names each language in itself", () => {
    expect(languageName("en")).toBe("English");
    expect(languageName("fr")).toBe("Français");
    expect(languageName("es")).toBe("Español");
    expect(languageName("de")).toBe("Deutsch");
    for (const code of Object.keys(catalogs)) {
      expect(languageName(code), code).not.toBe(code);
    }
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
