// The main list: classes, rows, statuses, grades, unaccepted assignments, search and hiding.
import { expect, test } from "./app";
import {
  classroom,
  grade,
  gradeTest,
  hold,
  hoursFromNow,
  ok,
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

// What the runner (cli 1.58.0 on) keeps of a test it stopped at its time limit: the input it
// was fed, then what it flushed before the kill.
const STOPPED_OUTPUT =
  "--- input ---\n-3\n\n--- actual stdout ---\nHeight: Height: \n--- stderr ---\nheight must be positive";

const graded = snapshot({
  projects: [
    project({
      slug: "mario",
      name: "Mario",
      remote: {
        grade: grade(1, 7, {
          testsPassed: 1,
          testsTotal: 5,
          tests: [
            gradeTest("compiles", { passed: true, score: 1 }),
            gradeTest("rejects a negative height", {
              maxScore: 2,
              output: STOPPED_OUTPUT,
              timedOutAfter: 10,
            }),
            // An older runner wrote the timeout line alone.
            gradeTest("waits for input", { timedOutAfter: 1 }),
            gradeTest("builds a pyramid", { maxScore: 2, output: "-#\n+##" }),
            gradeTest("style"),
          ],
          releaseNotes: { notes: null, notesClipped: false },
        }),
      },
    }),
  ],
});

test("a test the autograder stopped says so before what it printed", async ({ app, page }) => {
  await app.start(scenario({ snapshot: graded }));
  const mario = app.row("Mario");
  await mario.locator("[data-row]").click();
  await expect(mario.locator(".details .heading")).toHaveText("Failed tests");
  const testButton = (name: string) => mario.locator("button.test", { hasText: name });
  const failedTest = (name: string) =>
    mario.locator(".tests li", { has: page.locator("button.test", { hasText: name }) });

  await testButton("rejects a negative height").click();
  await expect(testButton("rejects a negative height")).toHaveAttribute("aria-expanded", "true");
  const stopped = failedTest("rejects a negative height");
  await expect(stopped.locator(".timeout")).toHaveText(
    "Stopped after 10 seconds: the program was still running when time ran out.",
  );
  await expect(stopped.locator(".hint")).toHaveText(
    "What it printed but hadn't flushed yet is lost, so a prompt may be missing.",
  );
  const output = stopped.locator(".output pre");
  await expect(output).toHaveAttribute("dir", "ltr");
  await expect(output).toContainText("--- actual stdout ---");
  await expect(output).toContainText("height must be positive");
  await expect(stopped).not.toContainText("timed out after");

  // The old one-line format opens on the same line and hint, with no output under them.
  await testButton("waits for input").click();
  const old = failedTest("waits for input");
  await expect(old.locator(".timeout")).toHaveText(
    "Stopped after 1 second: the program was still running when time ran out.",
  );
  await expect(old.locator(".hint")).toBeVisible();
  await expect(old.locator("pre")).toHaveCount(0);

  // Any other failure looks as before.
  await testButton("builds a pyramid").click();
  const pyramid = failedTest("builds a pyramid");
  await expect(pyramid.locator("pre")).toHaveText("-#\n+##");
  await expect(pyramid.locator(".timeout, .hint")).toHaveCount(0);
  await expect(mario.locator("div.test", { hasText: "style" })).toBeVisible();
  await expect(testButton("style")).toHaveCount(0);

  // Reading a grade is read-only.
  expect(await app.writes()).toEqual([]);
  await expect(page.locator(".timeout")).toHaveCount(2);
});

test("Clear cache spins until the list is rebuilt", async ({ app, page }) => {
  await app.start(scenario({ responses: { clear_cache: [hold()] } }));
  await app.openSettings();
  const clear = page.getByRole("button", { name: "Clear cache" });
  await clear.click();
  await expect(clear).toHaveAttribute("aria-busy", "true");
  await expect(clear.locator(".spinner")).toBeVisible();
  await app.release("clear_cache", ok(snapshot()));
  await expect(clear).toHaveAttribute("aria-busy", "false");
  await expect(clear).toBeEnabled();
});
