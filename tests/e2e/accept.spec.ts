// Accept: `gh student accept` never asks, so the app summarizes first, names a group when
// one is needed, then offers to clone.
import { expect, test } from "./app";
import { fail, ok, scenario, snapshot, unaccepted } from "./fixtures";
import type { Unaccepted } from "../../src/lib/api";

const lab3 = unaccepted({ slug: "lab3", name: "Lab 3", feedbackPr: true });
const team = unaccepted({ slug: "proj", name: "Project", mode: "team", teamFormation: "student" });

test("Cancel at the summary accepts nothing", async ({ app }) => {
  await app.start(scenario({ snapshot: snapshot({ unaccepted: [lab3] }) }));
  await app.row("Lab 3").getByRole("button", { name: "Accept" }).click();
  const dialog = await app.dialog();
  expect(dialog.title).toBe("Lab 3");
  expect(dialog.message).toContain(
    "This creates your repository for this assignment in Intro to CS",
  );
  expect(dialog.message).toContain("cs50-school/cs101-lab3-student, private");
  expect(dialog.message).toContain("A Feedback pull request is opened");
  expect(dialog.message).toContain("Then the app offers to clone it.");
  expect(dialog.labels).toEqual({ ok: "Accept", cancel: "Cancel" });
  await app.answer("cancel");
  await expect(app.row("Lab 3")).toContainText("Not accepted");
  expect(await app.writes()).toEqual([]);
});

test("accepting runs gh student accept, then offers to clone; Later clones nothing", async ({
  app,
}) => {
  await app.start(
    scenario({
      snapshot: snapshot({ unaccepted: [lab3] }),
      responses: {
        accept_assignment: [
          ok({ repo: "cs50-school/cs101-lab3-student", projectId: "p-lab3", joinedGroup: false }),
        ],
      },
    }),
  );
  await app.row("Lab 3").getByRole("button", { name: "Accept" }).click();
  await app.answer("ok");
  expect((await app.waitForCall("accept_assignment"))[0].args).toEqual({
    id: lab3.id,
    teamName: null,
  });

  const clone = await app.dialog();
  expect(clone.message).toBe(
    "cs50-school/cs101-lab3-student is ready. Clone it now into ~/Classroom/cs101-lab3-student?",
  );
  expect(clone.labels).toEqual({ yes: "Clone", no: "Choose another folder…", cancel: "Later" });
  await app.answer("cancel");
  expect(await app.writes()).toEqual(["accept_assignment"]);
});

test("a team assignment asks the group's name with the summary, founds it, then clones", async ({
  app,
}) => {
  await app.start(
    scenario({
      snapshot: snapshot({ unaccepted: [team] }),
      responses: {
        accept_assignment: [
          ok({ repo: "cs50-school/cs101-proj-group-4", projectId: "p-proj", joinedGroup: false }),
        ],
        clone_project: [ok("/Users/student/Classroom/cs101-proj-group-4")],
      },
    }),
  );
  await app.row("Project").getByRole("button", { name: "Accept" }).click();

  // One step: the summary is the prompt asking the new group's name, required.
  await expect(app.prompt.getByRole("heading")).toHaveText("Project");
  await expect(app.prompt).toContainText("Team assignment: your group shares one repository.");
  await expect(app.prompt).toContainText("Name your new group below");
  expect(await app.pendingDialogs()).toEqual([]);
  const field = app.prompt.getByRole("textbox", { name: "Name your group" });
  const accept = app.prompt.getByRole("button", { name: "Accept" });
  await expect(accept).toBeDisabled();
  await field.fill("   ");
  await expect(accept).toBeDisabled();
  await field.fill("  The Sharks ");
  await accept.click();

  const accepts = await app.waitForCall("accept_assignment");
  expect(accepts.map((call) => call.args)).toEqual([{ id: team.id, teamName: "The Sharks" }]);

  // Founded: adding teammates is offered first; Later keeps it for the menu.
  const founded = await app.expectDialog(
    [/^The Sharks is created and you're its founder\./],
    "cancel",
  );
  expect(founded.labels).toEqual({ ok: "Add teammates", cancel: "Later" });

  await app.expectDialog([/Clone it now into ~\/Classroom\/cs101-proj-group-4\?/], "yes");
  const clone = await app.waitForCall("clone_project");
  expect(clone[0].args).toEqual({ id: "p-proj", parent: "/Users/student/Classroom" });
  // The new clone opens in the default editor.
  const opened = await app.waitForCall("open_in_editor");
  expect(opened[0].args).toEqual({
    path: "/Users/student/Classroom/cs101-proj-group-4",
    editor: "vscode",
  });
  expect(await app.writes()).toEqual(["accept_assignment", "clone_project"]);
});

test("cancelling the group name accepts nothing", async ({ app }) => {
  await app.start(scenario({ snapshot: snapshot({ unaccepted: [team] }) }));
  await app.row("Project").getByRole("button", { name: "Accept" }).click();
  await app.prompt.getByRole("button", { name: "Cancel" }).click();
  await expect(app.prompt).toHaveCount(0);
  expect(await app.calls("accept_assignment")).toEqual([]);
  expect(await app.pendingDialogs()).toEqual([]);
});

test("a teammate's group joined since the last sync founds nothing", async ({ app }) => {
  await app.start(
    scenario({
      snapshot: snapshot({ unaccepted: [team] }),
      responses: {
        accept_assignment: [
          ok({ repo: "cs50-school/cs101-proj-group-2", projectId: "p-proj", joinedGroup: true }),
        ],
      },
    }),
  );
  await app.row("Project").getByRole("button", { name: "Accept" }).click();
  await app.prompt.getByRole("textbox").fill("The Sharks");
  await app.prompt.getByRole("button", { name: "Accept" }).click();

  // gh-student accepted the existing group's repo: no "created" note, straight to the clone.
  await app.expectDialog([/Clone it now into ~\/Classroom\/cs101-proj-group-2\?/], "cancel");
  expect(await app.writes()).toEqual(["accept_assignment"]);
});

test("a group the last sync listed but gone asks for a new name on needsTeam", async ({ app }) => {
  const listed: Unaccepted = {
    ...team,
    group: {
      org: "cs50-school",
      slug: "classroom50-group-abc123-2",
      number: 2,
      name: "Old group",
      members: [],
      founders: null,
      url: "https://github.com/orgs/cs50-school/teams/classroom50-group-abc123-2",
      classroom: null,
      assignment: null,
    },
  };
  await app.start(
    scenario({
      snapshot: snapshot({ unaccepted: [listed] }),
      responses: {
        accept_assignment: [
          fail("needsTeam"),
          ok({ repo: "cs50-school/cs101-proj-group-4", projectId: null, joinedGroup: false }),
        ],
      },
    }),
  );
  await app.row("Project").getByRole("button", { name: "Accept" }).click();
  await app.expectDialog([/This creates your repository/], "ok");

  await expect(app.prompt.getByRole("heading")).toHaveText("Name your group");
  await expect(app.prompt).toContainText("You aren't in a group for this assignment yet.");
  await app.prompt.getByRole("textbox").fill("The Sharks");
  await app.prompt.getByRole("button", { name: "Create group" }).click();

  const accepts = await app.waitForCall("accept_assignment", 2);
  expect(accepts.map((call) => call.args)).toEqual([
    { id: listed.id, teamName: null },
    { id: listed.id, teamName: "The Sharks" },
  ]);
  await app.expectDialog([/^The Sharks is created and you're its founder\./], "cancel");
});

test("a failed accept shows gh-student's own words", async ({ app }) => {
  await app.start(
    scenario({
      snapshot: snapshot({ unaccepted: [lab3] }),
      responses: { accept_assignment: [fail("student", "the classroom key is wrong")] },
    }),
  );
  await app.row("Lab 3").getByRole("button", { name: "Accept" }).click();
  await app.answer("ok");
  const dialog = await app.expectDialog(
    ["The assignment couldn't be accepted: the classroom key is wrong"],
    "ok",
  );
  expect(dialog.kind).toBe("error");
});

test("a locked assignment can't be accepted from the row", async ({ app }) => {
  const locked = unaccepted({ slug: "old", name: "Old Lab", locked: true });
  await app.start(scenario({ snapshot: snapshot({ unaccepted: [locked] }) }));
  await expect(app.row("Old Lab").getByRole("button", { name: "Accept" })).toBeDisabled();
  const menu = await app.rowMenu("Old Lab");
  expect(menu.items.find((item) => item.text === "Accept")?.enabled).toBe(false);
});
