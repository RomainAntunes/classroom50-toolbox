// Pure keyboard rules, kept apart from the DOM so they can be unit-tested.

export interface MoveOptions {
  /** Left and Right move too (a row of radio buttons). */
  horizontal?: boolean;
  /** Past the last item comes the first, and the other way round. */
  wrap?: boolean;
  /** Laid out right to left: Left moves to the next item, Right to the previous one. */
  rtl?: boolean;
}

/**
 * Where a key moves the focus among `count` items from `current` (-1 when none has it):
 * Up and Down by one, Home and End to the ends. Null when the key doesn't move it.
 */
export function moveIndex(
  key: string,
  current: number,
  count: number,
  { horizontal = false, wrap = false, rtl = false }: MoveOptions = {},
): number | null {
  if (count <= 0) return null;
  const last = count - 1;
  let step = 0;
  switch (key) {
    case "Home":
      return 0;
    case "End":
      return last;
    case "ArrowDown":
      step = 1;
      break;
    case "ArrowUp":
      step = -1;
      break;
    case "ArrowRight":
      step = horizontal ? (rtl ? -1 : 1) : 0;
      break;
    case "ArrowLeft":
      step = horizontal ? (rtl ? 1 : -1) : 0;
      break;
  }
  if (step === 0) return null;
  if (current < 0) return step > 0 ? 0 : last;
  const next = current + step;
  if (wrap) return (next + count) % count;
  return Math.min(Math.max(next, 0), last);
}

/** The keys that open a context menu: the Menu key, or Shift+F10. */
export function isMenuKey(event: { key: string; shiftKey: boolean }): boolean {
  return event.key === "ContextMenu" || (event.shiftKey && event.key === "F10");
}
