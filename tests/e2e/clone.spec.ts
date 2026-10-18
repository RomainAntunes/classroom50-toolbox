// Clone: every clone asks first in a native dialog naming the destination, with Clone,
// Choose another folder… and Cancel. With no clone folder yet the picker comes first and the
// folder is only remembered on Clone; "Clone to…" picks another one directly. Cancelling
// anywhere clones nothing and saves nothing.
import { expect, test, type App } from "./app";
import { HOME, fail, hold, ok, project, scenario, settings, snapshot } from "./fixtures";
import type { Settings } from "../../src/lib/api";

const lab = project({ slug: "lab1", name: "Lab 1", local: null });
const REPO = "cs101-lab1-student";

async function start(app: App, over: Partial<Settings> = {}, responses = {}) {
  await app.start(
    scenario({ snapshot: snapshot({ projects: [lab] }), settings: settings(over), responses }),
  );
  await expect(app.row("Lab 1")).toContainText("Not cloned");
}

const cloneButton = (app: App) => app.row("Lab 1").getByRole("button", { name: "Clone" });

/** The clone folder of each `update_settings` call. */
async function savedFolders(app: App): Promise<unknown[]> {
  return (await app.calls("update_settings")).map(
    (call) => (call.args.next as Settings).cloneFolder,
  );
}

test("the question names the destination; Clone clones there and opens the editor", async ({
  app,
  page,
}) => {
  await start(app, {}, { clone_project: [hold()] });
  await cloneButton(app).click();

  const dialog = await app.dialog();
  expect(dialog.type).toBe("message");
  expect(dialog.title).toBe(REPO);
  expect(dialog.message).toBe(`Clone into ~/Classroom/${REPO}?`);
  expect(dialog.labels).toEqual({ yes: "Clone", no: "Choose another folder…", cancel: "Cancel" });
  // The row only says Cloning… once the clone runs, not while the question is up.
  await expect(app.row("Lab 1")).toContainText("Not cloned");
  await app.answer("yes");

  const calls = await app.waitForCall("clone_project");
  expect(calls[0].args).toEqual({ id: lab.id, parent: `${HOME}/Classroom` });
  await expect(app.row("Lab 1")).toContainText("Cloning…");
  await expect(cloneButton(app)).toBeDisabled();
  await app.release("clone_project", ok(`${HOME}/Classroom/${REPO}`));

  const opened = await app.waitForCall("open_in_editor");
  expect(opened[0].args).toEqual({ path: `${HOME}/Classroom/${REPO}`, editor: "vscode" });
  await expect(page.getByRole("status")).toHaveText(`Cloned into ${REPO}`);
  // The folder was already the saved one: nothing to remember.
  expect(await app.calls("update_settings")).toEqual([]);
  expect(await app.writes()).toEqual(["clone_project"]);
});

test("Cancel clones nothing and saves nothing", async ({ app }) => {
  await start(app);
  await cloneButton(app).click();
  await app.answer("cancel");
  await expect(app.row("Lab 1")).toContainText("Not cloned");
  expect(await app.writes()).toEqual([]);
  expect(await app.calls("update_settings")).toEqual([]);
  expect(await app.pendingDialogs()).toEqual([]);
});

test("Escape on the question is a cancel too", async ({ app }) => {
  await start(app);
  await cloneButton(app).click();
  await app.answer(null);
  expect(await app.writes()).toEqual([]);
});

test("Choose another folder… picks one, remembers it and clones there", async ({ app }) => {
  await start(app, {}, { clone_project: [ok(`${HOME}/Code/${REPO}`)] });
  await cloneButton(app).click();
  await app.answer("no");

  const picker = await app.dialog();
  expect(picker.type).toBe("open");
  expect(picker.title).toBe("Where should assignments be cloned?");
  expect(picker.options).toMatchObject({ directory: true, defaultPath: `${HOME}/Classroom` });
  await app.answer(`${HOME}/Code`);

  const calls = await app.waitForCall("clone_project");
  expect(calls[0].args).toEqual({ id: lab.id, parent: `${HOME}/Code` });
  expect((await app.settings()).cloneFolder).toBe(`${HOME}/Code`);
});

test("Choose another folder…, then cancelling the picker clones nothing", async ({ app }) => {
  await start(app);
  await cloneButton(app).click();
  await app.answer("no");
  await app.answer(null);
  expect(await app.writes()).toEqual([]);
  expect((await app.settings()).cloneFolder).toBe(`${HOME}/Classroom`);
});

test("no clone folder yet: the picker comes first, and Clone remembers it", async ({ app }) => {
  await start(app, { cloneFolder: null }, { clone_project: [ok(`${HOME}/School/${REPO}`)] });
  await cloneButton(app).click();

  const picker = await app.dialog();
  expect(picker.type).toBe("open");
  await app.answer(`${HOME}/School`);
  // Nothing saved before the student confirms.
  expect(await app.calls("update_settings")).toEqual([]);

  await app.expectDialog([`Clone into ~/School/${REPO}?`], "yes");
  const calls = await app.waitForCall("clone_project");
  expect(calls[0].args).toEqual({ id: lab.id, parent: `${HOME}/School` });
  expect(await savedFolders(app)).toEqual([`${HOME}/School`]);
});

test("no clone folder yet: Cancel after the picker forgets the folder", async ({ app }) => {
  await start(app, { cloneFolder: null });
  await cloneButton(app).click();
  await app.answer(`${HOME}/School`);
  await app.expectDialog([`Clone into ~/School/${REPO}?`], "cancel");
  expect(await app.writes()).toEqual([]);
  expect(await app.calls("update_settings")).toEqual([]);
  expect((await app.settings()).cloneFolder).toBeNull();
});

test("no clone folder yet: cancelling the picker asks nothing more", async ({ app }) => {
  await start(app, { cloneFolder: null });
  await cloneButton(app).click();
  await app.answer(null);
  expect(await app.pendingDialogs()).toEqual([]);
  expect(await app.writes()).toEqual([]);
});

test("Clone to… in the menu picks a folder directly and remembers it", async ({ app }) => {
  await start(app, {}, { clone_project: [ok(`${HOME}/Elsewhere/${REPO}`)] });
  const menu = await app.rowMenu("Lab 1");
  expect(menu.items.find((i) => i.text === "Clone")?.enabled).toBe(true);
  await app.pickMenu("Clone to…");

  const picker = await app.dialog();
  expect(picker.type).toBe("open");
  await app.answer(`${HOME}/Elsewhere`);
  const calls = await app.waitForCall("clone_project");
  expect(calls[0].args).toEqual({ id: lab.id, parent: `${HOME}/Elsewhere` });
  expect((await app.settings()).cloneFolder).toBe(`${HOME}/Elsewhere`);
  // No question in between: the menu item was the go-ahead.
  expect(await app.pendingDialogs()).toEqual([]);
});

test("a folder already there is worded, with a way out", async ({ app }) => {
  await start(
    app,
    {},
    {
      clone_project: [
        fail("pathExists", `${HOME}/Classroom/${REPO} already exists`, {
          path: `${HOME}/Classroom/${REPO}`,
        }),
      ],
    },
  );
  await cloneButton(app).click();
  await app.answer("yes");
  const dialog = await app.dialog();
  expect(dialog.kind).toBe("error");
  expect(dialog.message).toContain(
    "Move or rename that folder, or use Clone to… to pick another place.",
  );
  await app.answer("ok");
  expect(await app.calls("open_in_editor")).toEqual([]);
  await expect(app.row("Lab 1")).toContainText("Not cloned");
});

test("another failure shows Rust's words", async ({ app }) => {
  await start(app, {}, { clone_project: [fail("commandFailed", "Repository not found.")] });
  await cloneButton(app).click();
  await app.answer("yes");
  await app.expectDialog([/^The clone failed: Repository not found\.$/], "ok");
});

test("a clone already on disk offers no Clone", async ({ app }) => {
  await app.start(
    scenario({ snapshot: snapshot({ projects: [project({ slug: "lab1", name: "Lab 1" })] }) }),
  );
  await expect(cloneButton(app)).toHaveCount(0);
  const menu = await app.rowMenu("Lab 1");
  expect(menu.items.find((i) => i.text === "Clone")?.enabled).toBe(false);
  expect(menu.items.find((i) => i.text === "Clone to…")?.enabled).toBe(false);
});
