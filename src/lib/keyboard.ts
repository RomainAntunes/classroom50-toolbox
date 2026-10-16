// Keyboard access in the DOM: the project list's arrow keys, and native menus opened from
// the keyboard. The rules themselves are in `keys.ts`.
import { LogicalPosition } from "@tauri-apps/api/dpi";
import type { Menu } from "@tauri-apps/api/menu";
import { withDialog } from "$lib/api";
import { isMenuKey, moveIndex } from "$lib/keys";

/** When the last menu opened from the keyboard; see `onContextMenu`. */
let keyboardMenuAt = -Infinity;

/**
 * Shows a native menu, pinned like a dialog. Under `anchor` when given (a "..." button, or a
 * row opened from the keyboard), else at the pointer (a right click).
 */
export async function popupMenu(menu: Menu, anchor?: Element | null): Promise<void> {
  let at: LogicalPosition | undefined;
  if (anchor) {
    const rect = anchor.getBoundingClientRect();
    at = new LogicalPosition(Math.round(rect.left), Math.round(rect.bottom));
  }
  await withDialog(() => menu.popup(at));
}

/** A keydown handler opening `open` on the Menu key or Shift+F10, under the element. */
export function menuKey(open: (anchor: HTMLElement) => void) {
  return (event: KeyboardEvent) => {
    if (!isMenuKey(event)) return;
    event.preventDefault();
    keyboardMenuAt = performance.now();
    open(event.currentTarget as HTMLElement);
  };
}

/**
 * A contextmenu handler opening `open` at the pointer. Windows and Linux follow the Menu key
 * with a contextmenu event, which `menuKey` already answered.
 */
export function onContextMenu(open: () => void) {
  return (event: MouseEvent) => {
    event.preventDefault();
    if (performance.now() - keyboardMenuAt < 1000) return;
    open();
  };
}

/**
 * Makes the rows under `node` one Tab stop: Up, Down, Home and End move between rows
 * (`[data-row]`, each inside an `[data-item]`), and only the current item's buttons are in
 * the Tab order. Escape inside an item whose details are open closes them.
 */
export function rovingList(node: HTMLElement) {
  let active: HTMLElement | null = null;
  const rows = () => [...node.querySelectorAll<HTMLElement>("[data-row]")];

  function sync() {
    const list = rows();
    if (!active || !list.includes(active)) active = list[0] ?? null;
    for (const item of node.querySelectorAll<HTMLElement>("[data-item]")) {
      const current = active !== null && item.contains(active);
      for (const element of item.querySelectorAll<HTMLElement>("button, a[href], input")) {
        element.tabIndex = current ? 0 : -1;
      }
    }
  }

  function onFocusIn(event: FocusEvent) {
    const row = (event.target as Element | null)
      ?.closest?.("[data-item]")
      ?.querySelector("[data-row]");
    if (row instanceof HTMLElement && row !== active) {
      active = row;
      sync();
    }
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
    const target = event.target as HTMLElement;
    if (event.key === "Escape") {
      const row = target.closest("[data-item]")?.querySelector<HTMLElement>("[data-row]");
      if (row?.getAttribute("aria-expanded") === "true") {
        event.preventDefault();
        row.click();
        row.focus();
      }
      return;
    }
    if (!target.matches("[data-row]")) return;
    const list = rows();
    const next = moveIndex(event.key, list.indexOf(target), list.length);
    if (next === null) return;
    event.preventDefault();
    list[next].focus();
  }

  // Rows come and go with syncs, filters and folded classes.
  const observer = new MutationObserver(sync);
  observer.observe(node, { childList: true, subtree: true });
  node.addEventListener("focusin", onFocusIn);
  node.addEventListener("keydown", onKeyDown);
  sync();
  return {
    destroy() {
      observer.disconnect();
      node.removeEventListener("focusin", onFocusIn);
      node.removeEventListener("keydown", onKeyDown);
    },
  };
}
