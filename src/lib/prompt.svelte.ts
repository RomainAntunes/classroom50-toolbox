// A text prompt drawn inside the popover. The dialog plugin has no text input, and a native
// window would take focus and hide the popover.

export interface PromptOptions {
  title: string;
  message?: string;
  placeholder?: string;
  /** The input's accessible name, when the title names something else. */
  label?: string;
  /** The input's `maxlength`, in UTF-16 units. */
  maxLength?: number;
  okLabel: string;
  /** A second action next to the main one (`askTextAction` only). */
  altLabel?: string;
  /** Read-only text shown instead of the input (`showText` only). */
  text?: string;
}

export const prompt = $state<{
  current: (PromptOptions & { resolve: (value: string | null, alt?: boolean) => void }) | null;
}>({ current: null });

/** Shows the prompt; resolves to the trimmed text and whether the second action was chosen,
 *  or null when cancelled or empty. */
export function askTextAction(
  options: PromptOptions,
): Promise<{ text: string; alt: boolean } | null> {
  prompt.current?.resolve(null);
  return new Promise((resolve) => {
    prompt.current = {
      ...options,
      resolve: (value, alt = false) => {
        prompt.current = null;
        const text = value?.trim();
        resolve(text ? { text, alt } : null);
      },
    };
  });
}

/** Shows the prompt; resolves to the trimmed text, or null when cancelled or empty. */
export async function askText(options: PromptOptions): Promise<string | null> {
  const answer = await askTextAction({ ...options, altLabel: undefined });
  return answer?.text ?? null;
}

/** Shows read-only text (monospace, scrollable) with Cancel and `okLabel`; resolves to
 *  whether the main action was chosen. */
export function showText(options: PromptOptions & { text: string }): Promise<boolean> {
  prompt.current?.resolve(null);
  return new Promise((resolve) => {
    prompt.current = {
      ...options,
      altLabel: undefined,
      resolve: (value) => {
        prompt.current = null;
        resolve(value !== null);
      },
    };
  });
}
