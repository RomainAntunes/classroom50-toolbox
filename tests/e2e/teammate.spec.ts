// Adding classmates from a row's menu: the in-popover prompt takes several usernames at once
// (commas or spaces, `@` dropped), `add_teammates` runs once with all of them, and the
// results dialog offers "Add another" until Done. On a team assignment only the group's
// founder can add members, which the menu says up front instead of letting gh fail.
import { expect, test, type App } from "./app";
import { LOGIN, hold, ok, fail, project, scenario, snapshot } from "./fixtures";
import type { Group, Project } from "../../src/lib/api";
import type { Reply } from "./fixtures";

const solo = project({ slug: "lab1", name: "Lab 1" });

function group(founders: string[] | null): Group {
  return {
    org: "cs50-school",
    slug: "classroom50-group-abc123-4",
    number: 4,
    name: "Team Rocket",
    members: [{ login: LOGIN, name: null, email: null }],
    founders,
    url: "https://github.com/orgs/cs50-school/teams/classroom50-group-abc123-4",
    classroom: null,
    assignment: null,
  };
}

const teamProject = (founders: string[] | null, teamFormation: "student" | "teacher" = "student") =>
  project({
    slug: "proj",
    name: "Project",
    assignment: { mode: "team", teamFormation },
    group: group(founders),
  });

async function open(app: App, row: Project, item: string, responses: Record<string, Reply[]> = {}) {
  await app.start(scenario({ snapshot: snapshot({ projects: [row] }), responses }));
  await app.rowMenu(row.assignment!.name);
  await app.pickMenu(item);
}

test("several usernames go in at once; add_teammates runs once with all of them", async ({
  app,
}) => {
  await open(app, solo, "Invite a classmate…", {
    add_teammates: [
      ok([
        { username: "alice", error: null },
        { username: "bob", error: { kind: "student", message: "bob isn't a GitHub user" } },
        { username: "carol", error: null },
      ]),
    ],
  });
  await expect(app.prompt.getByRole("heading")).toHaveText("Invite a classmate");
  await expect(app.prompt).toContainText(
    "They get a GitHub invitation to push to cs50-school/cs101-lab1-student.",
  );
  await expect(app.prompt.getByRole("textbox")).toHaveAttribute("placeholder", "GitHub usernames");
  await app.prompt.getByRole("textbox").fill("@alice, bob  carol;@Alice");
  await app.prompt.getByRole("button", { name: "Invite" }).click();

  const calls = await app.waitForCall("add_teammates");
  expect(calls).toHaveLength(1);
  expect(calls[0].args).toEqual({ id: solo.id, usernames: ["alice", "bob", "carol"] });

  const results = await app.dialog();
  expect(results.title).toBe("Invite a classmate");
  // Some went through: not an error.
  expect(results.kind).toBe("info");
  expect(results.message).toBe(
    [
      "alice got an invitation to push to cs50-school/cs101-lab1-student.",
      "Couldn't add bob: bob isn't a GitHub user",
      "carol got an invitation to push to cs50-school/cs101-lab1-student.",
    ].join("\n"),
  );
  expect(results.labels).toEqual({ ok: "Add another", cancel: "Done" });
  await app.answer("cancel");
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual(["add_teammates"]);
});

test("Add another asks again, until Done", async ({ app }) => {
  await open(app, solo, "Invite a classmate…", {
    add_teammates: [
      ok([{ username: "alice", error: null }]),
      ok([{ username: "dave", error: null }]),
    ],
  });
  await app.prompt.getByRole("textbox").fill("alice");
  await app.prompt.getByRole("button", { name: "Invite" }).click();
  await app.answer("ok");

  // A fresh prompt for the next ones.
  await expect(app.prompt).toBeVisible();
  await expect(app.prompt.getByRole("textbox")).toHaveValue("");
  await app.prompt.getByRole("textbox").fill("dave");
  await app.prompt.getByRole("button", { name: "Invite" }).click();
  await app.expectDialog(["dave got an invitation"], "cancel");

  const calls = await app.calls("add_teammates");
  expect(calls.map((c) => c.args.usernames)).toEqual([["alice"], ["dave"]]);
  await expect(app.prompt).toHaveCount(0);
});

test("a failure for everyone is an error, worded per username", async ({ app }) => {
  await open(app, solo, "Invite a classmate…", {
    add_teammates: [fail("student", "the classroom is closed")],
  });
  await app.prompt.getByRole("textbox").fill("alice bob");
  await app.prompt.getByRole("button", { name: "Invite" }).click();
  const results = await app.dialog();
  expect(results.kind).toBe("error");
  expect(results.message).toBe(
    "Couldn't add alice: the classroom is closed\nCouldn't add bob: the classroom is closed",
  );
  await app.answer("cancel");
});

test("cancelling the prompt adds nobody", async ({ app }) => {
  await open(app, solo, "Invite a classmate…");
  await app.prompt.getByRole("textbox").fill("alice");
  await app.prompt.getByRole("button", { name: "Cancel" }).click();
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
  expect(await app.pendingDialogs()).toEqual([]);
});

test("only separators and @ add nobody", async ({ app }) => {
  await open(app, solo, "Invite a classmate…");
  await app.prompt.getByRole("textbox").fill(" @ ,, ; ");
  await app.prompt.getByRole("button", { name: "Invite" }).click();
  await expect(app.prompt).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
});

test("the group's founder adds members to the group", async ({ app }) => {
  await open(app, teamProject([LOGIN]), "Add to my group…", {
    add_teammates: [hold()],
  });
  await expect(app.prompt.getByRole("heading")).toHaveText("Add to my group");
  await expect(app.prompt).toContainText("They get push access to Project through your group.");
  await app.prompt.getByRole("textbox").fill("bob");
  await app.prompt.getByRole("button", { name: "Add" }).click();
  expect((await app.waitForCall("add_teammates"))[0].args).toEqual({
    id: teamProject([LOGIN]).id,
    usernames: ["bob"],
  });
  await app.release("add_teammates", ok([{ username: "bob", error: null }]));
  await app.expectDialog(["bob is in your group now. They should accept Project next."], "cancel");
});

for (const [why, row, words] of [
  [
    "another student founded the group",
    teamProject(["leader"]),
    "Only the group's founder can add members: ask leader to add your classmates.",
  ],
  [
    "the teacher forms the groups",
    teamProject([], "teacher"),
    "Your teacher assigns the groups for this assignment: ask them to add classmates to yours.",
  ],
  [
    "the group has no founder",
    teamProject([]),
    "Only the group's founder can add members, and you didn't found this group.",
  ],
] as const) {
  test(`when ${why}, the menu says so up front and asks for nobody`, async ({ app }) => {
    await open(app, row, "Add to my group…");
    const dialog = await app.dialog();
    expect(dialog.title).toBe("Add to my group");
    expect(dialog.kind).toBe("info");
    expect(dialog.message).toBe(words);
    await app.answer("ok");
    await expect(app.prompt).toHaveCount(0);
    expect(await app.writes()).toEqual([]);
  });
}

test("roles not read yet: the prompt shows and gh decides", async ({ app }) => {
  await open(app, teamProject(null), "Add to my group…");
  await expect(app.prompt.getByRole("heading")).toHaveText("Add to my group");
  await app.prompt.getByRole("button", { name: "Cancel" }).click();
  expect(await app.writes()).toEqual([]);
});

test("a closed assignment offers no way to add classmates", async ({ app }) => {
  await app.start(
    scenario({
      snapshot: snapshot({
        projects: [project({ slug: "lab1", name: "Lab 1", assignment: { closed: true } })],
      }),
    }),
  );
  const menu = await app.rowMenu("Lab 1");
  expect(menu.items.find((i) => i.text === "Invite a classmate…")?.enabled).toBe(false);
});
