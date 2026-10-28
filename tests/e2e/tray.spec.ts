// The tray icon's menu: the layout words it and sends it to Rust (`set_tray_menu`) whatever
// page shows, again when a sync, the language or the date changes it, and never twice the
// same. Rust hands some items back (`tray-action`).
import { expect, test, type App } from "./app";
import { scenario, settings, snapshot } from "./fixtures";
import type { TrayMenu } from "../../src/lib/api";

// Zurich time: 28 October 2026, synced at 14:32, the app started at 15:00.
test.use({ timezoneId: "Europe/Zurich" });
const SYNCED = "2026-10-28T13:32:00Z";
const synced = snapshot({ syncedAt: SYNCED, checkedAt: SYNCED });

// ICU puts a narrow no-break space before "PM".
const plain = (text: string | null) => text?.replace(/\s/g, " ") ?? null;

/** Every menu sent so far, the last one last. */
async function menus(app: App): Promise<TrayMenu[]> {
  return (await app.calls("set_tray_menu")).map((call) => {
    const menu = call.args.menu as TrayMenu;
    return { ...menu, synced: plain(menu.synced) };
  });
}

async function lastMenu(app: App): Promise<TrayMenu | undefined> {
  return (await menus(app)).at(-1);
}

/** Starts on the list, its listeners in place. */
async function start(app: App) {
  await app.start(scenario({ snapshot: synced }));
  await expect(app.page.getByRole("searchbox")).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-28T14:00:00Z") });
});

test("the menu holds Refresh, when GitHub was read and Settings", async ({ app }) => {
  await start(app);
  await expect
    .poll(() => lastMenu(app))
    .toEqual({
      open: "Open Classroom 50 Toolbox",
      quit: "Quit",
      refresh: "Refresh",
      settings: "Settings…",
      synced: "Synced today at 2:32 PM",
      deadlines: [],
      recentTitle: "Open Recent",
      recent: [],
      update: null,
      validUntil: "2026-10-28T23:00:00.000Z",
    } satisfies TrayMenu);
});

test("a sync moves the synced line, and the same menu isn't sent twice", async ({ app }) => {
  await start(app);
  await expect.poll(async () => (await lastMenu(app))?.synced).toBe("Synced today at 2:32 PM");

  const later = "2026-10-28T13:58:00Z";
  await app.emit("projects-updated", snapshot({ syncedAt: SYNCED, checkedAt: later }));
  await expect.poll(async () => (await lastMenu(app))?.synced).toBe("Synced today at 2:58 PM");
  const sent = (await menus(app)).length;

  // Same words: nothing is sent.
  await app.emit("projects-updated", snapshot({ syncedAt: SYNCED, checkedAt: later }));
  await app.emit("settings-updated", settings());
  await app.page.clock.runFor(60_000);
  expect(await menus(app)).toHaveLength(sent);
});

test("the menu follows the UI's language", async ({ app }) => {
  await start(app);
  await expect.poll(async () => (await lastMenu(app))?.refresh).toBe("Refresh");

  await app.emit("settings-updated", settings({ language: "fr" }));
  await expect
    .poll(() => lastMenu(app))
    .toMatchObject({
      open: "Ouvrir Classroom 50 Toolbox",
      quit: "Quitter",
      refresh: "Actualiser",
      settings: "Réglages…",
      synced: "Synchronisé aujourd’hui à 14:32",
      recentTitle: "Ouvrir un élément récent",
    });
});

test("past midnight Rust asks again and today becomes yesterday; the UI has no timer", async ({
  app,
}) => {
  await start(app);
  await expect.poll(async () => (await lastMenu(app))?.synced).toBe("Synced today at 2:32 PM");
  const sent = (await menus(app)).length;

  // Hidden all evening and past midnight: the UI sends nothing by itself.
  await app.emit("popover-hidden", null);
  await app.page.clock.runFor(10 * 3_600_000);
  expect(await menus(app)).toHaveLength(sent);
  // Rust's tick saw `validUntil` pass.
  await app.emit("tray-expired", null);
  await expect.poll(async () => (await lastMenu(app))?.synced).toBe("Synced yesterday at 2:32 PM");
  expect((await lastMenu(app))?.validUntil).toBe("2026-10-29T23:00:00.000Z");
});

test("startup sends one menu, in the saved language", async ({ app }) => {
  await app.start(scenario({ snapshot: synced, settings: settings({ language: "fr" }) }));
  await expect(app.page.getByRole("searchbox")).toBeVisible();
  await expect.poll(async () => (await menus(app)).length).toBe(1);
  expect((await lastMenu(app))?.refresh).toBe("Actualiser");
  // A settings save that leaves the hidden lists alone sends nothing.
  await app.emit("settings-updated", settings({ language: "fr", listFilter: "week" }));
  await app.page.clock.runFor(1000);
  expect(await menus(app)).toHaveLength(1);
});

test("Settings from the menu opens Settings", async ({ app, page }) => {
  await start(app);
  await app.emit("tray-action", { kind: "settings" });
  await expect(page).toHaveURL(/\/settings$/);
  await expect(page.locator("main section").first()).toBeVisible();
});

test("Settings from the menu leaves the first-run guide where it is", async ({ app, page }) => {
  await app.start(scenario({ settings: settings({ onboarded: false }) }), "/welcome");
  await expect(page.getByRole("heading").first()).toBeVisible();
  await app.emit("tray-action", { kind: "settings" });
  await page.clock.runFor(1000);
  await expect(page).toHaveURL(/\/welcome$/);
});
