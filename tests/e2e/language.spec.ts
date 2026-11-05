// Each language but English is its own chunk, loaded when it is first shown: Settings spins
// on the Language button until the picked one loads, and only then saves it; one that
// won't load keeps the language shown. A saved language reaches Rust's words once.
import type { Page } from "@playwright/test";
import { expect, test } from "./app";
import { scenario, settings } from "./fixtures";
import fr from "../../src/lib/i18n/fr.json" with { type: "json" };

/**
 * Holds the request for the catalog named `name` in itself (its `language.name`, which only
 * that chunk holds as a key's value) until `settle(true)` serves it or `settle(false)`
 * fails it. Every other request goes through.
 */
async function holdCatalog(page: Page, name: string) {
  let settle!: (ok: boolean) => void;
  const decided = new Promise<boolean>((resolve) => (settle = resolve));
  let requested = false;
  const own = new RegExp(`"language\\.name":\\s*[\`"']${name}[\`"']`);
  await page.route("**/_app/immutable/**/*.js", async (route) => {
    const response = await route.fetch();
    const body = await response.text();
    if (!own.test(body)) return route.fulfill({ response, body });
    requested = true;
    if (await decided) await route.fulfill({ response, body });
    else await route.abort();
  });
  return { settle, requested: () => requested };
}

const languageCalls = async (app: { calls: (cmd: string) => Promise<{ args: unknown }[]> }) =>
  (await app.calls("update_settings")).filter(
    (call) => (call.args as { next: { language: string } }).next.language !== "en",
  );

test("English loads no other catalog", async ({ app, page }) => {
  const others: string[] = [];
  page.on("response", async (response) => {
    if (!response.url().endsWith(".js")) return;
    const body = await response.text().catch(() => "");
    const name = /"language\.name":\s*[`"']([^`"']+)[`"']/.exec(body)?.[1];
    if (name && name !== "English") others.push(name);
  });
  await app.start(scenario());
  await expect(page.getByRole("searchbox")).toBeVisible();
  await app.openSettings();
  await expect(page.getByRole("button", { name: /^Language:/ })).toHaveText("English");
  expect(others).toEqual([]);
});

test("a picked language spins until its catalog loads, then switches", async ({ app, page }) => {
  const catalog = await holdCatalog(page, "Deutsch");
  await app.start(scenario());
  await app.openSettings();
  const language = page.getByRole("button", { name: /^Language:/ });
  await expect(language).toHaveText("English");

  await language.click();
  await app.pickMenu("Deutsch");
  await expect(language).toHaveAttribute("aria-busy", "true");
  await expect(language.locator(".spinner")).toBeVisible();
  await expect.poll(() => catalog.requested()).toBe(true);
  // Nothing is saved, nor shown, before the words are there.
  expect(await languageCalls(app)).toEqual([]);
  await expect(page.locator("html")).toHaveAttribute("lang", "en");

  catalog.settle(true);
  const switched = page.getByRole("button", { name: /^Sprache:/ });
  await expect(switched).toHaveText("Deutsch");
  await expect(switched).toHaveAttribute("aria-busy", "false");
  await expect(switched.locator(".spinner")).toHaveCount(0);
  await expect(page.locator("html")).toHaveAttribute("lang", "de");
  await expect(page.getByRole("heading", { name: "Allgemein" })).toBeVisible();
  expect((await app.settings()).language).toBe("de");
  expect(await languageCalls(app)).toHaveLength(1);
});

test("a catalog that won't load keeps the language shown and says so", async ({ app, page }) => {
  const catalog = await holdCatalog(page, "Deutsch");
  await app.start(scenario());
  await app.openSettings();
  const language = page.getByRole("button", { name: /^Language:/ });

  await language.click();
  await app.pickMenu("Deutsch");
  await expect(language).toHaveAttribute("aria-busy", "true");
  catalog.settle(false);
  await app.expectDialog(["Couldn't load Deutsch. The current language stays."], "ok");

  await expect(language).toHaveText("English");
  await expect(language).toHaveAttribute("aria-busy", "false");
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  expect((await app.settings()).language).toBe("en");
  expect(await languageCalls(app)).toEqual([]);
});

test("a saved language reaches Rust's calendar words once, in that language", async ({
  app,
  page,
}) => {
  await app.start(scenario({ settings: settings({ language: "fr" }) }));
  await expect(page.locator("html")).toHaveAttribute("lang", "fr");
  // The tray menu goes out with them, once the first loads are in.
  await app.waitForCall("set_tray_menu");
  const calls = await app.calls("set_calendar_labels");
  expect(calls.map((call) => (call.args as { labels: { class: string } }).labels.class)).toEqual([
    fr["calendar.notes.class"],
  ]);
});

test("a notice arriving before the saved language loads is worded in it, in order", async ({
  app,
  page,
}) => {
  const catalog = await holdCatalog(page, "Français");
  await app.start(scenario({ settings: settings({ language: "fr" }) }));
  // The layout listens once mounted; the page shows in English while French loads.
  await expect(page.getByRole("searchbox")).toBeVisible();
  await expect.poll(() => catalog.requested()).toBe(true);
  await app.emit("notify", { kind: "grade", assignment: "Lab 1", score: 8, maxScore: 10 });
  await app.emit("notify", { kind: "closed", assignment: "Lab 2" });
  // Nothing goes out in English meanwhile.
  await page.waitForTimeout(200);
  expect(await app.notifications()).toEqual([]);

  catalog.settle(true);
  await expect
    .poll(() => app.notifications())
    .toEqual([
      { title: fr["alerts.grade.title"].replace("{assignment}", "Lab 1"), body: "8/10" },
      {
        title: fr["alerts.closed.title"].replace("{assignment}", "Lab 2"),
        body: fr["alerts.closed.body"],
      },
    ]);
  await expect(page.locator("html")).toHaveAttribute("lang", "fr");
});

test("a notice still goes out, in English, when the saved language won't load", async ({
  app,
  page,
}) => {
  const catalog = await holdCatalog(page, "Français");
  await app.start(scenario({ settings: settings({ language: "fr" }) }));
  await expect(page.getByRole("searchbox")).toBeVisible();
  await expect.poll(() => catalog.requested()).toBe(true);
  await app.emit("notify", { kind: "grade", assignment: "Lab 1", score: 8, maxScore: 10 });
  catalog.settle(false);
  await expect
    .poll(() => app.notifications())
    .toEqual([{ title: "New grade: Lab 1", body: "8/10" }]);
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
});

test("the link that started the app is taken at once and asks in the saved language", async ({
  app,
  page,
}) => {
  const catalog = await holdCatalog(page, "Français");
  const link = {
    kind: "accept",
    org: "acme-school",
    classroom: "cs101",
    assignment: "lab9",
    key: null,
  } as const;
  await app.start(
    scenario({ settings: settings({ language: "fr" }), deepLink: { kind: "unlisted", link } }),
  );
  await app.waitForCall("take_deep_link");
  await expect.poll(() => catalog.requested()).toBe(true);
  await page.waitForTimeout(200);
  expect(await app.pendingDialogs()).toEqual([]);

  catalog.settle(true);
  const ask = await app.dialog();
  expect(ask.message).toBe(
    fr["link.openAssignment"]
      .replace("{assignment}", "lab9")
      .replace("{classroom}", "acme-school/cs101"),
  );
  await app.answer("cancel");
  await app.waitForCall("deep_link_done");
  expect(await app.writes()).toEqual([]);
});
