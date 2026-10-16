// The first-run guide: shown until `onboarded`, every step skippable.
import { expect, test } from "./app";
import { scenario, settings, tools } from "./fixtures";

const firstRun = () =>
  scenario({
    settings: settings({ onboarded: false, cloneFolder: null }),
    tools: tools(["gh-student"]),
  });

test("a first run opens the guide, and Skip ends it for good", async ({ app, page }) => {
  await app.start(firstRun());
  await expect(page).toHaveURL(/\/welcome$/);
  await expect(
    page.getByRole("heading", { name: "Welcome to Classroom 50 Toolbox" }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Skip" }).click();
  await expect(page).toHaveURL(/\/$/);
  expect((await app.settings()).onboarded).toBe(true);
  // Signed in already: the list starts filling at once.
  await app.waitForCall("refresh_projects");
  expect(await app.writes()).toEqual([]);
});

test("the guide walks through tools, account and folders", async ({ app, page }) => {
  await app.start(firstRun());
  const next = page.locator("footer .nav .primary");
  await expect(next).toHaveText("Get started");

  await next.click();
  await expect(page.getByText("Install what's missing")).toBeVisible();
  // The missing tool offers Install; the others show their version.
  await expect(page.getByRole("button", { name: "Install" })).toHaveCount(1);
  await expect(page.getByText("2.83.0")).toBeVisible();

  await next.click();
  await expect(page.getByText("GitHub account", { exact: true })).toBeVisible();
  await expect(page.locator(".login .account")).toContainText("student");

  await next.click();
  await expect(page.getByText("Not chosen yet")).toBeVisible();
  // Choosing the clone folder goes through the native picker, then Rust's settings.
  await page.getByRole("button", { name: "Choose" }).click();
  const picker = await app.dialog();
  expect(picker.type).toBe("open");
  await app.answer("/Users/student/Code");
  await expect(page.getByText("/Users/student/Code")).toBeVisible();
  expect((await app.settings()).cloneFolder).toBe("/Users/student/Code");

  await expect(next).toHaveText("Done");
  await page.getByRole("button", { name: "Back" }).click();
  await expect(next).toHaveText("Next");
  await next.click();
  await next.click();
  await expect(page).toHaveURL(/\/$/);
  expect((await app.settings()).onboarded).toBe(true);
  // Nothing was installed: the guide only offered it.
  expect(await app.writes()).toEqual([]);
});

test("an onboarded student never sees the guide", async ({ app, page }) => {
  await app.start(scenario());
  await expect(page.getByRole("searchbox")).toBeVisible();
  await expect(page).toHaveURL(/\/$/);
});
