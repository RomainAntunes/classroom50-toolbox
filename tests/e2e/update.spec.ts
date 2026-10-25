// Self-update: `update-state` drives the main page's banner and Settings > About; the
// student picks Restart now or Later, and Settings holds the check and the channel.
import { expect, test, type App } from "./app";
import { fail, hold, ok, scenario, settings, updateStatus } from "./fixtures";
import type { UpdateState } from "../../src/lib/api";

const downloading = (downloaded: number, total: number | null): UpdateState => ({
  phase: "downloading",
  version: "9.9.0",
  downloaded,
  total,
});
const ready: UpdateState = { phase: "ready", version: "9.9.0", notes: null };

async function state(app: App, next: UpdateState, over = {}) {
  await app.emit("update-state", updateStatus({ state: next, ...over }));
}

test("idle shows no banner; a download shows a bar from the bytes, then the question", async ({
  app,
  page,
}) => {
  await app.start(scenario());
  await expect(page.getByRole("searchbox")).toBeVisible();
  const banner = page.locator("main .banner");
  await expect(banner).toHaveCount(0);

  await state(app, downloading(3_000_000, 10_000_000));
  await expect(banner).toContainText("Downloading version 9.9.0");
  await expect(banner).toContainText("3.0 MB of 10.0 MB");
  const bar = banner.getByRole("progressbar");
  await expect(bar).toHaveAttribute("aria-valuenow", "30");

  // No length from the server: the bar only says it works.
  await state(app, downloading(4_000_000, null));
  await expect(banner).toContainText("4.0 MB");
  await expect(bar).not.toHaveAttribute("aria-valuenow");

  await state(app, ready);
  await expect(banner).toContainText("Version 9.9.0 is ready");
  await expect(banner).toContainText("Restart now, or it installs when you quit the app.");
  await expect(banner.getByRole("button", { name: "Later" })).toBeVisible();
  await expect(banner.getByRole("button", { name: "Restart now" })).toBeEnabled();
  expect(await app.calls("install_update")).toEqual([]);
});

test("Restart now installs; the button says so meanwhile", async ({ app, page }) => {
  await app.start(scenario({ update: updateStatus({ state: ready }) }));
  const banner = page.locator("main .banner");
  await banner.getByRole("button", { name: "Restart now" }).click();
  await app.waitForCall("install_update");
  await expect(banner.getByRole("button", { name: "Installing…" })).toBeDisabled();
  expect(await app.calls("defer_update")).toEqual([]);
});

test("Later keeps the banner without the question", async ({ app, page }) => {
  await app.start(scenario({ update: updateStatus({ state: ready }) }));
  const banner = page.locator("main .banner");
  await banner.getByRole("button", { name: "Later" }).click();
  await app.waitForCall("defer_update");
  await expect(banner.getByRole("button", { name: "Later" })).toHaveCount(0);
  await expect(banner).toContainText("It installs when you quit the app.");
  await expect(banner.getByRole("button", { name: "Restart now" })).toBeVisible();
  expect(await app.calls("install_update")).toEqual([]);
});

test("deb and rpm say Restart asks for the password", async ({ app, page }) => {
  await app.start(
    scenario({
      update: updateStatus({
        state: ready,
        bundle: "deb",
        installOnQuit: false,
        asksPassword: true,
      }),
    }),
  );
  await expect(page.locator("main .banner")).toContainText(
    "Restarting installs it and asks for your password.",
  );
});

test("a failed restart shows Rust's words", async ({ app, page }) => {
  await app.start(
    scenario({
      update: updateStatus({ state: ready }),
      responses: { install_update: [fail("updater", "signature mismatch")] },
    }),
  );
  await page.locator("main .banner").getByRole("button", { name: "Restart now" }).click();
  const dialog = await app.expectDialog(["signature mismatch"], "ok");
  expect(dialog.kind).toBe("error");
});

test("Settings > About shows the same update, with What's new instead of Later", async ({
  app,
  page,
}) => {
  await app.start(scenario({ update: updateStatus({ state: ready }) }));
  await app.openSettings();
  const about = page.locator(".banner.compact");
  await expect(about).toContainText("Version 9.9.0 is ready");
  await expect(about.getByRole("button", { name: "Later" })).toHaveCount(0);
  await about.getByRole("button", { name: "What's new" }).click();
  const opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened[0].args.url).toBe(
    "https://github.com/RomainAntunes/classroom50-toolbox/releases/tag/v9.9.0",
  );
  // Already downloaded: nothing to check.
  await expect(page.getByRole("button", { name: "Check for updates" })).toBeDisabled();
});

test("Check for updates says when it's up to date", async ({ app, page }) => {
  await app.start(scenario());
  await app.openSettings();
  await page.getByRole("button", { name: "Check for updates" }).click();
  await app.waitForCall("check_update");
  await expect(page.getByRole("button", { name: "You're up to date" })).toBeVisible();
});

test("Check for updates spins while it asks", async ({ app, page }) => {
  await app.start(scenario({ responses: { check_update: [hold()] } }));
  await app.openSettings();
  const check = page.getByRole("button", { name: "Check for updates" });
  await check.click();
  await expect(check).toHaveAttribute("aria-busy", "true");
  await expect(check.locator(".spinner")).toBeVisible();
  await app.release("check_update", ok(updateStatus()));
  const done = page.getByRole("button", { name: "You're up to date" });
  await expect(done).toHaveAttribute("aria-busy", "false");
  await expect(done.locator(".spinner")).toHaveCount(0);
});

test("a failed check is worded by its kind", async ({ app, page }) => {
  await app.start(scenario({ responses: { check_update: [fail("offline")] } }));
  await app.openSettings();
  await page.getByRole("button", { name: "Check for updates" }).click();
  await app.expectDialog(["can't reach GitHub"], "ok");
});

test("a check that broke in the background shows under Updates", async ({ app, page }) => {
  await app.start(
    scenario({
      update: updateStatus({
        state: { phase: "error", error: { kind: "offline", message: "dns error" } },
      }),
    }),
  );
  await app.openSettings();
  await expect(page.locator(".sub.issue")).toHaveText("can't reach GitHub");
});

test("a copy that can't update itself says so and offers no check", async ({ app, page }) => {
  await app.start(scenario({ update: updateStatus({ canUpdate: false, bundle: "unbundled" }) }));
  await app.openSettings();
  await expect(page.getByText("this copy of the toolbox can't update itself")).toBeVisible();
  await expect(page.getByRole("button", { name: "Check for updates" })).toBeDisabled();
});

test("the update channel is a menu saved through Rust", async ({ app, page }) => {
  await app.start(scenario());
  await app.openSettings();
  const channel = page.getByRole("button", { name: "Update channel: Stable" });
  await expect(page.getByText("Tested releases only.")).toBeVisible();
  await channel.click();
  const menu = await app.menu();
  expect(menu.items.map((item) => [item.text, item.checked])).toEqual([
    ["Stable", true],
    ["Beta", false],
  ]);
  await app.pickMenu("Beta");
  await expect(page.getByRole("button", { name: "Update channel: Beta" })).toBeVisible();
  expect((await app.settings()).updateChannel).toBe("beta");
});

test("an MSI install stays on Stable, with the reason", async ({ app, page }) => {
  await app.start(
    scenario({
      settings: settings({ updateChannel: "beta" }),
      update: updateStatus({ bundle: "msi", hasBeta: false }),
    }),
  );
  await app.openSettings();
  await expect(page.getByRole("button", { name: "Update channel: Stable" })).toBeDisabled();
  await expect(page.getByText("this MSI install stays on Stable")).toBeVisible();
});
