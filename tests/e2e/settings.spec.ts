// Settings in four tabs: one shown at a time, the others kept mounted and hidden.
import { expect, test } from "./app";
import { scenario } from "./fixtures";

const tab = (app: { page: import("@playwright/test").Page }, name: string) =>
  app.page.getByRole("tab", { name });

test("Settings opens on General, and a tab shows only its sections", async ({ app, page }) => {
  await app.start(scenario());
  await app.openSettings();
  await expect(tab(app, "General")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByText("Appearance")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Tools" })).toBeHidden();

  await tab(app, "Tools").click();
  await expect(page.getByRole("heading", { name: "Tools" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sync" })).toBeVisible();
  await expect(page.getByText("Appearance")).toBeHidden();
  await expect(page.getByRole("tabpanel")).toHaveAttribute("id", "panel-tools");

  await tab(app, "Assignments").click();
  await expect(page.getByRole("heading", { name: "Deadline reminders" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Calendar" })).toBeVisible();

  await tab(app, "About").click();
  await expect(page.getByText(/^Version /)).toBeVisible();
  expect(await app.writes()).toEqual([]);
});

test("arrow keys, Home and End move between the tabs and show each", async ({ app, page }) => {
  await app.start(scenario());
  await app.openSettings();
  await tab(app, "General").focus();
  await page.keyboard.press("ArrowRight");
  await expect(tab(app, "Assignments")).toBeFocused();
  await expect(tab(app, "Assignments")).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("End");
  await expect(tab(app, "About")).toHaveAttribute("aria-selected", "true");
  // Wraps around.
  await page.keyboard.press("ArrowRight");
  await expect(tab(app, "General")).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowLeft");
  await expect(tab(app, "About")).toBeFocused();
  // One Tab stop: the shown tab only.
  await expect(tab(app, "General")).toHaveAttribute("tabindex", "-1");
  await expect(tab(app, "About")).toHaveAttribute("tabindex", "0");
});

test("the last tab shows again when Settings opens again", async ({ app, page }) => {
  await app.start(scenario());
  await app.openSettings("Tools");
  await page.getByRole("button", { name: "Back" }).click();
  await expect(page).toHaveURL(/\/$/);
  await app.openSettings();
  await expect(tab(app, "Tools")).toHaveAttribute("aria-selected", "true");
});
