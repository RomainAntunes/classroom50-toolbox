// Submit: `gh student submit` asks nothing, so the app warns first (late, GitHub ahead,
// editor files), refuses uncommitted work, and submits repos without a marker when Rust
// says gh-student can (else it explains, or offers to update gh-student). Cancelling
// anywhere invokes no write.
import { expect, test } from "./app";
import {
  classroom,
  fail,
  hoursFromNow,
  ok,
  project,
  scenario,
  settings,
  snapshot,
  tools,
} from "./fixtures";
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

// Without `.classroom50.yaml` (`no_autograder`, `empty_repo`, a deleted marker) gh-student
// finds the assignment from the repo's name; Rust refuses first when it couldn't.
const markerless = { ...clean, hasMarker: false };

test("no marker in a listed class submits like any other repo", async ({ app }) => {
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [ready()] }),
      responses: {
        submit_check: [ok(markerless)],
        submit_project: [ok({ url: null })],
      },
    }),
  );
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  const confirm = await app.dialog();
  expect(confirm.message).toContain("This sends your committed work in");
  expect(confirm.labels).toEqual({ ok: "Submit", cancel: "Cancel" });
  await app.answer("ok");
  await app.expectDialog([/^Submitted\./], "ok");
  expect(await app.writes()).toEqual(["submit_project"]);
  expect((await app.calls("submit_project"))[0].args).toEqual({ id: ready().id });
  await expect(app.page.getByRole("status")).toContainText("Submitted: Lab 1");
});

test("no marker in an unlisted class submits too; the key stays in Rust", async ({ app }) => {
  const lab = ready({ slug: "lab1", classroom: classroom({ secret: "a1b2c3d4" }) });
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [lab] }),
      responses: {
        submit_check: [ok(markerless)],
        submit_project: [ok({ url: null })],
      },
    }),
  );
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  await app.expectDialog([/This sends your committed work in/], "ok");
  await app.expectDialog([/^Submitted\./], "ok");
  expect((await app.calls("submit_check"))[0].args).toEqual({ id: lab.id });
  expect((await app.calls("submit_project"))[0].args).toEqual({ id: lab.id });
});

for (const commitActions of ["none", "push"] as const) {
  test(`no marker on an every-push assignment submits from the menu with commitActions ${commitActions}`, async ({
    app,
  }) => {
    const lab = ready({ slug: "lab1", assignment: { submissionMode: "every-push" } });
    await app.start(setup(lab, markerless, { commitActions }));
    // Every-push rows have no Submit button; the menu has it.
    await expect(app.row("Lab 1").getByRole("button", { name: "Submit" })).toHaveCount(0);
    await app.rowMenu("Lab 1");
    await app.pickMenu("Submit…");
    const confirm = await app.dialog();
    expect(confirm.message).toContain("This sends your committed work in");
    expect(confirm.labels).toEqual({ ok: "Submit", cancel: "Cancel" });
    await app.answer("cancel");
    expect(await app.calls("commit_check")).toEqual([]);
    expect(await app.writes()).toEqual([]);
  });
}

test("no marker and an assignment the app doesn't know only explains", async ({ app }) => {
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [ready()] }),
      responses: { submit_check: [fail("unknownAssignment")] },
    }),
  );
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  const dialog = await app.dialog();
  expect(dialog.message).toContain("can't tell which assignment this repository belongs to");
  expect(dialog.message).toContain("Commit and push your work with git instead");
  expect(dialog.labels).toEqual({ ok: "Ok" });
  await app.answer("ok");
  expect(await app.writes()).toEqual([]);
});

const tooOld = fail(
  "ghStudentOutdated",
  "submitting a repository without .classroom50.yaml needs gh-student 1.56.0 or newer (installed: v1.52.2)",
  { version: "v1.52.2", min: "1.56.0" },
);
const oldTools = tools().map((tool) =>
  tool.tool === "gh-student" ? { ...tool, version: "v1.52.2", outdated: true } : tool,
);
const updated = { tool: "gh-student", installed: true, version: "v1.58.1", outdated: false };

test("no marker with an old gh-student offers Update; Cancel updates and submits nothing", async ({
  app,
}) => {
  await app.start(
    scenario({
      tools: oldTools,
      snapshot: snapshot({ projects: [ready()] }),
      responses: { submit_check: [tooOld] },
    }),
  );
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  const dialog = await app.dialog();
  expect(dialog.message).toContain("finds its assignment from the repository's name");
  expect(dialog.message).toContain("needs version 1.56.0 or newer, and v1.52.2 is installed");
  expect(dialog.labels).toEqual({ ok: "Update", cancel: "Cancel" });
  await app.answer("cancel");
  expect(await app.writes()).toEqual([]);
});

test("no marker with an old gh-student updates it, then asks to submit", async ({ app, page }) => {
  await app.start(
    scenario({
      tools: oldTools,
      snapshot: snapshot({ projects: [ready()] }),
      responses: {
        submit_check: [tooOld, ok(markerless)],
        upgrade_tool: [ok(updated)],
      },
    }),
  );
  // An outdated gh-student shows the Setup section.
  await expect(page.getByText("Setup", { exact: true })).toBeVisible();
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  await app.expectDialog([/v1\.52\.2 is installed/], "ok");
  expect((await app.waitForCall("upgrade_tool"))[0].args).toEqual({ tool: "gh-student" });
  // Then the usual confirm, after checking again.
  await app.expectDialog([/This sends your committed work in/], "cancel");
  expect(await app.calls("submit_check")).toHaveLength(2);
  expect(await app.writes()).toEqual(["upgrade_tool"]);
  await expect(page.getByText("Setup", { exact: true })).toHaveCount(0);
});

test("a failed gh-student update says so and submits nothing", async ({ app }) => {
  await app.start(
    scenario({
      tools: oldTools,
      snapshot: snapshot({ projects: [ready()] }),
      responses: {
        submit_check: [tooOld],
        upgrade_tool: [fail("notSignedIn", "not signed in to GitHub")],
      },
    }),
  );
  await app.row("Lab 1").getByRole("button", { name: "Submit" }).click();
  await app.expectDialog([/v1\.52\.2 is installed/], "ok");
  const failed = await app.dialog();
  expect(failed.kind).toBe("error");
  expect(failed.message).toContain("Couldn't update Classroom 50 (gh student).");
  expect(failed.message).toContain("Sign in to GitHub first.");
  await app.answer("ok");
  expect(await app.writes()).toEqual(["upgrade_tool"]);
  expect(await app.calls("submit_project")).toEqual([]);
});
