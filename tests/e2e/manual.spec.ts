// Projects the student adds by hand: work Classroom 50 doesn't track. Added from a class's
// "..." menu (its class filled in) or the empty list, edited and removed from the row's menu,
// saved through `save_manual_project`. Their rows have no Classroom 50 actions, and their
// due date shows on the row and among the tray's deadlines.
import { expect, menuTexts, test, type App } from "./app";
import { HOME, fail, manualProject, pass, project, scenario, snapshot } from "./fixtures";
import type { Project, TrayMenu } from "../../src/lib/api";
import type { Reply } from "./fixtures";

// Zurich time: 28 October 2026, 3 PM.
test.use({ timezoneId: "Europe/Zurich" });
test.beforeEach(async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-28T14:00:00Z") });
});

// ICU puts a narrow no-break space before "PM".
const plain = (text: string | null) => (text ?? "").replace(/\s/g, " ");

const lab = project({ slug: "lab-1", name: "Lab 1", assignment: { due: "2026-11-10T22:59:00Z" } });
const essay = manualProject("1", {
  name: "Essay",
  class: "Writing",
  due: "2026-10-29T17:00:00Z",
  notes: "Two pages, no more.",
  folder: "School/essay",
  repo: "student/essay",
});

async function start(app: App, projects: Project[], responses: Record<string, Reply[]> = {}) {
  await app.start(scenario({ snapshot: snapshot({ projects }), responses }));
}

test("a project is added from a class's menu, with its class filled in", async ({ app }) => {
  await start(app, [lab]);
  await app.page.getByRole("button", { name: "More for Intro to CS" }).click();
  await app.pickMenu("Add project…");
  await expect(app.prompt.getByRole("heading")).toHaveText("Add a project");
  await expect(app.prompt.getByLabel("Class")).toHaveValue("Intro to CS");
  const add = app.prompt.getByRole("button", { name: "Add", exact: true });
  // A name first.
  await expect(add).toBeDisabled();
  await expect(app.prompt.getByLabel("Name")).toBeFocused();
  await app.page.keyboard.type("Reading notes");

  await app.prompt.getByLabel("Due date").check();
  // Dated a week from today, so picking a day keeps the month and year.
  const date = app.prompt.getByLabel("Day");
  await expect(date).toHaveValue("2026-11-04");
  await date.fill("");
  await expect(add).toBeDisabled();
  await expect(date).toHaveAttribute("aria-invalid", "true");
  await expect(app.prompt).toContainText("Complete the due date: day, month and year.");
  await date.fill("2026-10-30");
  await app.prompt.getByLabel("Time").fill("12:00");

  await app.prompt.getByRole("button", { name: "Choose…" }).click();
  await app.answer(`${HOME}/School/reading`);
  await expect(app.prompt).toContainText("~/School/reading");

  const repo = app.prompt.getByLabel("GitHub repository");
  await repo.fill("not a repo");
  await expect(add).toBeDisabled();
  await expect(app.prompt).toContainText("Write the repository as owner/repo");
  await repo.fill("https://github.com/student/reading.git");
  await app.prompt.getByLabel("Notes").fill("Chapters 1 to 3");
  await add.click();

  const [call] = await app.waitForCall("save_manual_project");
  expect(call.args.project).toEqual({
    id: null,
    name: "Reading notes",
    class: "Intro to CS",
    due: "2026-10-30T11:00:00.000Z",
    notes: "Chapters 1 to 3",
    done: false,
    path: `${HOME}/School/reading`,
    repo: "student/reading",
  });
  await expect(app.prompt).toHaveCount(0);
  // Under the class it was added from, with its due date.
  const section = app.page.locator("section").filter({ hasText: "Intro to CS" });
  await expect(section.locator("li[data-item]")).toHaveCount(2);
  await expect(app.row("Reading notes").locator(".meta")).toContainText(
    /Due in 2 days at 12:00\sPM/,
  );
  expect(await app.writes()).toEqual(["save_manual_project"]);
});

test("the empty list offers to add one, without a class", async ({ app }) => {
  await start(app, []);
  await app.page.getByRole("button", { name: "Add project" }).click();
  await app.prompt.getByLabel("Name").fill("Side project");
  await app.prompt.getByRole("button", { name: "Add", exact: true }).click();
  const [call] = await app.waitForCall("save_manual_project");
  expect(call.args.project).toMatchObject({ name: "Side project", class: null, due: null });
  // A project with no class is filed under "My projects".
  await expect(app.page.locator("section").filter({ hasText: "My projects" })).toHaveCount(1);
  await expect(app.row("Side project").locator(".meta")).toContainText("Added by you");
});

test("cancelling writes nothing", async ({ app }) => {
  await start(app, [lab]);
  await app.page.getByRole("button", { name: "More for Intro to CS" }).click();
  await app.pickMenu("Add project…");
  await app.prompt.getByLabel("Name").fill("Never saved");
  await app.prompt.getByRole("button", { name: "Cancel" }).click();
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
});

test("a refusal shows in the form with the edits kept", async ({ app }) => {
  await start(app, [lab], {
    save_manual_project: [
      fail(
        "alreadyListed",
        "this folder or repository is already in the list as a Classroom 50 assignment",
      ),
      pass(),
    ],
  });
  await app.page.getByRole("button", { name: "More for Intro to CS" }).click();
  await app.pickMenu("Add project…");
  await app.prompt.getByLabel("Name").fill("Copy of Lab 1");
  await app.prompt.getByRole("button", { name: "Add", exact: true }).click();
  await expect(app.prompt.getByRole("alert")).toHaveText(
    "this folder or repository is already in the list as a Classroom 50 assignment",
  );
  await expect(app.prompt.getByLabel("Name")).toHaveValue("Copy of Lab 1");
  await app.prompt.getByRole("button", { name: "Add", exact: true }).click();
  await expect(app.prompt).toHaveCount(0);
});

test("a manual row has no Classroom 50 actions, and shows its notes", async ({ app }) => {
  await start(app, [essay]);
  const row = app.row("Essay");
  await expect(row.locator(".meta")).toContainText(/Due tomorrow at 6:00\sPM/);
  // The repo's name, as on a Classroom 50 row.
  await expect(row.locator(".meta")).toContainText("essay");
  // Under its own class.
  await expect(app.page.locator("section").filter({ hasText: "Writing" })).toHaveCount(1);
  const texts = menuTexts(await app.rowMenu("Essay"));
  for (const absent of [
    "Open in Classroom 50",
    "Submit",
    "Add a classmate…",
    "Add to my group…",
    "Clone",
    "App link",
  ]) {
    expect(texts).not.toContain(absent);
  }
  expect(texts).toEqual(expect.arrayContaining(["Edit project…", "Remove project…", "Phases…"]));
  await app.page.keyboard.press("Escape");

  await row.locator("[data-row]").click();
  await expect(row).toContainText("Two pages, no more.");
});

test("a manual project is edited and ticked done", async ({ app }) => {
  await start(app, [essay]);
  await app.rowMenu("Essay");
  await app.pickMenu("manual-edit");
  await expect(app.prompt.getByRole("heading")).toHaveText("Edit project");
  await expect(app.prompt.getByLabel("Name")).toHaveValue("Essay");
  await expect(app.prompt.getByLabel("Class")).toHaveValue("Writing");
  await expect(app.prompt.getByLabel("Day")).toHaveValue("2026-10-29");
  await expect(app.prompt.getByLabel("Time")).toHaveValue("18:00");
  await expect(app.prompt.getByLabel("Notes")).toHaveValue("Two pages, no more.");
  await app.prompt.getByLabel("Done").check();
  await app.prompt.getByRole("button", { name: "Save" }).click();
  const [call] = await app.waitForCall("save_manual_project");
  expect(call.args.project).toMatchObject({
    id: essay.id,
    name: "Essay",
    done: true,
    due: "2026-10-29T17:00:00.000Z",
    path: `${HOME}/School/essay`,
    repo: "student/essay",
  });
  await expect(app.row("Essay").locator(".meta")).toContainText("Done");
});

test("removing asks first and leaves the folder alone", async ({ app }) => {
  await start(app, [essay, lab]);
  await app.rowMenu("Essay");
  await app.pickMenu("manual-remove");
  await app.expectDialog(["Its folder stays as it is."], "cancel");
  expect(await app.writes()).toEqual([]);

  await app.rowMenu("Essay");
  await app.pickMenu("manual-remove");
  await app.expectDialog(["Its folder stays as it is."], "ok");
  const [call] = await app.waitForCall("remove_manual_project");
  expect(call.args.id).toBe(essay.id);
  await expect(app.row("Essay")).toHaveCount(0);
});

test("its due date is a tray deadline", async ({ app }) => {
  await start(app, [essay]);
  const menus = async () =>
    (await app.calls("set_tray_menu")).map((call) => call.args.menu as TrayMenu);
  await expect
    .poll(async () =>
      (await menus()).at(-1)?.deadlines.map((d) => ({ ...d, label: plain(d.label) })),
    )
    .toEqual([{ id: essay.id, label: "Essay · Due tomorrow at 6:00 PM" }]);
});
