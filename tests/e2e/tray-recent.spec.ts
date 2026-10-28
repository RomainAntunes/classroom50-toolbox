// The tray menu's Open Recent: the clones worked in last, opened in their editor straight
// from the menu (Rust hands the click back as `tray-action` `open`, with no popover).
import { expect, test, type App } from "./app";
import { HOME, classroom, project, scenario, settings, snapshot } from "./fixtures";
import type { TrayMenu } from "../../src/lib/api";

const algo = classroom({ slug: "algo", name: "Algorithms" });
const PROJECTS = [
  project({ slug: "hello", name: "Hello", local: { activeAt: "2026-10-28T09:00:00Z" } }),
  project({ slug: "mario", name: "Mario", local: { activeAt: "2026-10-28T13:00:00Z" } }),
  project({ slug: "lab-1", name: "Lab 1", local: { activeAt: "2026-10-28T12:00:00Z" } }),
  project({
    slug: "lab-1",
    name: "Lab 1",
    classroom: algo,
    editor: "idea",
    local: { activeAt: "2026-10-28T11:00:00Z" },
  }),
  project({ slug: "cash", name: "Cash", local: null }),
];
const EDITORS = [
  { id: "vscode", name: "Visual Studio Code" },
  { id: "idea", name: "IntelliJ IDEA" },
];

async function lastMenu(app: App): Promise<TrayMenu | undefined> {
  return (await app.calls("set_tray_menu")).at(-1)?.args.menu as TrayMenu | undefined;
}

test.beforeEach(async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-28T14:00:00Z") });
});

test("Open Recent lists the clones worked in last", async ({ app }) => {
  await app.start(scenario({ snapshot: snapshot({ projects: PROJECTS }), editors: EDITORS }));
  await expect
    .poll(async () => (await lastMenu(app))?.recent)
    .toEqual([
      { id: "cs50-school/cs101-mario-student", label: "Mario" },
      { id: "cs50-school/cs101-lab-1-student", label: "Lab 1 (Intro to CS)" },
      { id: "cs50-school/algo-lab-1-student", label: "Lab 1 (Algorithms)" },
      { id: "cs50-school/cs101-hello-student", label: "Hello" },
    ]);
});

test("Open Recent leaves hidden clones out and follows a sync", async ({ app }) => {
  const hiding = settings({
    hidden: ["cs50-school/cs101-mario-student"],
    hiddenClasses: ["cs50-school/algo"],
  });
  await app.start(
    scenario({ snapshot: snapshot({ projects: PROJECTS }), editors: EDITORS, settings: hiding }),
  );
  await expect
    .poll(async () => (await lastMenu(app))?.recent.map((item) => item.label))
    .toEqual(["Lab 1", "Hello"]);

  const hello = project({
    slug: "hello",
    name: "Hello",
    local: { activeAt: "2026-10-28T13:59:00Z" },
  });
  await app.emit("projects-updated", snapshot({ projects: [hello, ...PROJECTS.slice(1)] }));
  await expect
    .poll(async () => (await lastMenu(app))?.recent.map((item) => item.label))
    .toEqual(["Hello", "Lab 1"]);
});

test("Open Recent needs an editor", async ({ app }) => {
  await app.start(scenario({ snapshot: snapshot({ projects: PROJECTS }), editors: [] }));
  await expect(app.page.getByRole("searchbox")).toBeVisible();
  await expect.poll(async () => (await lastMenu(app))?.refresh).toBe("Refresh");
  expect((await lastMenu(app))?.recent).toEqual([]);
});

test("a clone picked in Open Recent opens in its editor", async ({ app, page }) => {
  await app.start(scenario({ snapshot: snapshot({ projects: PROJECTS }), editors: EDITORS }));
  await expect(page.getByRole("searchbox")).toBeVisible();

  await app.emit("tray-action", { kind: "open", id: "cs50-school/algo-lab-1-student" });
  const [call] = await app.waitForCall("open_in_editor");
  expect(call.args).toEqual({ path: `${HOME}/Classroom/algo-lab-1-student`, editor: "idea" });

  // No editor recorded: the default one, the first installed.
  await app.emit("tray-action", { kind: "open", id: "cs50-school/cs101-hello-student" });
  const calls = await app.waitForCall("open_in_editor", 2);
  expect(calls[1].args).toEqual({
    path: `${HOME}/Classroom/cs101-hello-student`,
    editor: "vscode",
  });
  await expect(page).toHaveURL(/\/$/);
});
