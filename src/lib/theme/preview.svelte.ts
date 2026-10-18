import type { Os } from "#lib/api.js";
import { locale } from "#lib/i18n/locale.svelte.js";

// Dev builds only: wear another platform's skin on this machine, to check every theme
// without three computers, and lay the UI out right to left with the current language's
// words. Kept in localStorage so a reload keeps them. Light and dark are a real setting
// (`appearance`), applied by Rust.
const OS_KEY = "dev.previewOs";
const RTL_KEY = "dev.previewRtl";

export const OS_CHOICES = ["auto", "macos", "windows", "linux"] as const;
export type OsChoice = (typeof OS_CHOICES)[number];

export const preview = $state({
  os: "auto" as OsChoice,
  /** The platform the app really runs on, from `app_info`. */
  realOs: null as Os | null,
});

function stored<T extends string>(key: string, choices: readonly T[]): T {
  const value = localStorage.getItem(key);
  return choices.includes(value as T) ? (value as T) : choices[0];
}

function applyOs() {
  const os = preview.os === "auto" ? preview.realOs : preview.os;
  if (os) document.documentElement.dataset.os = os;
}

/** Sets `data-os` from the real platform, or from the dev override. */
export function initTheme(realOs: Os) {
  preview.realOs = realOs;
  if (import.meta.env.DEV) {
    preview.os = stored(OS_KEY, OS_CHOICES);
    locale.rtlPreview = localStorage.getItem(RTL_KEY) === "1";
    // Left by builds that kept the dev appearance here.
    localStorage.removeItem("dev.previewTheme");
  }
  applyOs();
}

export function setPreviewOs(os: OsChoice) {
  preview.os = os;
  localStorage.setItem(OS_KEY, os);
  applyOs();
}

/** Forces `dir="rtl"` (the layout applies it), whatever the language. */
export function setPreviewRtl(on: boolean) {
  locale.rtlPreview = on;
  if (on) localStorage.setItem(RTL_KEY, "1");
  else localStorage.removeItem(RTL_KEY);
}
