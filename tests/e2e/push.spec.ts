// Commit and Commit & push from a row's menu: what the prompt says, which command runs, and
// each refusal worded by its kind. Nothing is written when the student cancels.
import { expect, test, type App } from "./app";
import { fail, hoursFromNow, ok, project, scenario, settings, snapshot } from "./fixtures";
import type { CommitCheck } from "../../src/lib/api";
import type { Reply } from "./fixtures";

const lab = project({
  slug: "lab1",
  name: "Lab 1",
  assignment: { submissionMode: "every-push" },
  pending: "uncommitted",
  local: { changedFiles: 2, changedPaths: ["hello.c", "notes.md"] },
});

function check(over: Partial<CommitCheck> = {}): CommitCheck {
  return {
    files: ["hello.c", "notes.md"],
    untracked: ["notes.md"],
    skipped: [],
    large: [],
    tooLarge: [],
    ahead: 0,
    hasIdentity: true,
    pushBlocked: null,
    ...over,
  };
}

async function open(
  app: App,
  responses: Record<string, Reply[]>,
  item = "Commit & push…",
  project = lab,
) {
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [project] }),
      settings: settings({ commitActions: "push" }),
      responses,
    }),
  );
  await app.rowMenu("Lab 1");
  await app.pickMenu(item);
}

test("Commit & push: the prompt lists the files and pushes with the message", async ({ app }) => {
  await open(app, {
    commit_check: [ok(check({ skipped: [".DS_Store"], ahead: 1 }))],
    commit_push: [ok({ commit: "abc" })],
  });
  await expect(app.prompt).toBeVisible();
  await expect(app.prompt.getByRole("heading")).toHaveText("Commit & push");
  await expect(app.prompt).toContainText("2 changed files: hello.c, notes.md.");
  await expect(app.prompt).toContainText("New files included: notes.md.");
  await expect(app.prompt).toContainText("Left out (editor and system files): .DS_Store.");
  await expect(app.prompt).toContainText("Your 1 commit not on GitHub yet is pushed too.");
  // Nothing to say, nothing to send.
  await expect(app.prompt.getByRole("button", { name: "Commit & push" })).toBeDisabled();
  await app.prompt.getByRole("textbox").fill("Fix the loop");
  await app.prompt.getByRole("button", { name: "Commit & push" }).click();

  const calls = await app.waitForCall("commit_push");
  expect(calls[0].args).toEqual({ id: lab.id, message: "Fix the loop" });
  expect(await app.writes()).toEqual(["commit_push"]);
});

test("Commit only is the second action and never pushes", async ({ app }) => {
  await open(app, { commit_check: [ok(check())], commit_only: [ok({ commit: "abc" })] });
  await app.prompt.getByRole("textbox").fill("WIP");
  await app.prompt.getByRole("button", { name: "Commit only" }).click();
  expect((await app.waitForCall("commit_only"))[0].args).toEqual({ id: lab.id, message: "WIP" });
  expect(await app.writes()).toEqual(["commit_only"]);
});

test("cancelling the prompt writes nothing", async ({ app }) => {
  await open(app, { commit_check: [ok(check())] });
  await app.prompt.getByRole("textbox").fill("never sent");
  await app.prompt.getByRole("button", { name: "Cancel" }).click();
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
});

test("past the due date, pushing asks once more", async ({ app }) => {
  const late = project({
    slug: "lab1",
    name: "Lab 1",
    assignment: { submissionMode: "every-push", due: hoursFromNow(-2) },
    local: { changedFiles: 2 },
    pending: "uncommitted",
  });
  await open(app, { commit_check: [ok(check())] }, "Commit & push…", late);
  await app.prompt.getByRole("textbox").fill("Late fix");
  await app.prompt.getByRole("button", { name: "Commit & push" }).click();
  const dialog = await app.expectDialog([/will be marked late\. Continue\?/], "cancel");
  expect(dialog.labels).toEqual({ ok: "Continue", cancel: "Cancel" });
  expect(await app.writes()).toEqual([]);
});

test("GitHub ahead: only Commit is offered, with one line saying why", async ({ app }) => {
  await open(app, { commit_check: [ok(check({ pushBlocked: "remoteAhead" }))] });
  await expect(app.prompt.getByRole("heading")).toHaveText("Commit");
  await expect(app.prompt).toContainText("GitHub has newer commits to merge before you push.");
  await expect(app.prompt.getByRole("button", { name: "Commit & push" })).toHaveCount(0);
  await expect(app.prompt.getByRole("button", { name: "Commit only" })).toHaveCount(0);
  await app.prompt.getByRole("button", { name: "Cancel" }).click();
  expect(await app.writes()).toEqual([]);
});

test("a tag-mode assignment commits only, from Commit…", async ({ app }) => {
  const tag = project({
    slug: "lab1",
    name: "Lab 1",
    assignment: { submissionMode: "tag" },
    local: { changedFiles: 2 },
    pending: "uncommitted",
  });
  await open(
    app,
    { commit_check: [ok(check({ pushBlocked: "tagMode" }))], commit_only: [ok({ commit: "abc" })] },
    "Commit…",
    tag,
  );
  await expect(app.prompt).toContainText("this assignment is graded through Submit");
  await app.prompt.getByRole("textbox").fill("Done");
  await app.prompt.getByRole("button", { name: "Commit", exact: true }).click();
  await app.waitForCall("commit_only");
  expect(await app.writes()).toEqual(["commit_only"]);
});

test("files over 100 MB stop it before any prompt", async ({ app }) => {
  await open(app, { commit_check: [ok(check({ tooLarge: ["data.bin"] }))] });
  const dialog = await app.expectDialog([/GitHub refuses files over 100 MB.*data\.bin/s], "ok");
  expect(dialog.kind).toBe("error");
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
});

test("no git identity: the signed-in account is offered, then the prompt", async ({ app }) => {
  await open(app, {
    commit_check: [ok(check({ hasIdentity: false }))],
    suggested_git_identity: [ok({ name: "Stu Dent", email: "1+student@users.noreply.github.com" })],
  });
  const dialog = await app.expectDialog(
    [/Use Stu Dent <1\+student@users\.noreply\.github\.com> for this clone\?/],
    "ok",
  );
  expect(dialog.labels.ok).toBe("Use it");
  const set = await app.waitForCall("set_git_identity");
  expect(set[0].args).toEqual({
    id: lab.id,
    name: "Stu Dent",
    email: "1+student@users.noreply.github.com",
  });
  await expect(app.prompt).toBeVisible();
});

test("no git identity, declined: nothing is written", async ({ app }) => {
  await open(app, {
    commit_check: [ok(check({ hasIdentity: false }))],
    suggested_git_identity: [ok({ name: "Stu Dent", email: "1+student@users.noreply.github.com" })],
  });
  await app.answer("cancel");
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
});

test("nothing to commit: Commit & push offers to push the local commits", async ({ app }) => {
  await open(app, {
    commit_check: [fail("nothingToCommit")],
    push_check: [ok(check({ files: [], untracked: [], ahead: 2 }))],
  });
  const dialog = await app.expectDialog([/Push your 2 commits to main on GitHub\?/], "cancel");
  expect(dialog.labels.ok).toBe("Push");
  expect(await app.writes()).toEqual([]);
});

test("nothing to push is worded, not Rust's message", async ({ app }) => {
  await open(app, { commit_check: [fail("nothingToCommit")], push_check: [fail("nothingToPush")] });
  const dialog = await app.expectDialog([/^Nothing to push: there are no changes/], "ok");
  expect(dialog.message).not.toContain("from Rust");
  expect(await app.writes()).toEqual([]);
});

for (const [kind, words] of [
  ["remoteAhead", /^GitHub has commits this clone doesn't have\. Pull first/],
  ["otherBranch", /^This clone is on another branch\. Switch to main/],
  ["gitInProgress", /^A merge or rebase isn't finished/],
  ["noIdentity", /^Git doesn't know your name and email yet/],
  ["pushRejected", /^GitHub refused the push because someone pushed/],
] as const) {
  test(`a ${kind} refusal from Commit & push is worded`, async ({ app }) => {
    await open(app, { commit_check: [ok(check())], commit_push: [fail(kind)] });
    await app.prompt.getByRole("textbox").fill("Try");
    await app.prompt.getByRole("button", { name: "Commit & push" }).click();
    const dialog = await app.expectDialog([words], "ok");
    expect(dialog.kind).toBe("warning");
  });
}

test("Commit and Commit & push are not offered while commitActions is none", async ({ app }) => {
  await app.start(scenario({ snapshot: snapshot({ projects: [lab] }) }));
  const menu = await app.rowMenu("Lab 1");
  const texts = menu.items.map((item) => item.text);
  expect(texts).not.toContain("Commit…");
  expect(texts).not.toContain("Commit & push…");
});
