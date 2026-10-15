// The active catalog's code as reactive state: `t` reads it, so every template and
// `$derived` that calls `t` follows a language change without a restart.
export const locale = $state({ code: "en" });
