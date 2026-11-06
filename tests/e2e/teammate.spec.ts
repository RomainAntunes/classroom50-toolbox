// Adding classmates from a row's menu: the in-popover prompt takes several usernames at once
// (commas or spaces, `@` dropped) and `add_teammates` queues them all and returns at once.
// Rust adds them in the background and sends `teammate-jobs`: the row shows each username
// waiting, being added, added or why not, with "Add another" at any time and Done once the
// queue is empty. On a team assignment only the group's founder can add members, which the
// menu says up front instead of letting gh fail.
import { expect, test, type App } from "./app";
import { LOGIN, fail, project, scenario, snapshot } from "./fixtures";
import type { Group, Project, TeammateEntry, TeammateJob } from "../../src/lib/api";
import type { Reply } from "./fixtures";

const solo = project({ slug: "lab1", name: "Lab 1" });
const REPO = `${solo.owner}/${solo.repo}`;

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

/** One username's line, as Rust sends it. */
function entry(username: string, state: TeammateEntry["state"], error?: string): TeammateEntry {
  return { username, state, error: error ? { kind: "student", message: error } : null };
}

/** The queue of `row` as Rust sends it in `teammate-jobs`. */
function job(
  row: Project,
  entries: TeammateEntry[],
  extra: Partial<TeammateJob> = {},
): TeammateJob {
  return {
    id: row.id,
    project: row.id,
    repo: `${row.owner}/${row.repo}`,
    assignment: row.assignment!.name,
    team: false,
    running: entries.some((e) => e.state === "waiting" || e.state === "adding"),
    entries,
    ...extra,
  };
}

/** Boots on `row` and waits for the first read of the queues, which an event sent earlier
 *  would race. */
async function startOn(app: App, row: Project) {
  await app.start(scenario({ snapshot: snapshot({ projects: [row] }) }));
  await app.waitForCall("teammate_jobs");
  await expect(app.page.getByRole("button", { name: row.assignment!.name })).toBeVisible();
}

/** The row's progress list. */
const progress = (app: App, label = "Invite a classmate") =>
  app.page.getByRole("region", { name: label });

/** Unfolds the progress list, which starts folded to its counts. */
const unfold = (list: ReturnType<typeof progress>) =>
  list.getByRole("button", { expanded: false }).click();

test("the usernames are queued at once and the row shows how each one goes", async ({ app }) => {
  await open(app, solo, "Invite a classmate…");
  await expect(app.prompt.getByRole("heading")).toHaveText("Invite a classmate");
  await expect(app.prompt).toContainText(
    `Each classmate gets a GitHub invitation to push to ${REPO}.`,
  );
  const field = app.prompt.getByRole("textbox", { name: "GitHub usernames" });
  await expect(field).toHaveAttribute("placeholder", "alice, bob");
  await expect(app.prompt).toContainText("Separate several usernames with commas or spaces.");
  await field.fill("@alice, bob  carol;@Alice");
  await expect(app.prompt).toContainText("Invites alice, bob, and carol.");
  await app.prompt.getByRole("button", { name: "Invite" }).click();

  const calls = await app.waitForCall("add_teammates");
  expect(calls).toHaveLength(1);
  expect(calls[0].args).toEqual({ id: solo.id, usernames: ["alice", "bob", "carol"] });
  // Nothing waits on gh: the prompt is gone and no dialog is up.
  await expect(app.prompt).toHaveCount(0);
  expect(await app.pendingDialogs()).toEqual([]);

  await app.emit("teammate-jobs", [
    job(solo, [entry("alice", "adding"), entry("bob", "waiting"), entry("carol", "waiting")]),
  ]);
  const list = progress(app);
  await unfold(list);
  await expect(list.getByRole("listitem")).toHaveText([
    "alice Inviting…",
    "bob Waiting",
    "carol Waiting",
  ]);
  await expect(list).toHaveAttribute("aria-busy", "true");
  await expect(list.getByRole("button", { name: "Add another" })).toBeVisible();
  await expect(list.getByRole("button", { name: "Done" })).toHaveCount(0);

  await app.emit("teammate-jobs", [
    job(solo, [
      entry("alice", "added"),
      entry("bob", "failed", "bob isn't a GitHub user"),
      entry("carol", "added"),
    ]),
  ]);
  await expect(list.getByRole("listitem")).toHaveText([
    "alice Invited",
    "bob Couldn't add · bob isn't a GitHub user",
    "carol Invited",
  ]);
  // No dialog says how it went: screen readers hear it.
  await expect(app.page.getByRole("status")).toHaveText(
    [
      `alice got an invitation to push to ${REPO}.`,
      "Couldn't add bob: bob isn't a GitHub user",
      `carol got an invitation to push to ${REPO}.`,
    ].join("\n"),
  );
  expect(await app.pendingDialogs()).toEqual([]);

  await list.getByRole("button", { name: "Done" }).click();
  expect((await app.waitForCall("dismiss_teammate_job"))[0].args).toEqual({ id: solo.id });
  await app.emit("teammate-jobs", []);
  await expect(list).toHaveCount(0);
  expect(await app.writes()).toEqual(["add_teammates"]);
});

test("Add another queues more while the first ones are still being added", async ({ app }) => {
  await startOn(app, solo);
  await app.emit("teammate-jobs", [job(solo, [entry("alice", "adding")])]);
  await unfold(progress(app));
  await progress(app).getByRole("button", { name: "Add another" }).click();

  // A fresh prompt, without waiting for alice.
  await expect(app.prompt.getByRole("heading")).toHaveText("Invite a classmate");
  await expect(app.prompt.getByRole("textbox")).toHaveValue("");
  await app.prompt.getByRole("textbox").fill("dave");
  await app.prompt.getByRole("button", { name: "Invite" }).click();
  const calls = await app.waitForCall("add_teammates");
  expect(calls.map((c) => c.args)).toEqual([{ id: solo.id, usernames: ["dave"] }]);
  await expect(app.prompt).toHaveCount(0);
});

test("the progress starts folded to counts, every classmate named on hover", async ({ app }) => {
  await startOn(app, solo);
  await app.emit("teammate-jobs", [
    job(solo, [
      entry("alice", "added"),
      entry("bob", "failed", "bob isn't a GitHub user"),
      entry("carol", "adding"),
      entry("dave", "waiting"),
    ]),
  ]);
  const list = progress(app);
  const fold = list.getByRole("button", { name: /^Invite a classmate/ });
  await expect(fold).toHaveAttribute("aria-expanded", "false");
  await expect(list.getByRole("listitem")).toHaveCount(0);
  await expect(list.getByRole("button", { name: "Add another" })).toHaveCount(0);
  await expect(fold).toHaveText("Invite a classmate 1 Invited 1 Couldn't add 2 Inviting…");
  await expect(fold).toHaveAttribute("title", "alice, bob, carol, dave");

  // Still folded as the queue moves on.
  await app.emit("teammate-jobs", [
    job(solo, [
      entry("alice", "added"),
      entry("bob", "failed", "bob isn't a GitHub user"),
      entry("carol", "added"),
      entry("dave", "added"),
    ]),
  ]);
  await expect(fold).toHaveText("Invite a classmate 3 Invited 1 Couldn't add");

  await fold.click();
  await expect(fold).toHaveAttribute("aria-expanded", "true");
  await expect(fold).not.toHaveAttribute("title");
  await expect(list.getByRole("listitem")).toHaveCount(4);
  await expect(list.getByRole("button", { name: "Done" })).toBeVisible();
  await fold.click();
  await expect(list.getByRole("listitem")).toHaveCount(0);
  expect(await app.calls("dismiss_teammate_job")).toEqual([]);
});

test("a call Rust refuses (nothing queued) is an error for everyone", async ({ app }) => {
  await open(app, solo, "Invite a classmate…", {
    add_teammates: [fail("student", "the classroom is closed")],
  });
  await app.prompt.getByRole("textbox").fill("alice bob");
  await app.prompt.getByRole("button", { name: "Invite" }).click();
  const dialog = await app.dialog();
  expect(dialog.title).toBe("Invite a classmate");
  expect(dialog.kind).toBe("error");
  expect(dialog.message).toBe("Couldn't add alice and bob: the classroom is closed");
  await app.answer("ok");
});

test("added while the popover was hidden: the notification says who made it", async ({
  app,
  page,
}) => {
  await app.start(scenario({ snapshot: snapshot({ projects: [solo] }) }));
  // The layout listens once mounted; an event before that is lost.
  await expect(page.getByRole("searchbox")).toBeVisible();
  await app.emit("notify", {
    kind: "teammatesAdded",
    assignment: "Project",
    team: true,
    added: ["alice", "carol"],
    failed: ["bob"],
  });
  await app.emit("notify", {
    kind: "teammatesAdded",
    assignment: "Lab 1",
    team: false,
    added: [],
    failed: ["dave"],
  });
  await expect
    .poll(() => app.notifications())
    .toEqual([
      {
        title: "Classmates added: Project",
        body: "In your group now: alice and carol\nCouldn't add: bob",
      },
      { title: "Couldn't add classmates: Lab 1", body: "Couldn't add: dave" },
    ]);
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
  const row = teamProject([LOGIN]);
  await open(app, row, "Add to my group…");
  await expect(app.prompt.getByRole("heading")).toHaveText("Add to my group");
  await expect(app.prompt).toContainText(
    "Classmates you add join your group and can push to Project.",
  );
  await app.prompt.getByRole("textbox").fill("bob");
  await app.prompt.getByRole("button", { name: "Add" }).click();
  expect((await app.waitForCall("add_teammates"))[0].args).toEqual({
    id: row.id,
    usernames: ["bob"],
  });
  await app.emit("teammate-jobs", [job(row, [entry("bob", "adding")], { team: true })]);
  const list = progress(app, "Add to my group");
  await unfold(list);
  await expect(list.getByRole("listitem")).toHaveText(["bob Adding…"]);
  await app.emit("teammate-jobs", [job(row, [entry("bob", "added")], { team: true })]);
  await expect(list.getByRole("listitem")).toHaveText(["bob In your group"]);
  await expect(list).toContainText("Once added, each teammate accepts Project too.");
  await expect(app.page.getByRole("status")).toHaveText(
    "bob is in your group now. They should accept Project next.",
  );
});

test("a group just founded: its queue moves to the repo's row once a sync lists it", async ({
  app,
}) => {
  const row = teamProject([LOGIN]);
  await startOn(app, row);
  // Queued from the accepted assignment's id, before the repo was listed.
  await app.emit("teammate-jobs", [
    job(row, [entry("bob", "waiting")], { id: "cs50-school/cs101/proj", team: true }),
  ]);
  await unfold(progress(app, "Add to my group"));
  await expect(progress(app, "Add to my group").getByRole("listitem")).toHaveText(["bob Waiting"]);
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
