// Page-wide checks shared by the layout and right-to-left specs: raw catalog keys or
// unfilled placeholders in the text, and anything sticking out of the popover sideways.
import type { Page } from "@playwright/test";
import en from "../../src/lib/i18n/en.json" with { type: "json" };

/** Namespaces of catalog keys: `t` shows a missing key as is, e.g. `projects.filter.foo`. */
const NAMESPACES = [...new Set(Object.keys(en).map((key) => key.split(".")[0]))];
const FILE_EXTENSIONS = ["c", "md", "json", "py", "txt", "yaml", "org", "com", "io"];

/** Raw keys and `{placeholders}` in the page's text and labels. */
export async function untranslated(page: Page): Promise<string[]> {
  const texts = await page.evaluate(() => {
    const out = [document.body.innerText];
    for (const el of document.querySelectorAll("[title], [aria-label], [placeholder]")) {
      for (const name of ["title", "aria-label", "placeholder"]) {
        const value = el.getAttribute(name);
        if (value) out.push(value);
      }
    }
    return out;
  });
  const found = new Set<string>();
  for (const text of texts) {
    for (const [token] of text.matchAll(/\b[a-z][a-zA-Z0-9]*(?:\.[a-zA-Z0-9-]+)+\b/g)) {
      const parts = token.split(".");
      const last = parts[parts.length - 1];
      if (NAMESPACES.includes(parts[0]) && !FILE_EXTENSIONS.includes(last)) found.add(token);
    }
    for (const [token] of text.matchAll(/\{\w+\}/g)) found.add(token);
  }
  return [...found];
}

/** Visible elements sticking out of the popover sideways, and scrolling containers that
 *  scroll sideways. Clipped content (an ellipsis, a scroller's own content) doesn't count. */
export async function overflow(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const popover = document.querySelector(".popover") as HTMLElement;
    const bounds = popover.getBoundingClientRect();
    const out: string[] = [];
    const describe = (el: Element) =>
      `${el.tagName.toLowerCase()}.${[...el.classList].join(".")} "${(el.textContent ?? "").trim().slice(0, 40)}"`;
    const clipped = (el: Element) => {
      for (let p = el.parentElement; p && p !== popover; p = p.parentElement) {
        if (getComputedStyle(p).overflowX !== "visible") return true;
      }
      return false;
    };
    if (document.documentElement.scrollWidth > window.innerWidth) {
      out.push(`page scrolls sideways: ${document.documentElement.scrollWidth}px`);
    }
    for (const el of popover.querySelectorAll("*")) {
      if (el.closest(".sr-only")) continue;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      const style = getComputedStyle(el);
      if (style.overflowX === "auto" || style.overflowX === "scroll") {
        if (el.scrollWidth > el.clientWidth + 1) out.push(`scrolls sideways: ${describe(el)}`);
      }
      if (clipped(el)) continue;
      if (rect.right > bounds.right + 1 || rect.left < bounds.left - 1) {
        out.push(
          `outside the popover: ${describe(el)} (${Math.round(rect.left)}..${Math.round(rect.right)})`,
        );
      }
    }
    return out;
  });
}
