// Minimal i18n: flat JSON catalogs keyed by dotted ids, `{name}` placeholders.
// Every user-facing string goes through `t`.
// A string with a count is stored per CLDR plural category (`foo.one`, `foo.other`, plus
// `few`/`many`/... where the language has them) and called as `t("foo", { count })`.
import en from "./en.json";
import fr from "./fr.json";
import es from "./es.json";
import de from "./de.json";
import it from "./it.json";
import ptBR from "./pt-BR.json";
import ptPT from "./pt-PT.json";
import nl from "./nl.json";
import pl from "./pl.json";
import tr from "./tr.json";
import ar from "./ar.json";
import he from "./he.json";
import zhHans from "./zh-Hans.json";
import ja from "./ja.json";
import ko from "./ko.json";
import { isolate, textDirection, type Direction } from "./direction";
import { locale } from "./locale.svelte";

export { isolate, textDirection, type Direction } from "./direction";

type Catalog = Record<string, string>;

/**
 * Every catalog, by BCP 47 code, in the order the language menu lists them. Adding a
 * language: import its JSON and list it here.
 */
export const catalogs: Record<string, Catalog> = {
  en,
  fr,
  es,
  de,
  it,
  "pt-BR": ptBR,
  "pt-PT": ptPT,
  nl,
  pl,
  tr,
  ar,
  he,
  "zh-Hans": zhHans,
  ja,
  ko,
};
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
    for (const candidate of [tag, tag.split("-")[0], ...aliases(tag)]) {
      const match = find(candidate);
      if (match) return match;
    }
  }
  return fallback;
}

// Portuguese outside Brazil follows Portugal's norm (Africa, Timor-Leste, Macau).
const EUROPEAN_PORTUGUESE = new Set(["ao", "mz", "cv", "gw", "st", "tl", "mo", "gq", "lu", "ch"]);
// Chinese written in Traditional characters, which the Simplified catalog doesn't serve.
const TRADITIONAL_CHINESE = new Set(["tw", "hk", "mo"]);

/**
 * Catalogs a lowercased tag stands for when neither its exact name nor its base language has one: a Portuguese region to
 * `pt-PT` or `pt-BR`, Simplified Chinese (`zh`, `zh-CN`, `zh-Hans-SG`) to `zh-Hans` but not
 * Traditional (`zh-TW`, `zh-Hant`, which go on to the next preferred language), and Hebrew's
 * old code `iw` to `he`.
 */
function aliases(tag: string): string[] {
  const [language, ...rest] = tag.split("-");
  const script = rest.find((part) => part.length === 4);
  const region = rest.find((part) => part.length === 2 || /^\d{3}$/.test(part));
  if (language === "pt") return [EUROPEAN_PORTUGUESE.has(region ?? "") ? "pt-pt" : "pt-br"];
  if (language === "zh") {
    const traditional = script === "hant" || (!script && TRADITIONAL_CHINESE.has(region ?? ""));
    return traditional ? [] : ["zh-hans"];
  }
  if (language === "iw") return ["he"];
  return [];
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

/** Which way the UI reads, for `<html dir>`: the catalog's, or `rtl` in the dev preview. */
export function currentDirection(): Direction {
  return locale.rtlPreview ? "rtl" : textDirection(locale.code);
}

// A bare number (a count, a score, `12.5%`): left alone, so `{score}/{max}` stays one
// European-number run that reads "8/10" right to left; isolating each half made it "10/8".
const NUMBER = /^[+-]?\d+(?:[.,]\d+)*%?$/;

/**
 * `template` with each `{name}` replaced by `params.name`; an unknown one stays visible.
 * In a right-to-left UI each text value is isolated (`isolate`), since it is usually data
 * (a login, repo, path, branch or URL) that the sentence around it would otherwise
 * reorder; a bare number isn't (`NUMBER`). A value holding more than a number (`"7/10"`)
 * is isolated whole, as one left-to-right unit.
 */
export function fill(
  template: string,
  params: Record<string, string | number>,
  direction: Direction = "ltr",
): string {
  return template.replace(/\{(\w+)\}/g, (match, name: string) => {
    if (!(name in params)) return match;
    const value = String(params[name]);
    return direction === "rtl" && !NUMBER.test(value) ? isolate(value) : value;
  });
}

/**
 * A value inside text built in code (`${author}, ${when}`): isolated in a right-to-left UI,
 * as `t` does with its parameters, else as it is.
 */
export function isolated(value: string): string {
  return currentDirection() === "rtl" ? isolate(value) : value;
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
  const text = fill(template, params, currentDirection());
  // The dev preview lays a left-to-right language out right to left: each sentence is one
  // island that reads in order, as a right-to-left catalog's own sentences would.
  return locale.rtlPreview && textDirection(code) === "ltr" ? isolate(text) : text;
}

/** "Ada, Bob and Carol" in the catalog's language, so a sentence never mixes two. Each
 *  item (a name, a login) is `isolated`. */
export function formatList(items: string[]): string {
  return new Intl.ListFormat(locale.code, { type: "conjunction" }).format(items.map(isolated));
}

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

/** "Fri 17 Oct, 23:59" in the catalog's language: for exact dates in details. */
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

/** Calendar days from `now`'s date to `date`'s, in local time: 1 is tomorrow at any hour.
 *  Each date is read as a UTC midnight, so a day of 23 or 25 hours (a DST change) counts
 *  as one. */
export function calendarDays(date: Date, now: Date): number {
  const day = (d: Date) => Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());
  return Math.round((day(date) - day(now)) / 86_400_000);
}

/** "in 3 hours", "yesterday", "in 4 days". From a day apart it counts calendar days, so
 *  "tomorrow" is always the next date, never 38 hours rounded to "the day after
 *  tomorrow". */
export function fromNow(date: Date, now = new Date()): string {
  const seconds = Math.round((date.getTime() - now.getTime()) / 1000);
  const days = calendarDays(date, now);
  // A 25-hour day (clocks going back) can hold a whole day on one date: hours then.
  if (Math.abs(seconds) >= 86400 && days !== 0) return relative().format(days, "day");
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [unit, size] of units) {
    if (Math.abs(seconds) >= size) return relative().format(Math.round(seconds / size), unit);
  }
  return relative().format(seconds, "second");
}

/** "10:30 PM", "22:30": the hour of a date in the catalog's language. */
export function formatTime(date: Date): string {
  return new Intl.DateTimeFormat(locale.code, { hour: "numeric", minute: "2-digit" }).format(date);
}

/**
 * When a deadline (or an opening) falls, for a sentence like "Due {when}": "tomorrow at
 * 10:30 PM" from the day before yesterday to the day after tomorrow, the day counted in
 * calendar dates; "in 40 minutes" within the hour, where the countdown says more than the
 * clock; "in 5 days" further out. Without `countdown` the hour keeps "today at 10:30 PM",
 * so the words only change at midnight (the tray menu, which nothing redraws each minute).
 */
export function relativeAt(date: Date, now = new Date(), countdown = true): string {
  const days = calendarDays(date, now);
  const soon = countdown && Math.abs(date.getTime() - now.getTime()) < 3_600_000;
  if (soon || Math.abs(days) > 2) {
    return fromNow(date, now);
  }
  return t("time.dayAt", { day: relative().format(days, "day"), time: formatTime(date) });
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

/** 10 -> "10 seconds", 120 -> "2 minutes", 90 -> "90 seconds": exact, for a time limit. */
export function secondsLabel(seconds: number): string {
  const units: [Intl.NumberFormatOptions["unit"], number][] = [
    ["hour", 3600],
    ["minute", 60],
  ];
  const [unit, size] = units.find(([, size]) => seconds >= size && seconds % size === 0) ?? [
    "second",
    1,
  ];
  return new Intl.NumberFormat(locale.code, { style: "unit", unit, unitDisplay: "long" }).format(
    seconds / size,
  );
}

/** 24 -> "1 day", 6 -> "6 hours", in the user's locale. */
export function hoursLabel(hours: number): string {
  const [value, unit] = hours % 24 === 0 ? [hours / 24, "day"] : [hours, "hour"];
  return new Intl.NumberFormat(locale.code, { style: "unit", unit, unitDisplay: "long" }).format(
    value,
  );
}
