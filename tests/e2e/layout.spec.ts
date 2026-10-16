// Every registered catalog in every platform skin: the main page and Settings show no raw
// catalog key or unfilled placeholder, and nothing spills out of the popover's width.
import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";
import { expect, test } from "./app";
import { richScenario } from "./samples";
import en from "../../src/lib/i18n/en.json" with { type: "json" };

/** The catalogs `src/lib/i18n/index.ts` registers, read from its `catalogs` line. */
const source = readFileSync(new URL("../../src/lib/i18n/index.ts", import.meta.url), "utf8");
const LOCALES = /export const catalogs[^=]*=\s*\{([^}]*)\}/
  .exec(source)![1]
  .split(",")
  .map((code) => code.trim().replace(/["']/g, "").split(":")[0])
  .filter(Boolean);
const SKINS = ["macos", "windows", "linux"] as const;

/** Namespaces of catalog keys: `t` shows a missing key as is, e.g. `projects.filter.foo`. */
const NAMESPACES = [...new Set(Object.keys(en).map((key) => key.split(".")[0]))];
const FILE_EXTENSIONS = ["c", "md", "json", "py", "txt", "yaml", "org", "com", "io"];

/** Raw keys and `{placeholders}` in the page's text and labels. */
async function untranslated(page: Page): Promise<string[]> {
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
async function overflow(page: Page): Promise<string[]> {
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

for (const locale of LOCALES) {
  for (const os of SKINS) {
    test(`${locale} on ${os}: main page and Settings are fully translated and fit`, async ({
      app,
      page,
    }) => {
      const scenario = richScenario({ language: locale });
      scenario.appInfo.os = os;
      await app.start(scenario);
      await expect(page.locator("html")).toHaveAttribute("data-os", os);
      await expect(page.locator("html")).toHaveAttribute("lang", locale);
      await expect(page.locator("li[data-item]").first()).toBeVisible();
      // Open a row's details too: they hold the longest labels.
      await page.locator("[data-row]").first().click();

      expect(await untranslated(page), "raw keys on the main page").toEqual([]);
      expect(await overflow(page), "overflow on the main page").toEqual([]);

      await page.locator(".titlebar .icon").last().click();
      await expect(page).toHaveURL(/\/settings$/);
      await expect(page.locator("main section").first()).toBeVisible();
      expect(await untranslated(page), "raw keys in Settings").toEqual([]);
      expect(await overflow(page), "overflow in Settings").toEqual([]);
    });
  }
}
