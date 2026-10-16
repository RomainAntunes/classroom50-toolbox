// Minimal i18n: flat JSON catalogs keyed by dotted ids, `{name}` placeholders.
// Every user-facing string goes through `t`.
// A string with a count is stored per CLDR plural category (`foo.one`, `foo.other`, plus
// `few`/`many`/... where the language has them) and called as `t("foo", { count })`.
import en from "./en.json";
import fr from "./fr.json";
import es from "./es.json";
import de from "./de.json";
import { locale } from "./locale.svelte";

type Catalog = Record<string, string>;

/** Every catalog, by BCP 47 code. Adding a language: import its JSON and list it here. */
export const catalogs: Record<string, Catalog> = { en, fr, es, de };
const fallback = "en";

/** The language each catalog is written in, named in itself ("English", "Français"). */
export function languageName(code: string): string {
  return catalogs[code]?.["language.name"] ?? code;
}

/**
 * The catalog for a `language` setting: a code, or "system" for the best match among
 * `preferred` (the OS's languages, most wanted first), else English. Each language is
 * tried by its full tag, then its base language (`fr-CH` -> `fr`, `es-419` -> `es`); the
 * first one with a catalog wins.
 */
export function resolveLocale(
  language: string,
  preferred: readonly string[] = [],
  available: readonly string[] = Object.keys(catalogs),
): string {
  const wanted = language === "system" ? preferred : [language];
  const find = (tag: string) => available.find((code) => code.toLowerCase() === tag);
  for (const raw of wanted) {
    // POSIX spellings too: `fr_CH.UTF-8@euro`.
    const tag = raw.split(/[.@]/)[0].replace(/_/g, "-").toLowerCase();
    const match = find(tag) ?? find(tag.split("-")[0]);
    if (match) return match;
  }
  return fallback;
}

// Plural rules per catalog, built on first use.
const plurals = new Map<string, Intl.PluralRules>();
function pluralRules(code: string): Intl.PluralRules {
  let rules = plurals.get(code);
  if (!rules) plurals.set(code, (rules = new Intl.PluralRules(code)));
  return rules;
}

/** Switches the catalog (`language` as in `resolveLocale`); the UI follows at once. */
export function setLocale(language: string, preferred: readonly string[] = []): void {
  locale.code = resolveLocale(language, preferred);
}

/** The active catalog's code, e.g. for `<html lang>`. */
export function currentLocale(): string {
  return locale.code;
}

/** `key` in one catalog: its plural form for `count` first, then `.other`, then `key`. */
function lookup(code: string, key: string, count: number | undefined): string | undefined {
  const catalog = catalogs[code];
  if (count !== undefined) {
    const form = catalog[`${key}.${pluralRules(code).select(count)}`] ?? catalog[`${key}.other`];
    if (form !== undefined) return form;
  }
  return catalog[key];
}

/** Whether `key` is worded (in English, which every catalog matches), plural or not. */
export function hasKey(key: string): boolean {
  const reference = catalogs[fallback];
  return key in reference || `${key}.other` in reference;
}

/**
 * `params.count`, when a number, picks the plural form. `fallbackKey` is used when `key` is
 * missing, e.g. for keys built from an error kind. A key the active catalog lacks comes
 * from English.
 */
export function t(
  key: string,
  params: Record<string, string | number> = {},
  fallbackKey?: string,
): string {
  const code = locale.code;
  const count = typeof params.count === "number" ? params.count : undefined;
  const find = (k: string) => lookup(code, k, count) ?? lookup(fallback, k, count);
  const template = find(key) ?? (fallbackKey && find(fallbackKey)) ?? key;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

/** "Ada, Bob and Carol" in the catalog's language, so a sentence never mixes two. */
export function formatList(items: string[]): string {
  return new Intl.ListFormat(locale.code, { type: "conjunction" }).format(items);
}

/** "Fri 17 Oct, 23:59" in the catalog's language: for exact dates in details. */
/** A number in the UI's language, with exactly `digits` decimals: 1.5 -> "1,5" in French. */
export function formatNumber(value: number, digits = 0): string {
  return new Intl.NumberFormat(locale.code, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}

/** 12400000 -> "12.4 MB" in the UI's language: download sizes. */
export function formatMegabytes(bytes: number): string {
  return new Intl.NumberFormat(locale.code, {
    style: "unit",
    unit: "megabyte",
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  }).format(bytes / 1_000_000);
}

export function formatDate(date: Date): string {
  return new Intl.DateTimeFormat(locale.code, {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

/** "16 October 2026" in the catalog's language: a day, with no time. */
export function formatDay(date: Date): string {
  return new Intl.DateTimeFormat(locale.code, { dateStyle: "long" }).format(date);
}

// Dates and numbers follow the catalog's language, not the OS's, so a French system
// doesn't get "Due il y a 6 jours" while the app is in English.
const relatives = new Map<string, Intl.RelativeTimeFormat>();
function relative(): Intl.RelativeTimeFormat {
  let format = relatives.get(locale.code);
  if (!format) {
    format = new Intl.RelativeTimeFormat(locale.code, { numeric: "auto" });
    relatives.set(locale.code, format);
  }
  return format;
}

/** "in 3 hours", "yesterday": for due dates. */
export function fromNow(date: Date, now = new Date()): string {
  const seconds = Math.round((date.getTime() - now.getTime()) / 1000);
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [unit, size] of units) {
    if (Math.abs(seconds) >= size) return relative().format(Math.round(seconds / size), unit);
  }
  return relative().format(seconds, "second");
}

/** 7200 -> "2 hours", 90000 -> "1 day": the largest whole unit, for lateness. */
export function durationLabel(seconds: number): string {
  const units: [Intl.NumberFormatOptions["unit"], number][] = [
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  const [unit, size] = units.find(([, size]) => seconds >= size) ?? ["minute", 60];
  return new Intl.NumberFormat(locale.code, { style: "unit", unit, unitDisplay: "long" }).format(
    Math.max(1, Math.floor(seconds / size)),
  );
}

/** 24 -> "1 day", 6 -> "6 hours", in the user's locale. */
export function hoursLabel(hours: number): string {
  const [value, unit] = hours % 24 === 0 ? [hours / 24, "day"] : [hours, "hour"];
  return new Intl.NumberFormat(locale.code, { style: "unit", unit, unitDisplay: "long" }).format(
    value,
  );
}
