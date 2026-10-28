import type { HourCycle } from "#lib/api.js";

// The active catalog's code as reactive state: `t` reads it, so every template and
// `$derived` that calls `t` follows a language change without a restart. `rtlPreview`
// (dev builds, Settings > Developer) lays the UI out right to left whatever the language.
// `hourCycle` is the OS's 12- or 24-hour clock, which times follow whatever the language
// (`null` until Rust says, or when it can't: the language's own clock).
export const locale = $state({
  code: "en",
  rtlPreview: false,
  hourCycle: null as HourCycle | null,
});
