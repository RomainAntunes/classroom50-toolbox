// Pull: the row's button only shows when a fast-forward can work, the menu always offers it
// while GitHub has commits the clone lacks, and Rust's refusals are worded by kind.
import { expect, test, type App } from "./app";
import { fail, hold, ok, project, scenario, snapshot } from "./fixtures";
import type { Project } from "../../src/lib/api";
import type { ProjectOptions } from "./fixtures";

const lab = (local: ProjectOptions["local"] = { behind: 2 }) =>
  project({ slug: "lab1", name: "Lab 1", local });

async function start(app: App, row: Project, responses = {}) {
  await app.start(scenario({ snapshot: snapshot({ projects: [row] }), responses }));
  await expect(app.row("Lab 1")).toBeVisible();
}

const pullButton = (app: App) => app.row("Lab 1").getByRole("button", { name: "Pull" });

test("the button pulls, says Pulling… meanwhile, then announces it", async ({ app, page }) => {
  const behind = lab();
  await start(app, behind, { pull_project: [hold()] });
  await expect(app.row("Lab 1")).toContainText("2 to pull");
  await pullButton(app).click();

  expect((await app.waitForCall("pull_project"))[0].args).toEqual({ id: behind.id });
  await expect(app.row("Lab 1")).toContainText("Pulling…");
  await expect(pullButton(app)).toBeDisabled();

  await app.release("pull_project", ok());
  await expect(page.getByRole("status")).toHaveText("Pulled: Lab 1 is up to date with GitHub");
  expect(await app.pendingDialogs()).toEqual([]);
  // Rust refreshes after the pull (the watcher sees FETCH_HEAD); the row follows.
  await app.emit("projects-updated", snapshot({ projects: [lab({ behind: 0 })] }));
  await expect(pullButton(app)).toHaveCount(0);
  await expect(app.row("Lab 1")).not.toContainText("to pull");
  expect(await app.writes()).toEqual(["pull_project"]);
});

test("GitHub ahead with commits not fetched yet shows the button too", async ({ app }) => {
  await start(app, lab({ remoteAhead: true }));
  await expect(app.row("Lab 1")).toContainText("New commits on GitHub");
  await expect(pullButton(app)).toBeVisible();
});

test("untracked files alone don't hide the button", async ({ app }) => {
  await start(app, lab({ behind: 1, changedFiles: 1, changedPaths: ["notes.txt"] }));
  await expect(pullButton(app)).toBeVisible();
});

for (const [why, local] of [
  ["uncommitted changes", { behind: 1, changedFiles: 1, trackedChanges: 1 }],
  ["another branch", { behind: 1, branch: "experiment" }],
  ["local commits", { behind: 1, ahead: 1 }],
] as const) {
  test(`no button with ${why}, but the menu still offers Pull`, async ({ app }) => {
    await start(app, lab(local));
    await expect(pullButton(app)).toHaveCount(0);
    const menu = await app.rowMenu("Lab 1");
    const item = menu.items.find((i) => i.text === "Pull");
    expect(item?.enabled).toBe(true);
  });
}

test("up to date: no button, and Pull is disabled in the menu", async ({ app }) => {
  await start(app, lab({ behind: 0 }));
  await expect(pullButton(app)).toHaveCount(0);
  const menu = await app.rowMenu("Lab 1");
  expect(menu.items.find((i) => i.text === "Pull")?.enabled).toBe(false);
});

for (const [kind, local, words] of [
  [
    "localChanges",
    { behind: 1, changedFiles: 1, trackedChanges: 1 },
    "You have changes that aren't committed yet. Commit them first, then pull",
  ],
  ["otherBranch", { behind: 1, branch: "experiment" }, "Switch to main to pull the new commits."],
  ["diverged", { behind: 1, ahead: 1 }, "Both your clone and GitHub have new commits"],
] as const) {
  test(`a ${kind} refusal is worded from the menu`, async ({ app }) => {
    await start(app, lab(local), { pull_project: [fail(kind)] });
    await app.rowMenu("Lab 1");
    await app.pickMenu("Pull");
    const dialog = await app.dialog();
    expect(dialog.title).toBe("Lab 1");
    expect(dialog.kind).toBe("warning");
    expect(dialog.message).toContain(words);
    expect(dialog.message).not.toContain("from Rust");
    await app.answer("ok");
    await expect(app.row("Lab 1")).not.toContainText("Pulling…");
    expect(await app.page.getByRole("status").textContent()).toBe("");
  });
}

test("the branch named in the otherBranch refusal is GitHub's default one", async ({ app }) => {
  const trunk = project({
    slug: "lab1",
    name: "Lab 1",
    local: { behind: 1, branch: "main" },
    remote: { defaultBranch: "trunk" },
  });
  await start(app, trunk, { pull_project: [fail("otherBranch")] });
  await app.rowMenu("Lab 1");
  await app.pickMenu("Pull");
  await app.expectDialog(["Switch to trunk to pull"], "ok");
});

test("any other failure shows Rust's words", async ({ app }) => {
  await start(app, lab(), {
    pull_project: [fail("commandFailed", "fatal: unable to access GitHub")],
  });
  await pullButton(app).click();
  await app.expectDialog(["The pull failed: fatal: unable to access GitHub"], "ok");
});
