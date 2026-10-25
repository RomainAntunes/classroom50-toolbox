// Busy buttons: a spinner that shows long enough to be seen, even when the work is quick.

/** How long a busy button spins at least, so a fast answer still reads as "checked". */
export const MIN_BUSY_MS = 300;

/** `work`'s result (or error), not before `ms` have passed. Slower work isn't padded. */
export async function atLeast<T>(work: Promise<T>, ms = MIN_BUSY_MS): Promise<T> {
  const [result] = await Promise.allSettled([
    work,
    new Promise((resolve) => setTimeout(resolve, ms)),
  ]);
  if (result.status === "rejected") throw result.reason;
  return result.value;
}
