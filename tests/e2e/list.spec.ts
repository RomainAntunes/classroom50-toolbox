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
  type ProjectOptions,
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

test.describe("due dates", () => {
  // Zurich time: 28 October 2026 at 08:00, a deadline 38.5 hours later is still tomorrow.
  test.use({ timezoneId: "Europe/Zurich" });
  const dates = snapshot({
    projects: [
      project({ slug: "lab1", name: "Lab 1", assignment: { due: "2026-10-29T21:30:00Z" } }),
    ],
    unaccepted: [
      unaccepted({ slug: "lab0", name: "Lab 0", classroom: cs101, due: "2026-10-27T21:30:00Z" }),
    ],
  });

  test("say the calendar day and the time of a deadline", async ({ app, page }) => {
    await page.clock.install({ time: new Date("2026-10-28T07:00:00Z") });
    await app.start(scenario({ snapshot: dates }));

    const due = app.row("Lab 1").getByText(/^Due /);
    await expect(due).toHaveText(/^Due tomorrow at 10:30\sPM$/);
    await expect(due).toHaveAttribute("title", /Oct 29/);
    const was = app.row("Lab 0").getByText(/^Was due /);
    await expect(was).toHaveText(/^Was due yesterday at 10:30\sPM$/);
    await expect(was).toHaveAttribute("title", /Oct 27/);
  });

  test("in the UI's language", async ({ app, page }) => {
    await page.clock.install({ time: new Date("2026-10-28T07:00:00Z") });
    await app.start(scenario({ snapshot: dates, settings: settings({ language: "fr" }) }));
    await expect(app.row("Lab 1").getByText("À rendre demain à 22:30")).toBeVisible();
    await expect(app.row("Lab 0").getByText("Était à rendre hier à 22:30")).toBeVisible();
  });
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

// A row's status names what stands between the work and its grade, and only warns when the
// student has something to do.
const single = (options: Omit<ProjectOptions, "slug" | "name">) =>
  snapshot({ projects: [project({ slug: "lab", name: "Lab", ...options })] });
const EVERY_PUSH = { submissionMode: "every-push" } as const;
const EDITED = { changedFiles: 2, changedPaths: ["notes.md", "out/"] };

test("an every-push lab whose last push is the clone's HEAD only has local changes", async ({
  app,
}) => {
  await app.start(
    scenario({
      snapshot: single({ assignment: EVERY_PUSH, pending: "uncommitted", local: EDITED }),
    }),
  );
  const status = app.row("Lab").locator(".status");
  await expect(status).toHaveText("Local changes");
  await expect(status).not.toHaveClass(/warn/);
  await expect(status).toHaveAttribute(
    "title",
    "Your last push is on GitHub and counts. These changes are only on this computer and not committed yet:\nnotes.md\nout/",
  );
  // The row's button describes itself with the same words.
  await expect(app.row("Lab").locator("[data-row]")).toHaveAccessibleDescription(/Local changes/);
  expect(await app.writes()).toEqual([]);
});

for (const [why, options] of [
  ["tag mode", { pending: "uncommitted", local: EDITED }],
  [
    "GitHub at another commit",
    {
      assignment: EVERY_PUSH,
      pending: "uncommitted",
      local: EDITED,
      remote: { head: { oid: "ffff0000", at: hoursFromNow(-1), author: "student" } },
    },
  ],
  [
    "commits not pushed",
    { assignment: EVERY_PUSH, pending: "uncommitted", local: { ...EDITED, ahead: 1 } },
  ],
] as const) {
  test(`changes still warn Uncommitted with ${why}`, async ({ app }) => {
    await app.start(scenario({ snapshot: single(options) }));
    const status = app.row("Lab").locator(".status");
    await expect(status).toHaveText("Uncommitted");
    await expect(status).toHaveClass(/warn/);
  });
}

const conflicts: [string, ProjectOptions["local"], string, boolean][] = [
  [
    "Commit, then pull",
    { remoteAhead: true, changedFiles: 2, trackedChanges: 1, changedPaths: ["rapport.md", "img/"] },
    "A teammate pushed while you have changes that aren't committed yet. Commit them first",
    false,
  ],
  [
    "Pull and merge",
    { behind: 1, ahead: 2 },
    "A teammate pushed while you have commits that aren't on GitHub yet",
    false,
  ],
  [
    "Pull first",
    { remoteAhead: true, changedFiles: 1, changedPaths: ["img/"] },
    "A teammate pushed while you have uncommitted or unpushed work. Pull now",
    true,
  ],
];
for (const [label, local, hint, button] of conflicts) {
  test(`a teammate's push over local work says ${label}`, async ({ app }) => {
    await app.start(
      scenario({ snapshot: single({ assignment: EVERY_PUSH, pending: "conflict", local }) }),
    );
    const status = app.row("Lab").locator(".status");
    await expect(status).toHaveText(label);
    await expect(status).toHaveClass(/warn/);
    await expect(status).toHaveAttribute("title", new RegExp(`^${hint}`));
    // The Pull button only shows when the app's Pull can fast-forward: untracked files
    // don't stop it, tracked changes and local commits do.
    await expect(app.row("Lab").getByRole("button", { name: "Pull" })).toHaveCount(button ? 1 : 0);
    expect(await app.writes()).toEqual([]);
  });
}

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

test("the synced label ticks while the popover is open and stops while it is hidden", async ({
  app,
  page,
}) => {
  await page.clock.install({ time: new Date() });
  const at = new Date().toISOString();
  await app.start(scenario({ snapshot: snapshot({ ...list, syncedAt: at, checkedAt: at }) }));
  await expect(page.getByText("Synced just now")).toBeVisible();
  await page.clock.runFor(2 * 60_000);
  await expect(page.getByText("Synced 2 minutes ago")).toBeVisible();

  // Hidden, nothing re-renders: the label keeps the last minute it showed.
  await app.emit("popover-hidden", null);
  await page.clock.runFor(5 * 60_000);
  await expect(page.getByText("Synced 2 minutes ago")).toBeVisible();

  await app.emit("popover-shown", null);
  await expect(page.getByText("Synced 7 minutes ago")).toBeVisible();
  await page.clock.runFor(60_000);
  await expect(page.getByText("Synced 8 minutes ago")).toBeVisible();
});

test("a list that mounts hidden doesn't tick until the popover shows", async ({ app, page }) => {
  await page.clock.install({ time: new Date() });
  const at = new Date().toISOString();
  await app.start(
    scenario({
      snapshot: snapshot({ ...list, syncedAt: at, checkedAt: at }),
      responses: { "plugin:window|is_visible": [ok(false)] },
    }),
  );
  await expect(page.getByText("Synced just now")).toBeVisible();
  await page.clock.runFor(3 * 60_000);
  await expect(page.getByText("Synced just now")).toBeVisible();

  await app.emit("popover-shown", null);
  await expect(page.getByText("Synced 3 minutes ago")).toBeVisible();
});
