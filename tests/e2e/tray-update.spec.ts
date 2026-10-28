// The tray menu's "Restart to Update to X": there while a downloaded update waits, gone
// while it installs, and it runs what the banner's Restart now runs.
import { expect, test, type App } from "./app";
import { fail, scenario, updateStatus } from "./fixtures";
import type { TrayMenu, UpdateState } from "../../src/lib/api";

const ready: UpdateState = { phase: "ready", version: "9.9.0", notes: null };

/** The last menu sent to Rust. */
async function lastMenu(app: App): Promise<TrayMenu | undefined> {
  return (await app.calls("set_tray_menu")).at(-1)?.args.menu as TrayMenu | undefined;
}

test("the item comes with a downloaded update and goes while it installs", async ({
  app,
  page,
}) => {
  await app.start(scenario());
  await expect(page.getByRole("searchbox")).toBeVisible();
  await expect.poll(async () => (await lastMenu(app))?.refresh).toBe("Refresh");
  expect((await lastMenu(app))?.update).toBeNull();

  await app.emit("update-state", updateStatus({ state: ready }));
  await expect.poll(async () => (await lastMenu(app))?.update).toBe("Restart to Update to 9.9.0");

  await app.emit("update-state", updateStatus({ state: ready, installing: true }));
  await expect.poll(async () => (await lastMenu(app))?.update).toBeNull();
});

test("Restart to Update installs as Restart now does", async ({ app, page }) => {
  await app.start(scenario({ update: updateStatus({ state: ready }) }));
  await expect.poll(async () => (await lastMenu(app))?.update).toBe("Restart to Update to 9.9.0");

  await app.emit("tray-action", { kind: "installUpdate" });
  await app.waitForCall("install_update");
  // Rust says it installs: the menu drops the item and the banner says so.
  await expect.poll(async () => (await lastMenu(app))?.update).toBeNull();
  await expect(
    page.locator("main .banner").getByRole("button", { name: "Installing…" }),
  ).toBeDisabled();
});

test("a failed Restart to Update shows Rust's words", async ({ app }) => {
  await app.start(
    scenario({
      update: updateStatus({ state: ready }),
      responses: { install_update: [fail("updater", "signature mismatch")] },
    }),
  );
  await expect.poll(async () => (await lastMenu(app))?.update).toBe("Restart to Update to 9.9.0");

  await app.emit("tray-action", { kind: "installUpdate" });
  const dialog = await app.expectDialog(["signature mismatch"], "ok");
  expect(dialog.kind).toBe("error");
});
