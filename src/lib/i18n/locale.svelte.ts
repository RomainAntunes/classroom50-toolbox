// The active catalog's code as reactive state: `t` reads it, so every template and
// `$derived` that calls `t` follows a language change without a restart. `rtlPreview`
// (dev builds, Settings > Developer) lays the UI out right to left whatever the language.
export const locale = $state({ code: "en", rtlPreview: false });
