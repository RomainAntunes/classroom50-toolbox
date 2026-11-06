// Clone: every clone asks first in a native dialog naming the destination, with Clone,
// Choose another folder… and Cancel. With no clone folder yet the picker comes first and the
// folder is only remembered on Clone; "Clone to…" picks another one directly. Cancelling
// anywhere clones nothing and saves nothing.
import { expect, menuTexts, test, type App } from "./app";
import {
  HOME,
  fail,
  hold,
  ok,
  project,
  scenario,
  settings,
  snapshot,
  type Scenario,
} from "./fixtures";
import type { Settings } from "../../src/lib/api";

const lab = project({ slug: "lab1", name: "Lab 1", local: null });
const REPO = "cs101-lab1-student";

async function start(
  app: App,
  over: Partial<Settings> = {},
  responses = {},
  extra: Partial<Scenario> = {},
) {
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [lab] }),
      settings: settings(over),
      responses,
      ...extra,
    }),
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
  expect(calls[0].args).toEqual({ id: lab.id, parent: `${HOME}/Classroom`, protocol: null });
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
  expect(calls[0].args).toEqual({ id: lab.id, parent: `${HOME}/Code`, protocol: null });
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
  expect(calls[0].args).toEqual({ id: lab.id, parent: `${HOME}/School`, protocol: null });
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
  expect(calls[0].args).toEqual({ id: lab.id, parent: `${HOME}/Elsewhere`, protocol: null });
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

// HTTPS or SSH: gh's `git_protocol` decides, "Clone with" overrides it, and an SSH clone
// checks SSH first so a key or a network that won't work falls back to HTTPS.

test("gh on HTTPS: no SSH check", async ({ app }) => {
  await start(app, {}, { clone_project: [ok(`${HOME}/Classroom/${REPO}`)] });
  await cloneButton(app).click();
  await app.answer("yes");
  await app.waitForCall("clone_project");
  expect(await app.calls("ssh_status")).toEqual([]);
});

test("gh on SSH that works: cloned the way gh says", async ({ app }) => {
  await start(
    app,
    {},
    { clone_project: [ok(`${HOME}/Classroom/${REPO}`)] },
    { gitProtocol: "ssh" },
  );
  await cloneButton(app).click();
  await app.answer("yes");
  const [call] = await app.waitForCall("clone_project");
  expect(call.args).toEqual({ id: lab.id, parent: `${HOME}/Classroom`, protocol: null });
  const [probe] = await app.calls("ssh_status");
  expect(probe.args).toEqual({ fresh: false });
});

test("SSH not ready: the reason, then Clone with HTTPS", async ({ app }) => {
  await start(
    app,
    {},
    { clone_project: [ok(`${HOME}/Classroom/${REPO}`)] },
    { gitProtocol: "ssh", ssh: { state: "keyNotOffered", localKey: "~/.ssh/id_ed25519.pub" } },
  );
  await cloneButton(app).click();
  await app.answer("yes");
  const dialog = await app.expectDialog(
    [/^Your key ~\/\.ssh\/id_ed25519\.pub is on GitHub, but ssh didn't use it\./, /ssh-add/],
    "Clone with HTTPS",
  );
  expect(dialog.title).toBe(REPO);
  expect(dialog.labels).toEqual({ yes: "Clone with HTTPS", no: "SSH settings…", cancel: "Cancel" });
  const [call] = await app.waitForCall("clone_project");
  expect(call.args).toEqual({ id: lab.id, parent: `${HOME}/Classroom`, protocol: "https" });
});

test("SSH not ready: SSH settings… opens Settings and clones nothing", async ({ app, page }) => {
  await start(app, {}, {}, { gitProtocol: "ssh", ssh: { state: "noKey", localKey: null } });
  await cloneButton(app).click();
  await app.answer("yes");
  await app.expectDialog(["No SSH key of this computer is on your GitHub account."], "no");
  // Settings, at its SSH row.
  await expect(page).toHaveURL(/\/settings#ssh$/);
  await expect(page.locator("#ssh")).toBeFocused();
  expect(await app.writes()).toEqual([]);
});

test("SSH not ready: Cancel clones nothing", async ({ app }) => {
  await start(app, {}, {}, { gitProtocol: "ssh", ssh: { state: "unreachable", message: "x" } });
  await cloneButton(app).click();
  await app.answer("yes");
  await app.expectDialog(["Can't reach GitHub over SSH"], "cancel");
  expect(await app.writes()).toEqual([]);
  await expect(app.row("Lab 1")).toContainText("Not cloned");
});

test("an unknown GitHub host key is trusted from the question, then SSH clones", async ({
  app,
}) => {
  await start(
    app,
    {},
    { clone_project: [ok(`${HOME}/Classroom/${REPO}`)] },
    { gitProtocol: "ssh", ssh: { state: "hostKeyUnknown" } },
  );
  await cloneButton(app).click();
  await app.answer("yes");
  await app.expectDialog(["doesn't know GitHub's SSH host key yet"], "Trust GitHub");
  const [call] = await app.waitForCall("clone_project");
  expect(call.args.protocol).toBeNull();
  expect(await app.writes()).toEqual(["trust_github_host", "clone_project"]);
});

test("Clone with in the menu overrides gh's protocol", async ({ app }) => {
  await start(
    app,
    {},
    { clone_project: [ok(`${HOME}/Classroom/${REPO}`)] },
    { gitProtocol: "ssh" },
  );
  const menu = await app.rowMenu("Lab 1");
  const submenu = menu.items.find((i) => i.text === "Clone with");
  expect(submenu?.enabled).toBe(true);
  expect(menuTexts(submenu!)).toEqual(["HTTPS", "SSH"]);
  await app.pickMenu("HTTPS");
  await app.answer("yes");
  const [call] = await app.waitForCall("clone_project");
  expect(call.args.protocol).toBe("https");
  // HTTPS needs no SSH check.
  expect(await app.calls("ssh_status")).toEqual([]);
});

test("Clone with SSH checks SSH, then clones with SSH", async ({ app }) => {
  await start(app, {}, { clone_project: [ok(`${HOME}/Classroom/${REPO}`)] });
  await app.rowMenu("Lab 1");
  await app.pickMenu("SSH");
  await app.answer("yes");
  const [call] = await app.waitForCall("clone_project");
  expect(call.args.protocol).toBe("ssh");
  // SSH was checked before the clone, not after.
  const order = (await app.calls()).map((c) => c.cmd);
  expect(order.indexOf("ssh_status")).toBeGreaterThan(-1);
  expect(order.indexOf("ssh_status")).toBeLessThan(order.indexOf("clone_project"));
});

test("an SSH failure offers Clone with HTTPS, once", async ({ app }) => {
  await start(
    app,
    {},
    {
      clone_project: [
        fail("ssh", "GitHub didn't accept an SSH key", { problem: "keyRejected" }),
        ok(`${HOME}/Classroom/${REPO}`),
      ],
    },
    { gitProtocol: "ssh" },
  );
  await cloneButton(app).click();
  await app.answer("yes");
  const dialog = await app.expectDialog(
    [
      "The clone failed: GitHub didn't accept an SSH key from this computer",
      "Try again with HTTPS?",
    ],
    "ok",
  );
  expect(dialog.labels).toEqual({ ok: "Clone with HTTPS", cancel: "Cancel" });
  const calls = await app.waitForCall("clone_project", 2);
  expect(calls.map((c) => c.args.protocol)).toEqual([null, "https"]);
  await app.waitForCall("open_in_editor");
});

test("an SSH failure, HTTPS declined: one clone only", async ({ app }) => {
  await start(
    app,
    {},
    { clone_project: [fail("ssh", "x", { problem: "unreachable" })] },
    { gitProtocol: "ssh" },
  );
  await cloneButton(app).click();
  await app.answer("yes");
  await app.expectDialog(["can't reach GitHub over SSH"], "cancel");
  expect(await app.calls("clone_project")).toHaveLength(1);
  expect(await app.pendingDialogs()).toEqual([]);
});

test("a clone's menu switches its origin the other way", async ({ app, page }) => {
  const cloned = project({ slug: "lab1", name: "Lab 1" });
  await app.start(scenario({ snapshot: snapshot({ projects: [cloned] }) }));
  let menu = await app.rowMenu("Lab 1");
  expect(menuTexts(menu)).toContain("Use SSH for this clone");
  expect(menuTexts(menu)).not.toContain("Use HTTPS for this clone");
  await app.pickMenu("Use SSH for this clone");
  const [call] = await app.waitForCall("set_clone_remote");
  expect(call.args).toEqual({ id: cloned.id, protocol: "ssh" });
  await expect(page.getByRole("status")).toHaveText("Lab 1 now uses SSH");
  menu = await app.rowMenu("Lab 1");
  expect(menuTexts(menu)).toContain("Use HTTPS for this clone");
});

test("no clone, or an origin elsewhere: no switch in the menu", async ({ app }) => {
  const elsewhere = project({ slug: "lab2", name: "Lab 2" });
  elsewhere.local = { ...elsewhere.local!, origin: null };
  await app.start(scenario({ snapshot: snapshot({ projects: [lab, elsewhere] }) }));
  for (const name of ["Lab 1", "Lab 2"]) {
    const texts = menuTexts(await app.rowMenu(name));
    expect(texts.some((text) => text.endsWith("for this clone"))).toBe(false);
    await app.page.keyboard.press("Escape");
  }
});

const cloned = project({ slug: "lab1", name: "Lab 1" });

test("Use SSH for this clone checks SSH first; Trust GitHub, then the switch", async ({ app }) => {
  await app.start(
    scenario({ snapshot: snapshot({ projects: [cloned] }), ssh: { state: "hostKeyUnknown" } }),
  );
  await app.rowMenu("Lab 1");
  await app.pickMenu("Use SSH for this clone");
  const dialog = await app.expectDialog(["doesn't know GitHub's SSH host key yet"], "ok");
  expect(dialog.labels).toEqual({ ok: "Trust GitHub", cancel: "Cancel" });
  const [call] = await app.waitForCall("set_clone_remote");
  expect(call.args).toEqual({ id: cloned.id, protocol: "ssh" });
  expect(await app.writes()).toEqual(["trust_github_host", "set_clone_remote"]);
});

test("Use SSH for this clone while SSH has no key: no switch", async ({ app, page }) => {
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [cloned] }),
      ssh: { state: "noKey", localKey: null },
    }),
  );
  await app.rowMenu("Lab 1");
  await app.pickMenu("Use SSH for this clone");
  const dialog = await app.expectDialog(["No SSH key of this computer"], "cancel");
  expect(dialog.labels).toEqual({ ok: "SSH settings…", cancel: "Cancel" });
  expect(await app.writes()).toEqual([]);
  await expect(page).toHaveURL(/\/$/);
});

test("Use HTTPS for this clone switches at once, with no SSH check", async ({ app }) => {
  const sshClone = { ...cloned, local: { ...cloned.local!, origin: "ssh" as const } };
  await app.start(scenario({ snapshot: snapshot({ projects: [sshClone] }) }));
  await app.rowMenu("Lab 1");
  await app.pickMenu("Use HTTPS for this clone");
  await app.waitForCall("set_clone_remote");
  expect(await app.calls("ssh_status")).toEqual([]);
});

test("a switch Rust refuses is worded", async ({ app }) => {
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [cloned] }),
      responses: {
        set_clone_remote: [fail("unsupported", "the clone's origin isn't acme-cs/lab-1-alex")],
      },
    }),
  );
  await app.rowMenu("Lab 1");
  await app.pickMenu("Use SSH for this clone");
  const dialog = await app.expectDialog(
    ["Couldn't change how this clone reaches GitHub: the clone's origin isn't acme-cs/lab-1-alex"],
    "ok",
  );
  expect(dialog.kind).toBe("error");
});

test("no switch while git runs in the clone", async ({ app }) => {
  const behind = project({ slug: "lab1", name: "Lab 1", local: { behind: 1 } });
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [behind] }),
      responses: { pull_project: [hold()] },
    }),
  );
  await app.row("Lab 1").getByRole("button", { name: "Pull" }).click();
  await app.waitForCall("pull_project");
  const menu = await app.rowMenu("Lab 1");
  expect(menu.items.find((i) => i.text === "Use SSH for this clone")?.enabled).toBe(false);
});
