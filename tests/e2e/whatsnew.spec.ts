// What's new: once after an update (until "Got it"), never instead of the first-run guide,
// again from Settings > About, and never in the way of a link being handled.
import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "./app";
import { scenario, settings, VERSION } from "./fixtures";
import type { DeepLinkTarget } from "../../src/lib/api";
import { compareVersions, pendingVersions } from "../../src/lib/changelog/versions.js";

/** The versions with notes, as `src/lib/changelog` bundles them, newest first. */
const DIR = new URL("../../src/lib/changelog/", import.meta.url);
const NOTES = readdirSync(DIR)
  .filter((file) => /^\d+\.\d+\.\d+.*\.json$/.test(file))
  .map((file) => file.slice(0, -".json".length))
  .sort((a, b) => compareVersions(b, a));
const NEWEST = NOTES[0];
/** The version before the newest, so only the newest notes are unread. */
const BEFORE = NOTES[1] ?? "0.9.0";
const FIRST_ENTRY: string = JSON.parse(readFileSync(new URL(`${NEWEST}.json`, DIR), "utf8")).notes
  .en[0].text;
const at = (version: string) => ({
  os: "macos" as const,
  version,
  nativeCalendar: true,
  globalShortcuts: true,
});

test("after an update the notes show once, and Got it records them as read", async ({
  app,
  page,
}) => {
  await app.start(
    scenario({
      settings: settings({ lastSeenVersion: BEFORE }),
      appInfo: at(NEWEST),
    }),
  );
  await expect(page).toHaveURL(/\/whats-new$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(`What's new in ${NEWEST}`);
  await expect(
    page.getByText("Classroom 50 Toolbox was updated. Here's what changed."),
  ).toBeVisible();
  await expect(page.getByText(FIRST_ENTRY)).toBeVisible();

  await page.getByRole("button", { name: "Got it" }).click();
  await expect(page).toHaveURL(/\/$/);
  expect((await app.settings()).lastSeenVersion).toBe(NEWEST);
  await expect(page.getByRole("searchbox")).toBeVisible();
});

test("a settings file from before the page shows the current notes once", async ({ app, page }) => {
  test.skip(!NOTES.includes(VERSION), `no notes for ${VERSION}`);
  await app.start(scenario({ settings: settings({ lastSeenVersion: null }) }));
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(`What's new in ${VERSION}`);
  await page.getByRole("button", { name: "Got it" }).click();
  expect((await app.settings()).lastSeenVersion).toBe(VERSION);
});

test("several versions show newest first", async ({ app, page }) => {
  // A stable version leaves out its own betas, whose notes it includes.
  const shown = pendingVersions(NOTES, NEWEST, "0.0.1");
  test.skip(shown.length < 2, "only one version has notes so far");
  await app.start(
    scenario({
      settings: settings({ lastSeenVersion: "0.0.1" }),
      appInfo: at(NEWEST),
    }),
  );
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("What's new");
  await expect(page.locator(".version-title h2")).toHaveText(
    shown.map((version) => `Version ${version}`),
  );
});

test("a first run gets the guide, and finishing it marks this version as read", async ({
  app,
  page,
}) => {
  await app.start(scenario({ settings: settings({ onboarded: false, lastSeenVersion: null }) }));
  await expect(page).toHaveURL(/\/welcome$/);
  await page.getByRole("button", { name: "Skip" }).click();
  await expect(page).toHaveURL(/\/$/);
  const saved = await app.settings();
  expect(saved.onboarded).toBe(true);
  expect(saved.lastSeenVersion).toBe(VERSION);
  await expect(page.getByRole("searchbox")).toBeVisible();
});

test("Settings > About shows this version's notes again, and goes back there", async ({
  app,
  page,
}) => {
  test.skip(!NOTES.includes(VERSION), `no notes for ${VERSION}`);
  await app.start(scenario());
  await app.openSettings();
  await page.getByRole("button", { name: "Show", exact: true }).click();
  await expect(page).toHaveURL(/\/whats-new\?from=settings$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(`What's new in ${VERSION}`);
  // Read again on purpose: no "was updated" lead.
  await expect(page.getByText("was updated")).toHaveCount(0);
  await page.getByRole("button", { name: "Got it" }).click();
  await expect(page).toHaveURL(/\/settings$/);
  expect((await app.calls("update_settings")).length).toBe(0);
});

test("a link being handled goes before the notes", async ({ app, page }) => {
  const unlisted: DeepLinkTarget = {
    kind: "unlisted",
    link: { kind: "accept", org: "cs50-school", classroom: "cs101", assignment: "lab9", key: null },
  };
  await app.start(
    scenario({
      settings: settings({ lastSeenVersion: BEFORE }),
      appInfo: at(NEWEST),
      deepLink: unlisted,
    }),
  );
  await app.expectDialog([/Open the assignment lab9/], "cancel");
  await expect(page).not.toHaveURL(/whats-new/);
  // The notes stay unread: they come back once the popover reopens.
  expect((await app.settings()).lastSeenVersion).toBe(BEFORE);
  await app.emit("popover-hidden", null);
  await expect(page).toHaveURL(/\/whats-new$/);
});
