// Submit: `gh student submit` asks nothing, so the app warns first (late, GitHub ahead,
// editor files), refuses uncommitted work, and handles repos without a marker per the
// `commitActions` setting. Cancelling anywhere invokes no write.
import { expect, test } from "./app";
import { hoursFromNow, ok, project, scenario, settings, snapshot } from "./fixtures";
import type { Project, Settings } from "../../src/lib/api";

const clean = { hasMarker: true, uncommitted: [], editorFiles: [] };

function setup(lab: Project, check: object, over: Partial<Settings> = {}) {
  return scenario({
    snapshot: snapshot({ projects: [lab] }),
    settings: settings(over),
    responses: { submit_check: [ok(check)] },
  });
}

const ready = (over: Parameters<typeof project>[0] = { slug: "lab1" }) =>
  project({ name: "Lab 1", pending: "unsubmitted", ...over });

test("late and GitHub ahead are both said before submitting; Cancel sends nothing", async ({
  app,
  page,
}) => {
  const lab = ready({
    slug: "lab1",
    assignment: { due: hoursFromNow(-3) },
    local: { remoteAhead: true },
    remote: { head: { oid: "ffff", at: hoursFromNow(-1), author: "teammate" } },
  });
  await app.start(setup(lab, clean));
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();

  const dialog = await app.dialog();
  expect(dialog.kind).toBe("warning");
  expect(dialog.message).toContain("This sends your committed work in");
  expect(dialog.message).toContain("(the latest by teammate)");
  expect(dialog.message).toContain("will be marked late");
  expect(dialog.labels).toEqual({ ok: "Submit", cancel: "Cancel" });
  await app.answer("cancel");

  await expect(app.row("Lab 1").getByRole("button", { name: "Submit" })).toBeEnabled();
  expect(await app.writes()).toEqual([]);
  expect((await app.calls("submit_check"))[0].args).toEqual({ id: lab.id });
});

test("confirming submits, then offers the submission page", async ({ app, page }) => {
  const lab = ready();
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [lab] }),
      responses: {
        submit_check: [ok({ ...clean, editorFiles: [".vscode/settings.json"] })],
        submit_project: [ok({ url: "https://github.com/cs50-school/commit/1" })],
      },
    }),
  );
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  const confirm = await app.dialog();
  expect(confirm.kind).toBe("info");
  expect(confirm.message).toContain(
    "Editor and system files in the folder are sent too: .vscode/settings.json",
  );
  expect(confirm.message).not.toContain("late");
  await app.answer("ok");

  await app.expectDialog([/^Submitted\./], "ok");
  expect(await app.writes()).toEqual(["submit_project"]);
  expect((await app.calls("submit_project"))[0].args).toEqual({ id: lab.id });
  const opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened[0].args.url).toBe("https://github.com/cs50-school/commit/1");
});

test("the same work again is pointed out", async ({ app }) => {
  // The clone's head is GitHub's: nothing new since the last push.
  await app.start(setup(ready(), clean));
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  await app.expectDialog([/grades the same work again/], "cancel");
  expect(await app.writes()).toEqual([]);
});

test("uncommitted work is refused; with commits left to git it points to the editor", async ({
  app,
}) => {
  const lab = ready({
    slug: "lab1",
    local: { changedFiles: 1, changedPaths: ["hello.c"] },
    pending: "uncommitted",
  });
  await app.start(setup(lab, { ...clean, uncommitted: ["hello.c"] }));
  // No row button while there are changes: the menu keeps Submit to explain.
  await expect(app.row("Lab 1").getByRole("button", { name: "Submit" })).toHaveCount(0);
  await app.rowMenu("Lab 1");
  await app.pickMenu("Submit…");
  const dialog = await app.dialog();
  expect(dialog.message).toContain("these changes aren't committed yet: hello.c");
  expect(dialog.message).toContain("Commit them in your editor or a terminal first");
  expect(dialog.labels).toEqual({ ok: "Ok" });
  await app.answer("ok");
  expect(await app.writes()).toEqual([]);
});

test("uncommitted work with Commit allowed offers Commit, and Cancel commits nothing", async ({
  app,
}) => {
  const lab = ready({
    slug: "lab1",
    local: { changedFiles: 1, changedPaths: ["hello.c"] },
    pending: "uncommitted",
  });
  await app.start(setup(lab, { ...clean, uncommitted: ["hello.c"] }, { commitActions: "commit" }));
  await app.rowMenu("Lab 1");
  await app.pickMenu("Submit…");
  const dialog = await app.dialog();
  expect(dialog.message).toContain("Commit them first, then submit.");
  expect(dialog.labels).toEqual({ ok: "Commit…", cancel: "Cancel" });
  await app.answer("cancel");
  expect(await app.calls("commit_check")).toEqual([]);
  expect(await app.writes()).toEqual([]);
});

test("no marker on a tag-mode assignment only explains", async ({ app }) => {
  await app.start(setup(ready(), { ...clean, hasMarker: false }, { commitActions: "push" }));
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  const dialog = await app.dialog();
  expect(dialog.message).toContain("no autograder setup to submit through");
  expect(dialog.labels).toEqual({ ok: "Ok" });
  await app.answer("ok");
  expect(await app.writes()).toEqual([]);
});

for (const commitActions of ["none", "commit"] as const) {
  test(`no marker on an every-push assignment only explains with commitActions ${commitActions}`, async ({
    app,
  }) => {
    const lab = ready({ slug: "lab1", assignment: { submissionMode: "every-push" } });
    await app.start(setup(lab, { ...clean, hasMarker: false }, { commitActions }));
    await app.rowMenu("Lab 1");
    await app.pickMenu("Submit…");
    await app.expectDialog([/Commit and push your work with git instead/], "ok");
    expect(await app.writes()).toEqual([]);
  });
}

test("no marker on an every-push assignment offers Commit & push when allowed", async ({ app }) => {
  const lab = ready({ slug: "lab1", assignment: { submissionMode: "every-push" } });
  await app.start(setup(lab, { ...clean, hasMarker: false }, { commitActions: "push" }));
  await app.rowMenu("Lab 1");
  await app.pickMenu("Submit…");
  const dialog = await app.dialog();
  expect(dialog.message).toContain("every push to GitHub counts as your submission");
  expect(dialog.labels).toEqual({ ok: "Commit & push…", cancel: "Cancel" });
  await app.answer("cancel");
  expect(await app.calls("commit_check")).toEqual([]);
  expect(await app.writes()).toEqual([]);
});
