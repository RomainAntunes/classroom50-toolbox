// Every registered catalog in every platform skin: the main page and Settings show no raw
// catalog key or unfilled placeholder, and nothing spills out of the popover's width.
import { readFileSync } from "node:fs";
import { expect, test } from "./app";
import { overflow, untranslated } from "./checks";
import { richScenario } from "./samples";
import { textDirection } from "../../src/lib/i18n/direction";

/** The catalogs `src/lib/i18n/index.ts` registers, read from its `catalogs` line. */
const source = readFileSync(new URL("../../src/lib/i18n/index.ts", import.meta.url), "utf8");
const LOCALES = /export const catalogs[^=]*=\s*\{([^}]*)\}/
  .exec(source)![1]
  .split(",")
  .map((code) => code.trim().replace(/["']/g, "").split(":")[0])
  .filter(Boolean);
const SKINS = ["macos", "windows", "linux"] as const;

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
      await expect(page.locator("html")).toHaveAttribute("dir", textDirection(locale));
      await expect(page.locator("li[data-item]").first()).toBeVisible();
      // Open a row's details too: they hold the longest labels, and its failed test, which
      // the autograder stopped at its time limit.
      await page.locator("[data-row]").first().click();
      await page.locator(".details button.test").first().click();
      await expect(page.locator(".details .timeout")).toBeVisible();

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
