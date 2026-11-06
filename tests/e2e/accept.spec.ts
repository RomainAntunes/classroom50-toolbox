// Accept: `gh student accept` never asks, so the app summarizes first and names a group when
// one is needed, then offers to add teammates to a group just founded. Cloning stays the
// row's own button: nothing asks about it.
import { expect, test, type App } from "./app";
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
  // Cloning is the row's own button now: the summary promises no question about it.
  expect(dialog.message).not.toContain("clone");
  expect(dialog.labels).toEqual({ ok: "Accept", cancel: "Cancel" });
  await app.answer("cancel");
  await expect(app.row("Lab 3")).toContainText("Not accepted");
  expect(await app.writes()).toEqual([]);
});

/** Waits for the accept to be announced, which comes after every question it asks. */
async function accepted(app: App, name: string) {
  await expect(app.page.getByRole("status")).toHaveText(`Accepted: ${name}`);
}

test("accepting runs gh student accept and asks nothing after, not even to clone", async ({
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
  await accepted(app, "Lab 3");
  expect(await app.pendingDialogs()).toEqual([]);
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual(["accept_assignment"]);
});

test("founding a group goes straight to adding teammates, Later adds nobody", async ({ app }) => {
  await app.start(
    scenario({
      snapshot: snapshot({ unaccepted: [team] }),
      responses: {
        accept_assignment: [
          ok({ repo: "cs50-school/cs101-proj-group-4", projectId: "p-proj", joinedGroup: false }),
        ],
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

  // Founded: the teammates prompt says so itself, with no dialog before it.
  await expect(app.prompt.getByRole("heading")).toHaveText("Add to my group");
  await expect(app.prompt).toContainText(
    "The Sharks is created and you're its founder. Classmates you add join it and can push to Project.",
  );
  expect(await app.pendingDialogs()).toEqual([]);
  await app.prompt.getByRole("button", { name: "Later" }).click();
  await accepted(app, "Project");
  expect(await app.pendingDialogs()).toEqual([]);
  expect(await app.writes()).toEqual(["accept_assignment"]);
});

test("adding teammates right after founding queues them, and the accept flow goes on at once", async ({
  app,
}) => {
  await app.start(
    scenario({
      snapshot: snapshot({
        unaccepted: [{ ...team, assignment: { ...team.assignment, maxGroupSize: 3 } }],
      }),
      responses: {
        accept_assignment: [
          ok({ repo: "cs50-school/cs101-proj-group-4", projectId: null, joinedGroup: false }),
        ],
      },
    }),
  );
  await app.row("Project").getByRole("button", { name: "Accept" }).click();
  await app.prompt.getByRole("textbox", { name: "Name your group" }).fill("The Sharks");
  await app.prompt.getByRole("button", { name: "Accept" }).click();

  await expect(app.prompt.getByRole("heading")).toHaveText("Add to my group");
  // The founder alone so far: two more fit in a group of 3.
  const field = app.prompt.getByRole("textbox", { name: "GitHub usernames" });
  await expect(field).toHaveAttribute("placeholder", "alice, bob");
  await expect(app.prompt).toContainText("Your group has room for 2 more.");
  await field.fill("bob carol");
  await expect(app.prompt).toContainText("Adds bob and carol.");
  await app.prompt.getByRole("button", { name: "Add" }).click();
  const added = await app.waitForCall("add_teammates");
  // Queued on the accepted assignment: no sync has listed the group's repo yet.
  expect(added[0].args).toEqual({ id: team.id, usernames: ["bob", "carol"] });
  // No wait for gh, no results dialog: the repo isn't listed yet, which the last note says.
  await app.expectDialog([/cs50-school\/cs101-proj-group-4/], "ok");
  expect(await app.writes()).toEqual(["accept_assignment", "add_teammates"]);
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

  // gh-student accepted the existing group's repo: no "created" note, no teammates prompt.
  await accepted(app, "Project");
  await expect(app.prompt).toHaveCount(0);
  expect(await app.pendingDialogs()).toEqual([]);
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
  await expect(app.prompt.getByRole("heading")).toHaveText("Add to my group");
  await expect(app.prompt).toContainText("The Sharks is created and you're its founder.");
  await app.prompt.getByRole("button", { name: "Later" }).click();
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
