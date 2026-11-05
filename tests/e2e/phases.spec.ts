// Phases a student sets on a project from its menu: an in-popover editor (name, date and
// time, Done) saved at once through `set_phases`. Without a due date of its own, the row
// shows the next open phase; the tray lists open phases with the deadlines.
import { expect, test, type App } from "./app";
import { fail, pass, project, scenario, snapshot } from "./fixtures";
import type { Project, ProjectPhase, TrayMenu } from "../../src/lib/api";
import type { Reply } from "./fixtures";

// Zurich time: 28 October 2026, 3 PM.
test.use({ timezoneId: "Europe/Zurich" });
test.beforeEach(async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-28T14:00:00Z") });
});

// ICU puts a narrow no-break space before "PM".
const plain = (text: string | null) => (text ?? "").replace(/\s/g, " ");

const phase = (id: string, name: string, due: string, done = false): ProjectPhase => ({
  id,
  name,
  due,
  done,
});

// No due date of its own, as a project the teacher gives phases for out of band.
const bdr = project({ slug: "bdr", name: "BDR project", assignment: { due: null } });
const withPhases = project({
  slug: "bdr",
  name: "BDR project",
  assignment: { due: null },
  phases: [
    phase("p1", "Schema", "2026-10-27T22:59:00Z", true),
    phase("p2", "Queries", "2026-10-29T17:00:00Z"),
  ],
});

async function open(app: App, row: Project, responses: Record<string, Reply[]> = {}) {
  await app.start(scenario({ snapshot: snapshot({ projects: [row] }), responses }));
  await app.rowMenu("BDR project");
  await app.pickMenu("Phases…");
  await expect(app.prompt.getByRole("heading")).toHaveText("Phases of BDR project");
}

test("phases are added, ticked and saved, and the row shows the next one", async ({ app }) => {
  await open(app, bdr);
  await expect(app.prompt).toContainText("No phases yet. Add the dates your teacher gave.");
  const add = app.prompt.getByRole("button", { name: "Add a phase" });
  const save = app.prompt.getByRole("button", { name: "Save" });

  await add.click();
  // The new name is selected, so typing replaces "Phase 1".
  await expect(app.prompt.getByLabel("Name of phase 1")).toBeFocused();
  await app.page.keyboard.type("Report");
  // No date yet: nothing to save.
  await expect(save).toBeDisabled();
  await expect(app.prompt).toContainText("Pick a date for each phase.");
  await app.prompt.getByLabel("Date of Report").fill("2026-10-29");
  await app.prompt.getByLabel("Time of Report").fill("18:00");

  await add.click();
  await expect(app.prompt.getByLabel("Name of phase 2")).toHaveValue("Phase 2");
  await app.prompt.getByLabel("Date of Phase 2").fill("2026-10-20");
  await app.prompt.getByRole("checkbox").nth(1).check();
  await save.click();

  const [call] = await app.waitForCall("set_phases");
  expect(call.args.id).toBe(bdr.id);
  // By date, local times as UTC.
  expect(call.args.phases).toEqual([
    {
      id: expect.stringMatching(/^[0-9a-f]{32}$/),
      name: "Phase 2",
      due: "2026-10-20T21:59:00.000Z",
      done: true,
    },
    {
      id: expect.stringMatching(/^[0-9a-f]{32}$/),
      name: "Report",
      due: "2026-10-29T17:00:00.000Z",
      done: false,
    },
  ]);
  await expect(app.prompt).toHaveCount(0);
  // The done phase is skipped; the row says when the next one is.
  await expect(app.row("BDR project").locator(".meta")).toContainText(
    /Report due tomorrow at 6:00\sPM/,
  );
  expect(await app.writes()).toEqual(["set_phases"]);
});

test("existing phases are edited and deleted", async ({ app }) => {
  await open(app, withPhases);
  await expect(app.prompt.getByLabel("Name of phase 1")).toHaveValue("Schema");
  await expect(app.prompt.getByLabel("Date of Schema")).toHaveValue("2026-10-27");
  await expect(app.prompt.getByLabel("Time of Schema")).toHaveValue("23:59");
  await expect(app.prompt.getByRole("checkbox").first()).toBeChecked();

  await app.prompt.getByRole("button", { name: "Delete Schema" }).click();
  await expect(app.prompt.getByLabel("Name of phase 1")).toBeFocused();
  await app.prompt.getByLabel("Name of phase 1").fill("SQL queries");
  await app.prompt.getByRole("button", { name: "Save" }).click();

  const [call] = await app.waitForCall("set_phases");
  expect(call.args.phases).toEqual([
    { id: "p2", name: "SQL queries", due: "2026-10-29T17:00:00.000Z", done: false },
  ]);
});

test("Cancel, Escape and a click outside save nothing", async ({ app }) => {
  await open(app, withPhases);
  await app.prompt.getByLabel("Name of phase 1").fill("Changed");
  await app.prompt.getByRole("button", { name: "Cancel" }).click();
  await expect(app.prompt).toHaveCount(0);

  await app.rowMenu("BDR project");
  await app.pickMenu("Phases…");
  // The edit was dropped.
  await expect(app.prompt.getByLabel("Name of phase 1")).toHaveValue("Schema");
  await app.page.keyboard.press("Escape");
  await expect(app.prompt).toHaveCount(0);

  await app.rowMenu("BDR project");
  await app.pickMenu("Phases…");
  await app.page.mouse.click(5, 5);
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
  // Escape only closed the editor: the popover stays.
  expect(await app.calls("hide_popover")).toEqual([]);
});

test("a blank name can't be saved", async ({ app }) => {
  await open(app, withPhases);
  await app.prompt.getByLabel("Name of phase 2").fill("  ");
  await expect(app.prompt).toContainText("Give each phase a name.");
  await expect(app.prompt.getByRole("button", { name: "Save" })).toBeDisabled();
  expect(await app.writes()).toEqual([]);
});

test("a refusal from Rust shows in the editor, edits kept", async ({ app }) => {
  await open(app, withPhases, { set_phases: [fail("invalidPhase"), pass()] });
  await app.prompt.getByLabel("Name of phase 2").fill("Final");
  await app.prompt.getByRole("button", { name: "Save" }).click();
  await expect(app.prompt.getByRole("alert")).toHaveText(
    "a phase needs a name of 1 to 80 characters, on one line, and a date",
  );
  await expect(app.prompt.getByLabel("Name of phase 2")).toHaveValue("Final");
  // Saving again goes through.
  await app.prompt.getByRole("button", { name: "Save" }).click();
  await expect(app.prompt).toHaveCount(0);
  expect(await app.calls("set_phases")).toHaveLength(2);
});

test("a teacher's due date stays on the row; the phases are in the details", async ({ app }) => {
  const dated = project({
    slug: "bdr",
    name: "BDR project",
    assignment: { due: "2026-11-30T22:59:00Z" },
    phases: withPhases.phases,
  });
  await app.start(scenario({ snapshot: snapshot({ projects: [dated] }) }));
  const row = app.row("BDR project");
  await expect(row.locator(".meta")).toContainText("Due ");
  await expect(row.locator(".meta")).not.toContainText("Queries");
  await row.locator("[data-row]").click();
  const details = plain(await row.locator("dl").textContent());
  expect(details).toContain("Schema · Tue, Oct 27 at 11:59 PM · done");
  expect(details).toContain("Queries · Thu, Oct 29 at 06:00 PM");
});

test("open phases are tray deadlines that open their row", async ({ app }) => {
  await app.start(scenario({ snapshot: snapshot({ projects: [withPhases] }) }));
  const menus = async () =>
    (await app.calls("set_tray_menu")).map((call) => call.args.menu as TrayMenu);
  await expect
    .poll(async () =>
      (await menus()).at(-1)?.deadlines.map((d) => ({ ...d, label: plain(d.label) })),
    )
    .toEqual([
      { id: `${withPhases.id}#p2`, label: "BDR project: Queries · Due tomorrow at 6:00 PM" },
    ]);
  await app.emit("tray-action", { kind: "focus", id: `${withPhases.id}#p2` });
  await expect(app.row("BDR project")).toHaveClass(/linked/);
  await expect(app.row("BDR project").locator("[data-row]")).toHaveAttribute(
    "aria-expanded",
    "true",
  );
});
