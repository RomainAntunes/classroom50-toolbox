import { getCurrentWindow, type Theme } from "@tauri-apps/api/window";
import type { Os } from "$lib/api";

// Dev builds only: wear another platform's skin or appearance on this machine, to check
// every theme without three computers. Kept in localStorage so a reload keeps it.
const OS_KEY = "dev.previewOs";
const THEME_KEY = "dev.previewTheme";

export const OS_CHOICES = ["auto", "macos", "windows", "linux"] as const;
export const THEME_CHOICES = ["auto", "light", "dark"] as const;
export type OsChoice = (typeof OS_CHOICES)[number];
export type ThemeChoice = (typeof THEME_CHOICES)[number];

export const preview = $state({
  os: "auto" as OsChoice,
  theme: "auto" as ThemeChoice,
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

function applyTheme() {
  const theme: Theme | null = preview.theme === "auto" ? null : preview.theme;
  void getCurrentWindow().setTheme(theme);
}

/** Sets `data-os` from the real platform, or from the dev override. */
export function initTheme(realOs: Os) {
  preview.realOs = realOs;
  if (import.meta.env.DEV) {
    preview.os = stored(OS_KEY, OS_CHOICES);
    preview.theme = stored(THEME_KEY, THEME_CHOICES);
    if (preview.theme !== "auto") applyTheme();
  }
  applyOs();
}

export function setPreviewOs(os: OsChoice) {
  preview.os = os;
  localStorage.setItem(OS_KEY, os);
  applyOs();
}

export function setPreviewTheme(theme: ThemeChoice) {
  preview.theme = theme;
  localStorage.setItem(THEME_KEY, theme);
  applyTheme();
}
