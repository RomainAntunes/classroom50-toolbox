// What the teacher published about grading, read from the class's `assignments.json`: an
// assignment that isn't graded says so instead of a grade, a manual one names its points
// and where the teaching staff's feedback comes (the mark itself isn't shown to students),
// a grade says whether it reaches the passing bar, and a group shows its size against the
// assignment's limit, the full group's "Add to my group…" saying so before asking anyone.
import { expect, test, type App } from "./app";
import { LOGIN, grade, project, scenario, snapshot, unaccepted } from "./fixtures";
import type { Group, Project } from "../../src/lib/api";

async function details(app: App, row: Project) {
  await app.start(scenario({ snapshot: snapshot({ projects: [row] }) }));
  const item = app.row(row.assignment!.name);
  await item.locator("[data-row]").click();
  return item;
}

/** The value of the details' line labelled `label`. */
const value = (item: ReturnType<App["row"]>, label: string) =>
  item.locator(".details dt", { hasText: label }).locator("xpath=following-sibling::dd[1]");

test("an assignment that isn't graded says so, with no grading news", async ({ app }) => {
  const row = project({
    slug: "lab1",
    name: "Lab 1",
    assignment: { grading: { mode: "off", maxPoints: null } },
    // A grading run the teacher didn't ask for: not worth a word.
    remote: { grading: { state: "running", url: null } },
  });
  const item = await details(app, row);
  await expect(item.locator(".grade")).toHaveText("Not graded");
  await expect(item.locator(".grade")).toHaveAttribute(
    "title",
    "Your teacher doesn't grade this assignment.",
  );
  await expect(item.getByText("Grading…")).toHaveCount(0);
  await expect(value(item, "Grading")).toHaveText("Not graded");
  await expect(item.locator(".details dt", { hasText: /^Grade$/ })).toHaveCount(0);
  // The calendar notes say it too, in the UI's words.
  const [labels] = await app.waitForCall("set_calendar_labels");
  expect((labels.args as { labels: Record<string, string> }).labels.notGraded).toBe("Not graded");
});

test("a grade the autograder left on an ungraded assignment still shows", async ({ app }) => {
  const row = project({
    slug: "lab1",
    name: "Lab 1",
    assignment: { grading: { mode: "off", maxPoints: null } },
    remote: { grade: grade(8, 10) },
  });
  const item = await details(app, row);
  await expect(item.locator(".grade")).toContainText("8/10");
  await expect(value(item, "Grading")).toHaveText("Not graded");
});

for (const [feedbackPr, pointer] of [
  [true, "Comments from your teaching staff come in the Feedback pull request."],
  [false, "Check the assignment's page on Classroom 50, or ask your teaching staff."],
] as const) {
  test(`a manual grading names its points and ${feedbackPr ? "the Feedback pull request" : "the web page"}`, async ({
    app,
  }) => {
    const row = project({
      slug: "lab2",
      name: "Lab 2",
      assignment: { grading: { mode: "manual", maxPoints: 50 }, feedbackPr },
    });
    const item = await details(app, row);
    const grading = value(item, "Grading");
    await expect(grading).toContainText("By your teaching staff, out of 50 points");
    await expect(grading).toContainText("Classroom 50 doesn't show this mark to students yet.");
    await expect(grading).toContainText(pointer);
    await expect(item.locator(".grade")).toHaveCount(0);
  });
}

test("an unaccepted assignment says how it is graded too", async ({ app }) => {
  const item = unaccepted({
    slug: "lab3",
    name: "Lab 3",
    grading: { mode: "manual", maxPoints: 1 },
  });
  await app.start(scenario({ snapshot: snapshot({ projects: [], unaccepted: [item] }) }));
  const row = app.row("Lab 3");
  await row.locator("[data-row]").click();
  await expect(value(row, "Grading")).toContainText("By your teaching staff, out of 1 point");
});

for (const [score, label, warn] of [
  [7, "Passing", false],
  [5, "Below the passing bar (60%)", true],
] as const) {
  test(`a grade of ${score}/10 reads "${label}" against a 60% bar`, async ({ app }) => {
    const row = project({
      slug: "lab4",
      name: "Lab 4",
      assignment: { passThreshold: 60 },
      remote: { grade: grade(score, 10) },
    });
    const item = await details(app, row);
    const badge = item.locator(".grade");
    // In words for screen readers and the hover title, not by color alone.
    await expect(badge.locator(".sr-only")).toHaveText(`Grade: ${score} out of 10, ${label}`);
    await expect(badge).toHaveAttribute(
      "title",
      new RegExp(`· ${label.replace(/[()]/g, "\\$&")}$`),
    );
    if (warn) await expect(badge).toHaveClass(/\bbelow\b/);
    else await expect(badge).not.toHaveClass(/\bbelow\b/);
    await expect(value(item, "Grade")).toContainText(`${score}/10 · `);
    await expect(value(item, "Grade")).toContainText(label);
  });
}

test("no passing bar, no verdict", async ({ app }) => {
  const row = project({ slug: "lab4", name: "Lab 4", remote: { grade: grade(0, 10) } });
  const item = await details(app, row);
  await expect(item.locator(".grade .sr-only")).toHaveText("Grade: 0 out of 10");
  await expect(value(item, "Grade")).not.toContainText("Passing");
  await expect(value(item, "Grade")).not.toContainText("passing bar");
});

function group(logins: string[]): Group {
  return {
    org: "cs50-school",
    slug: "classroom50-group-abc123-4",
    number: 4,
    name: "Team Rocket",
    members: logins.map((login) => ({ login, name: null, email: null })),
    founders: [LOGIN],
    url: "https://github.com/orgs/cs50-school/teams/classroom50-group-abc123-4",
    classroom: null,
    assignment: null,
  };
}

const teamProject = (logins: string[], maxGroupSize: number | null) =>
  project({
    slug: "proj",
    name: "Project",
    assignment: { mode: "team", teamFormation: "student", maxGroupSize },
    group: group(logins),
  });

test("a group shows its size against the assignment's limit", async ({ app }) => {
  const item = await details(app, teamProject([LOGIN, "ada"], 3));
  await expect(value(item, "Size")).toHaveText("2 of 3 members");
});

test("a group past the limit is flagged", async ({ app }) => {
  const item = await details(app, teamProject([LOGIN, "ada", "bob"], 2));
  await expect(value(item, "Size")).toHaveText(
    "3 of 2 members\nMore members than this assignment allows",
  );
});

test("a full group says so before asking for anyone", async ({ app }) => {
  const row = teamProject([LOGIN, "ada"], 2);
  await app.start(scenario({ snapshot: snapshot({ projects: [row] }) }));
  await app.rowMenu("Project");
  await app.pickMenu("Add to my group…");
  const dialog = await app.dialog();
  expect(dialog.title).toBe("Add to my group");
  expect(dialog.kind).toBe("info");
  expect(dialog.message).toBe("Your group is full: this assignment allows 2 members at most.");
  await app.answer("ok");
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
});

test("a group with room left asks for usernames", async ({ app }) => {
  const row = teamProject([LOGIN], 2);
  await app.start(scenario({ snapshot: snapshot({ projects: [row] }) }));
  await app.rowMenu("Project");
  await app.pickMenu("Add to my group…");
  await expect(app.prompt.getByRole("heading")).toHaveText("Add to my group");
  await app.prompt.getByRole("button", { name: "Cancel" }).click();
  expect(await app.writes()).toEqual([]);
});

test("Rust's refusal past the limit is worded on the row", async ({ app }) => {
  const row = teamProject([LOGIN], 2);
  await app.start(scenario({ snapshot: snapshot({ projects: [row] }) }));
  await app.waitForCall("teammate_jobs");
  await expect(app.page.getByRole("button", { name: "Project" })).toBeVisible();
  await app.emit("teammate-jobs", [
    {
      id: row.id,
      project: row.id,
      repo: `${row.owner}/${row.repo}`,
      assignment: "Project",
      team: true,
      running: false,
      entries: [
        { username: "ada", state: "added", error: null },
        {
          username: "bob",
          state: "failed",
          error: {
            kind: "groupFull",
            message: "your group is full (maximum: 2)",
            params: { max: "2" },
          },
        },
      ],
    },
  ]);
  await expect(app.row("Project")).toContainText(
    "Couldn't add bob: your group is full (maximum: 2)",
  );
});
