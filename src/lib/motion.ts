// The system's reduced-motion setting, for motion CSS can't reach: smooth scrolling started
// from script and Svelte transitions. CSS animations follow it in theme.css.

function reduced(): boolean {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** "smooth", or "auto" (a jump) when the system asks for less motion. */
export function scrollBehavior(): ScrollBehavior {
  return reduced() ? "auto" : "smooth";
}

/** A Svelte transition's duration in ms: `ms`, or 0 when the system asks for less motion. */
export function duration(ms: number): number {
  return reduced() ? 0 : ms;
}
