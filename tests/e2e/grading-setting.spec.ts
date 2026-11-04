// Settings > General's "Show how assignments are graded": off, the rows stop saying "Not
// graded" or "By your teaching staff" (for classes that don't use Classroom 50's grading),
// while grades, the passing bar and grading news stay. A row whose grading the teacher
// changed since (`grading_revealed`, from Rust) or that has a grade says it again.
import { expect, test, type App } from "./app";
import { grade, project, scenario, settings, snapshot, unaccepted } from "./fixtures";

const ungraded = project({
  slug: "lab1",
  name: "Lab 1",
  assignment: { grading: { mode: "off", maxPoints: null } },
});
const manual = project({
  slug: "lab2",
  name: "Lab 2",
  assignment: { grading: { mode: "manual", maxPoints: 50 } },
});
const upcoming = unaccepted({
  slug: "lab3",
  name: "Lab 3",
  grading: { mode: "manual", maxPoints: 10 },
});
const autograded = project({
  slug: "lab4",
  name: "Lab 4",
  assignment: { passThreshold: 60 },
  remote: { grade: grade(5, 10) },
});
const list = snapshot({ projects: [ungraded, manual, autograded], unaccepted: [upcoming] });

/** The value of the details' line labelled `label`. */
const value = (item: ReturnType<App["row"]>, label: string) =>
  item.locator(".details dt", { hasText: label }).locator("xpath=following-sibling::dd[1]");

async function open(app: App, name: string) {
  const item = app.row(name);
  await item.locator("[data-row]").click();
  await expect(item.locator(".details")).toBeVisible();
  return item;
}

/** Nothing on the row nor in its details says how it is graded. */
async function silent(app: App, name: string) {
  const item = await open(app, name);
  await expect(item.locator(".details dt", { hasText: /^Grading$/ })).toHaveCount(0);
  await expect(item.getByText("Not graded")).toHaveCount(0);
  await expect(item.getByText(/teaching staff/)).toHaveCount(0);
}

test("turning it off hides how assignments are graded and saves the setting", async ({ app }) => {
  await app.start(scenario({ snapshot: list }));
  await expect(app.row("Lab 1").locator(".grade")).toHaveText("Not graded");
  await app.openSettings();
  const toggle = app.page.getByRole("switch", { name: /Show how assignments are graded/ });
  await expect(toggle).toBeChecked();
  await expect(app.page.getByText(/doesn't use Classroom 50's grading/)).toBeVisible();
  await toggle.click();
  await expect.poll(async () => (await app.settings()).showGradingInfo).toBe(false);
  const [saved] = await app.calls("update_settings");
  expect((saved.args as { next: { showGradingInfo: boolean } }).next.showGradingInfo).toBe(false);
  expect(await app.writes()).toEqual([]);

  await app.page.getByRole("button", { name: "Back" }).click();
  await expect(app.row("Lab 1")).toBeVisible();
  // Not on the row, in screen-reader text or a hover title either.
  await expect(app.row("Lab 1").locator(".grade")).toHaveCount(0);
  await expect(app.row("Lab 1").locator("[title*='grade']")).toHaveCount(0);
  for (const name of ["Lab 1", "Lab 2", "Lab 3"]) await silent(app, name);
  // Grades and the passing bar aren't how an assignment is graded: they stay.
  const graded = await open(app, "Lab 4");
  await expect(graded.locator(".grade")).toContainText("5/10");
  await expect(graded.locator(".grade .sr-only")).toHaveText(
    "Grade: 5 out of 10, Below the passing bar (60%)",
  );
  await expect(value(graded, "Grade")).toContainText("Below the passing bar (60%)");
});

test("a row whose grading the teacher changed says it again, with the dot", async ({ app }) => {
  await app.start(
    scenario({
      settings: settings({ showGradingInfo: false }),
      snapshot: list,
      unseen: [manual.id, upcoming.id],
      gradingRevealed: [manual.id, upcoming.id],
    }),
  );
  const revealed = await open(app, "Lab 2");
  await expect(revealed.locator(".dot")).toBeVisible();
  await expect(value(revealed, "Grading")).toContainText(
    "By your teaching staff, out of 50 points",
  );
  const item = await open(app, "Lab 3");
  await expect(item.locator(".dot")).toBeVisible();
  await expect(value(item, "Grading")).toContainText("out of 10 points");
  // The others stay quiet.
  await silent(app, "Lab 1");
  await expect(app.row("Lab 1").locator(".dot")).toHaveCount(0);

  // Changing the setting starts over: what was revealed hides with the rest.
  await app.openSettings();
  const toggle = app.page.getByRole("switch", { name: /Show how assignments are graded/ });
  await toggle.click();
  await expect(toggle).toBeChecked();
  await toggle.click();
  await expect(toggle).not.toBeChecked();
  await app.page.getByRole("button", { name: "Back" }).click();
  await silent(app, "Lab 2");
  await silent(app, "Lab 3");
});

test("an ungraded assignment with a grade still says it isn't graded", async ({ app }) => {
  const leftover = project({
    slug: "lab5",
    name: "Lab 5",
    assignment: { grading: { mode: "off", maxPoints: null } },
    remote: { grade: grade(8, 10) },
  });
  await app.start(
    scenario({
      settings: settings({ showGradingInfo: false }),
      snapshot: snapshot({ projects: [leftover] }),
    }),
  );
  const item = await open(app, "Lab 5");
  await expect(item.locator(".grade")).toContainText("8/10");
  await expect(value(item, "Grading")).toHaveText("Not graded");
});
