// Minimal i18n: flat JSON catalogs keyed by dotted ids, `{name}` placeholders.
// Every user-facing string goes through `t`, even while English is the only locale.
import en from "./en.json";

type Catalog = Record<string, string>;

const catalogs: Record<string, Catalog> = { en };
const fallback = "en";

let current: Catalog = en;

export function setLocale(locale: string): void {
  const base = locale.toLowerCase().split("-")[0];
  current = catalogs[base] ?? catalogs[fallback];
}

export function t(key: string, params: Record<string, string | number> = {}): string {
  const template = current[key] ?? catalogs[fallback][key] ?? key;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

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
