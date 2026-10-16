// The main list: classes, rows, statuses, grades, unaccepted assignments, search and hiding.
import { expect, test } from "./app";
import {
  classroom,
  grade,
  hoursFromNow,
  project,
  scenario,
  settings,
  snapshot,
  unaccepted,
} from "./fixtures";

const cs101 = classroom();
const art = classroom({ org: "art-school", slug: "art200", name: "Drawing" });

const list = snapshot({
  projects: [
    project({
      slug: "lab1",
      name: "Lab 1",
      assignment: { due: hoursFromNow(-30) },
      lateSeconds: 2 * 3600,
      remote: { grade: grade(8, 10) },
    }),
    project({
      slug: "lab2",
      name: "Lab 2",
      pending: "uncommitted",
      local: { changedFiles: 2, changedPaths: ["a.c", "b.c"] },
    }),
    project({ slug: "sketch", name: "Sketchbook", classroom: art }),
  ],
  unaccepted: [unaccepted({ slug: "lab3", name: "Lab 3", classroom: cs101 })],
});

test("renders classes, rows, the late badge, the grade and an unaccepted assignment", async ({
  app,
  page,
}) => {
  await app.start(scenario({ snapshot: list }));

  const sections = page.locator("section h2 .label");
  await expect(sections).toHaveText(["Intro to CS", "Drawing"]);

  const lab1 = app.row("Lab 1");
  await expect(lab1.locator(".late")).toHaveText(/late/i);
  await expect(lab1.locator(".grade")).toContainText("8/10");
  await expect(app.row("Lab 2").locator(".status")).toHaveText("Uncommitted");
  await expect(app.row("Lab 3")).toContainText("Not accepted");
  await expect(app.row("Lab 3").getByRole("button", { name: "Accept" })).toBeVisible();
  await expect(app.row("Sketchbook")).toBeVisible();

  // Loading the list is read-only.
  expect(await app.writes()).toEqual([]);
});

test("search filters rows across classes", async ({ app, page }) => {
  await app.start(scenario({ snapshot: list }));
  await expect(app.row("Lab 1")).toBeVisible();

  await page.getByRole("searchbox").fill("sketch");
  await expect(app.row("Sketchbook")).toBeVisible();
  await expect(app.row("Lab 1")).toHaveCount(0);
  await expect(page.locator("section h2 .label")).toHaveText(["Drawing"]);

  await page.getByRole("searchbox").fill("nothing like this");
  await expect(page.getByText("No assignment matches")).toBeVisible();
});

test("hidden rows only show on a search, marked hidden", async ({ app, page }) => {
  const hiddenId = list.projects[0].id;
  await app.start(scenario({ snapshot: list, settings: settings({ hidden: [hiddenId] }) }));
  await expect(app.row("Lab 2")).toBeVisible();
  await expect(app.row("Lab 1")).toHaveCount(0);

  await page.getByRole("searchbox").fill("lab 1");
  await expect(app.row("Lab 1")).toBeVisible();
  await expect(app.row("Lab 1").locator(".hidden-tag")).toHaveText("Hidden");
});

test("hiding a row from its menu saves it in settings", async ({ app }) => {
  await app.start(scenario({ snapshot: list }));
  await app.rowMenu("Sketchbook");
  await app.pickMenu("Hide");
  await expect(app.row("Sketchbook")).toHaveCount(0);
  expect((await app.settings()).hidden).toEqual([list.projects[2].id]);
});

test("the date filter is saved through Rust", async ({ app, page }) => {
  await app.start(scenario({ snapshot: list }));
  await page.getByRole("radio", { name: "Past" }).click();
  await expect(app.row("Lab 1")).toBeVisible();
  await expect(app.row("Lab 2")).toHaveCount(0);
  expect((await app.settings()).listFilter).toBe("past");
});
