// Every registered catalog in every platform skin: the main page, the phases editor and
// Settings show no raw catalog key or unfilled placeholder, and nothing spills out of the
// popover's width. The first-run guide too, step by step, in one skin.
import { readFileSync } from "node:fs";
import { expect, test } from "./app";
import { overflow, untranslated } from "./checks";
import { richScenario } from "./samples";
import { scenario, settings, tools } from "./fixtures";
import { textDirection } from "../../src/lib/i18n/direction";
import { catalogCodes } from "../../src/lib/changelog/versions.js";

/** The catalogs `src/lib/i18n/index.ts` registers, read from its `languages` list. */
const source = readFileSync(new URL("../../src/lib/i18n/index.ts", import.meta.url), "utf8");
const LOCALES = catalogCodes(source);
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
      // And its Markdown description, unfolded: a heading, a table, a task list, math.
      await page.locator(".details .more-text").click();
      await expect(page.locator(".details .markdown table")).toBeVisible();
      expect(await overflow(page), "overflow in the Markdown description").toEqual([]);
      // And a group's, graded by hand, with its size.
      await app
        .row("Final Project with a Rather Long Name for Wrapping")
        .locator("[data-row]")
        .click();

      expect(await untranslated(page), "raw keys on the main page").toEqual([]);
      expect(await overflow(page), "overflow on the main page").toEqual([]);

      // The phases editor: two rows of inputs, a checkbox and buttons in one card.
      await app.rowMenu("Portfolio");
      await app.pickMenu("phases");
      await expect(app.prompt.locator("li")).toHaveCount(2);
      expect(await untranslated(page), "raw keys in the phases editor").toEqual([]);
      expect(await overflow(page), "overflow in the phases editor").toEqual([]);
      await page.keyboard.press("Escape");
      await expect(app.prompt).toHaveCount(0);

      await page.locator(".titlebar .icon").last().click();
      await expect(page).toHaveURL(/\/settings$/);
      await expect(page.locator("main section").first()).toBeVisible();
      expect(await untranslated(page), "raw keys in Settings").toEqual([]);
      expect(await overflow(page), "overflow in Settings").toEqual([]);

      // And the list with how assignments are graded hidden (the group's details stay open).
      const catalog = JSON.parse(
        readFileSync(new URL(`../../src/lib/i18n/${locale}.json`, import.meta.url), "utf8"),
      ) as Record<string, string>;
      await page.getByRole("switch", { name: catalog["settings.gradingInfo"] }).click();
      await expect.poll(async () => (await app.settings()).showGradingInfo).toBe(false);
      await page.locator(".titlebar .icon").first().click();
      await expect(page.locator("li[data-item]").first()).toBeVisible();
      expect(await untranslated(page), "raw keys with grading hidden").toEqual([]);
      expect(await overflow(page), "overflow with grading hidden").toEqual([]);
    });
  }
}

for (const locale of LOCALES) {
  test(`${locale}: every step of the first-run guide is translated and fits`, async ({
    app,
    page,
  }) => {
    await app.start(
      scenario({
        settings: settings({ onboarded: false, cloneFolder: null, language: locale }),
        tools: tools(["gh-student"]),
      }),
    );
    await expect(page.locator("html")).toHaveAttribute("lang", locale);
    const next = page.locator("footer .nav .primary");
    const dots = page.locator("footer .dots span");
    const steps = await dots.count();
    for (let step = 0; step < steps; step += 1) {
      await expect(dots.nth(step)).toHaveClass(/active/);
      // The git step: its rows once git and gh answered.
      if (step === 3) await expect(page.locator("#ssh")).toBeVisible();
      expect(await untranslated(page), `raw keys in step ${step + 1}`).toEqual([]);
      expect(await overflow(page), `overflow in step ${step + 1}`).toEqual([]);
      if (step < steps - 1) await next.click();
    }
  });
}
