// A text prompt drawn inside the popover. The dialog plugin has no text input, and a native
// window would take focus and hide the popover.

export interface PromptOptions {
  title: string;
  message?: string;
  placeholder?: string;
  okLabel: string;
}

export const prompt = $state<{
  current: (PromptOptions & { resolve: (value: string | null) => void }) | null;
}>({ current: null });

/** Shows the prompt; resolves to the trimmed text, or null when cancelled or empty. */
export function askText(options: PromptOptions): Promise<string | null> {
  prompt.current?.resolve(null);
  return new Promise((resolve) => {
    prompt.current = {
      ...options,
      resolve: (value) => {
        prompt.current = null;
        resolve(value?.trim() || null);
      },
    };
  });
}
