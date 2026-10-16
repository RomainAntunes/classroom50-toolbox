// The system-wide shortcut's recorder and how a shortcut reads, pure. Rust stores and
// registers it (`shortcut.rs`, whose `normalize` is the authority); the forms here match.
import type { Os } from "$lib/api";

export type Modifier = "Control" | "Alt" | "Shift" | "Super";

/** The order an accelerator stores its modifiers in, and macOS's ⌃⌥⇧⌘. */
const MODIFIERS: readonly Modifier[] = ["Control", "Alt", "Shift", "Super"];

const LETTERS = [..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"].map((c) => `Key${c}`);
const DIGITS = [..."0123456789"].map((c) => `Digit${c}`);
const FUNCTION_KEYS = Array.from({ length: 24 }, (_, i) => `F${i + 1}`);

/** Words for a key, per catalog: `shortcut.key.<name>`. */
const NAMED: Record<string, string> = {
  Space: "space",
  Enter: "enter",
  Backspace: "backspace",
  Delete: "delete",
  Insert: "insert",
  Home: "home",
  End: "end",
  PageUp: "pageUp",
  PageDown: "pageDown",
};

const PUNCTUATION: Record<string, string> = {
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

const ARROWS: Record<string, string> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

/** What macOS's menus show for these keys. */
const MAC_SYMBOLS: Record<string, string> = {
  Enter: "↩",
  Backspace: "⌫",
  Delete: "⌦",
  Home: "↖",
  End: "↘",
  PageUp: "⇞",
  PageDown: "⇟",
};

/**
 * The keys a shortcut can end with (`KeyboardEvent.code` names), as Rust's `KEYS`: no
 * Escape (it cancels the recorder), no Tab, lock or media keys (the OS keeps them).
 */
export const KEYS: ReadonlySet<string> = new Set([
  ...LETTERS,
  ...DIGITS,
  ...FUNCTION_KEYS,
  ...Object.keys(NAMED),
  ...Object.keys(ARROWS),
  ...Object.keys(PUNCTUATION),
]);

/** The keys that are modifiers themselves: held, not recorded. */
const MODIFIER_CODES = /^(Shift|Control|Alt|Meta|OS|AltGraph)(Left|Right)?$/;

export interface KeyPress {
  code: string;
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type Recorded =
  /** A shortcut to save. */
  | { kind: "shortcut"; accelerator: string }
  /** Only modifiers so far: shown while the user goes on. */
  | { kind: "modifiers"; held: Modifier[] }
  /** A key with no modifier but Shift. */
  | { kind: "needsModifier"; held: Modifier[] }
  /** A key a shortcut can't use. */
  | { kind: "unsupported"; held: Modifier[] };

/** The modifiers a key event holds, in the stored order. */
export function heldModifiers(press: KeyPress): Modifier[] {
  const on: Record<Modifier, boolean> = {
    Control: press.ctrlKey,
    Alt: press.altKey,
    Shift: press.shiftKey,
    Super: press.metaKey,
  };
  return MODIFIERS.filter((m) => on[m]);
}

/**
 * The key to store for a press. macOS registers the physical key (`code`). Windows and X11
 * register the key the layout types, so a letter comes from `key` there: the A of an
 * AZERTY keyboard is `KeyQ` by position but registers as A.
 */
function keyCode(press: KeyPress, os: Os): string | null {
  if (os !== "macos" && /^[a-z]$/i.test(press.key)) return `Key${press.key.toUpperCase()}`;
  return KEYS.has(press.code) ? press.code : null;
}

/** What a key press in the recorder means. */
export function recordKey(press: KeyPress, os: Os): Recorded {
  const modifiers = heldModifiers(press);
  if (MODIFIER_CODES.test(press.code)) return { kind: "modifiers", held: modifiers };
  const code = keyCode(press, os);
  if (!code) return { kind: "unsupported", held: modifiers };
  if (!modifiers.some((m) => m !== "Shift")) return { kind: "needsModifier", held: modifiers };
  return { kind: "shortcut", accelerator: [...modifiers, code].join("+") };
}

/** The modifiers and key of a stored accelerator, or `null` when it isn't one. */
export function parseShortcut(accelerator: string): { modifiers: Modifier[]; key: string } | null {
  const parts = accelerator.split("+");
  const key = parts.pop();
  if (!key || !KEYS.has(key)) return null;
  if (!parts.every((p): p is Modifier => (MODIFIERS as readonly string[]).includes(p))) {
    return null;
  }
  if (new Set(parts).size !== parts.length) return null;
  if (!parts.some((m) => m !== "Shift")) return null;
  return { modifiers: MODIFIERS.filter((m) => parts.includes(m)), key };
}

function keyLabel(code: string, os: Os, word: (name: string) => string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (os === "macos" && MAC_SYMBOLS[code]) return MAC_SYMBOLS[code];
  if (NAMED[code]) return word(NAMED[code]);
  return ARROWS[code] ?? PUNCTUATION[code] ?? code;
}

const MAC_MODIFIERS: Record<Modifier, string> = {
  Control: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  Super: "⌘",
};

/** Catalog names (`shortcut.key.<name>`) of the modifiers on Windows and Linux. */
function modifierWord(modifier: Modifier, os: Os): string {
  if (modifier === "Super") return os === "windows" ? "win" : "super";
  return modifier === "Control" ? "ctrl" : modifier.toLowerCase();
}

/**
 * How the platform writes a shortcut: `⌥⌘C` on macOS, `Ctrl+Alt+C` elsewhere. Modifiers
 * alone (while recording) read the same way, without a key. `word` gives a key's name in
 * the UI's language (`t("shortcut.key." + name)`).
 */
export function formatShortcut(
  modifiers: readonly Modifier[],
  key: string | null,
  os: Os,
  word: (name: string) => string,
): string {
  const ordered = MODIFIERS.filter((m) => modifiers.includes(m));
  const label = key ? keyLabel(key, os, word) : null;
  if (os === "macos") return ordered.map((m) => MAC_MODIFIERS[m]).join("") + (label ?? "");
  const parts = ordered.map((m) => word(modifierWord(m, os)));
  if (label !== null) parts.push(label);
  return parts.join("+");
}

/** A stored accelerator as the platform writes it; `null` when it can't be read. */
export function formatAccelerator(
  accelerator: string,
  os: Os,
  word: (name: string) => string,
): string | null {
  const parsed = parseShortcut(accelerator);
  return parsed && formatShortcut(parsed.modifiers, parsed.key, os, word);
}
