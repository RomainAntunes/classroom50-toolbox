// One polite live region (in the layout) for what happens out of sight: a sync that ends,
// an action that finishes. Screen readers read it without moving the focus.

export const announcer = $state({ text: "" });

let timer: ReturnType<typeof setTimeout> | undefined;

/** Has screen readers read `text`, even when it is the same as the last one. */
export function announce(text: string): void {
  announcer.text = "";
  clearTimeout(timer);
  timer = setTimeout(() => {
    announcer.text = text;
  }, 100);
}
