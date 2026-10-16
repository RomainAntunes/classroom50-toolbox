// Settings > General > Keyboard shortcut: record a combination (Rust registers it on save),
// Escape cancels, Clear turns it off, Shift alone and taken shortcuts are refused.
import type { Page } from "@playwright/test";
import { expect, test } from "./app";
import { fail, pass, project, scenario, settings, snapshot } from "./fixtures";
import type { Settings } from "../../src/lib/api";

function onWindows(over: Partial<Settings> = {}, globalShortcuts = true) {
  const s = scenario({ settings: settings(over) });
  s.appInfo = { ...s.appInfo, os: "windows", globalShortcuts };
  return s;
}

const recorder = (page: Page) => page.getByRole("button", { name: /^Keyboard shortcut:/ });

test("recording a combination saves it and shows it the platform's way", async ({ app, page }) => {
  await app.start(onWindows());
  await app.openSettings();
  await expect(recorder(page)).toHaveAccessibleName("Keyboard shortcut: Record");
  await recorder(page).click();
  // The saved shortcut is let go while recording, so pressing it reaches the recorder.
  expect((await app.waitForCall("pause_global_shortcut"))[0].args).toEqual({ paused: true });
  await expect(recorder(page)).toHaveText("Press keys…");
  await expect(recorder(page)).toHaveAttribute("aria-pressed", "true");

  await page.keyboard.press("Control+Alt+KeyK");
  await expect(recorder(page)).toHaveText("Ctrl+Alt+K");
  expect((await app.settings()).globalShortcut).toBe("Control+Alt+KeyK");
  const pauses = await app.calls("pause_global_shortcut");
  expect(pauses.map((call) => call.args.paused)).toEqual([true, false]);
  await expect(page.getByRole("button", { name: "Clear", exact: true })).toBeVisible();
});

test("Escape cancels recording and keeps the popover open", async ({ app, page }) => {
  await app.start(onWindows({ globalShortcut: "Control+Alt+KeyC" }));
  await app.openSettings();
  await recorder(page).click();
  await expect(recorder(page)).toHaveText("Press keys…");
  await page.keyboard.press("Escape");
  await expect(recorder(page)).toHaveText("Ctrl+Alt+C");
  expect((await app.settings()).globalShortcut).toBe("Control+Alt+KeyC");
  expect(await app.calls("hide_popover")).toEqual([]);
  expect(await app.calls("update_settings")).toEqual([]);
});

test("Clear turns the shortcut off", async ({ app, page }) => {
  await app.start(onWindows({ globalShortcut: "Control+Alt+KeyC" }));
  await app.openSettings();
  await page.getByRole("button", { name: "Clear", exact: true }).click();
  await expect(recorder(page)).toHaveText("Record");
  expect((await app.settings()).globalShortcut).toBeNull();
});

test("Shift alone isn't a shortcut", async ({ app, page }) => {
  await app.start(onWindows());
  await app.openSettings();
  await recorder(page).click();
  await page.keyboard.press("Shift+KeyA");
  await expect(page.getByRole("alert")).toHaveText(
    "Hold a modifier key too; Shift alone isn't enough.",
  );
  // Still listening, nothing saved.
  await expect(recorder(page)).toHaveAttribute("aria-pressed", "true");
  expect(await app.calls("update_settings")).toEqual([]);
});

test("a shortcut another app holds is refused and nothing is saved", async ({ app, page }) => {
  const s = onWindows();
  s.responses.update_settings = [fail("shortcutTaken", "taken (from Rust)"), pass()];
  await app.start(s);
  await app.openSettings();
  await recorder(page).click();
  await page.keyboard.press("Control+Alt+KeyK");
  await expect(page.getByRole("alert")).toHaveText(
    "another app or the system already uses this shortcut",
  );
  expect((await app.settings()).globalShortcut).toBeNull();
  // Still listening: another combination can be tried at once.
  await expect(recorder(page)).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Control+Alt+KeyJ");
  await expect(recorder(page)).toHaveText("Ctrl+Alt+J");
  expect((await app.settings()).globalShortcut).toBe("Control+Alt+KeyJ");
});

test("a saved shortcut another app took since says it isn't active", async ({ app, page }) => {
  const s = onWindows({ globalShortcut: "Control+Alt+KeyC" });
  s.responses.active_global_shortcut = [{ value: null }];
  await app.start(s);
  await app.openSettings();
  await expect(page.getByRole("alert")).toHaveText(
    "Another app or the system uses this shortcut, so it isn't active. Record another one.",
  );
});

test("under Wayland the row is disabled and says why", async ({ app, page }) => {
  await app.start(onWindows({}, false));
  await app.openSettings();
  await expect(recorder(page)).toBeDisabled();
  await expect(page.getByText("Not available under Wayland.")).toBeVisible();
  expect(await app.calls("active_global_shortcut")).toEqual([]);
});

test("macOS writes it with symbols", async ({ app, page }) => {
  await app.start(scenario({ settings: settings({ globalShortcut: "Control+Alt+KeyC" }) }));
  await app.openSettings();
  await expect(recorder(page)).toHaveText("⌃⌥C");
});

test("opened by the shortcut, the keyboard lands on the list", async ({ app, page }) => {
  const lab = project({ slug: "lab1", name: "Lab 1" });
  await app.start(scenario({ snapshot: snapshot({ projects: [lab] }) }));
  await expect(app.row("Lab 1")).toBeVisible();
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await app.emit("popover-shortcut", null);
  await expect(app.row("Lab 1").locator("[data-row]")).toBeFocused();
});
