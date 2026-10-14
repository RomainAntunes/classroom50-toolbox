// Minimal i18n: flat JSON catalogs keyed by dotted ids, `{name}` placeholders.
// Every user-facing string goes through `t`, even while English is the only locale.
import en from "./en.json";

type Catalog = Record<string, string>;

const catalogs: Record<string, Catalog> = { en };
const fallback = "en";

let currentLocale = fallback;
let current: Catalog = en;
// Dates and numbers follow the catalog's language, not the OS's, so a French system
// doesn't get "Due il y a 6 jours" while the app is in English.
let relative = new Intl.RelativeTimeFormat(currentLocale, { numeric: "auto" });

export function setLocale(locale: string): void {
  const base = locale.toLowerCase().split("-")[0];
  currentLocale = base in catalogs ? base : fallback;
  current = catalogs[currentLocale];
  relative = new Intl.RelativeTimeFormat(currentLocale, { numeric: "auto" });
}

/** `fallbackKey` is used when `key` is missing, e.g. for keys built from an error kind. */
export function t(
  key: string,
  params: Record<string, string | number> = {},
  fallbackKey?: string,
): string {
  const lookup = (k: string) => current[k] ?? catalogs[fallback][k];
  const template = lookup(key) ?? (fallbackKey && lookup(fallbackKey)) ?? key;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

/** "Fri 17 Oct, 23:59" in the catalog's language: for exact dates in details. */
export function formatDate(date: Date): string {
  return new Intl.DateTimeFormat(currentLocale, {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
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
    if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
  }
  return relative.format(seconds, "second");
}

/** 7200 -> "2 hours", 90000 -> "1 day": the largest whole unit, for lateness. */
export function durationLabel(seconds: number): string {
  const units: [Intl.NumberFormatOptions["unit"], number][] = [
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  const [unit, size] = units.find(([, size]) => seconds >= size) ?? ["minute", 60];
  return new Intl.NumberFormat(currentLocale, { style: "unit", unit, unitDisplay: "long" }).format(
    Math.max(1, Math.floor(seconds / size)),
  );
}

/** 24 -> "1 day", 6 -> "6 hours", in the user's locale. */
export function hoursLabel(hours: number): string {
  const [value, unit] = hours % 24 === 0 ? [hours / 24, "day"] : [hours, "hour"];
  return new Intl.NumberFormat(currentLocale, { style: "unit", unit, unitDisplay: "long" }).format(
    value,
  );
}
