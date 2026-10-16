// `classroom50-toolbox://` links: any web page can fire one, so nothing runs (not even a
// request) before the student says yes.
import { expect, test } from "./app";
import { ok, project, scenario, settings, snapshot, tools, unaccepted } from "./fixtures";
import type { DeepLinkTarget } from "../../src/lib/api";

const link = {
  kind: "accept",
  org: "cs50-school",
  classroom: "cs101",
  assignment: "lab9",
  key: null,
} as const;
const unlisted: DeepLinkTarget = { kind: "unlisted", link };
const lab9 = unaccepted({ slug: "lab9", name: "Lab 9" });

test("an unlisted class asks first; only then is its manifest read", async ({ app }) => {
  await app.start(
    scenario({
      deepLink: unlisted,
      responses: {
        fetch_linked_assignment: [ok({ kind: "accept", item: lab9, listed: false })],
      },
    }),
  );
  const ask = await app.dialog();
  expect(ask.message).toBe(
    "Open the assignment lab9 of cs50-school/cs101? The app then reads the class's published assignment list.",
  );
  expect(ask.labels).toEqual({ ok: "Open", cancel: "Cancel" });
  expect(await app.calls("fetch_linked_assignment")).toEqual([]);

  await app.answer("ok");
  await app.waitForCall("fetch_linked_assignment");
  // Then the usual accept, which asks again before anything is created.
  await app.expectDialog([/This creates your repository for this assignment/], "cancel");
  await app.waitForCall("deep_link_done");
  expect(await app.writes()).toEqual([]);
});

test("saying no to an unlisted class requests nothing", async ({ app }) => {
  await app.start(scenario({ deepLink: unlisted }));
  await app.answer("cancel");
  await app.waitForCall("deep_link_done");
  expect(await app.calls("fetch_linked_assignment")).toEqual([]);
  expect(await app.writes()).toEqual([]);
});

test("a link arriving while the app runs goes through the deep-link event", async ({
  app,
  page,
}) => {
  await app.start(scenario());
  await expect(page.getByRole("searchbox")).toBeVisible();
  await app.respond("take_deep_link", ok(unlisted), ok(null));
  await app.emit("deep-link", null);
  await app.expectDialog([/Open the assignment lab9/], "cancel");
  expect(await app.calls("fetch_linked_assignment")).toEqual([]);
});

test("a link before setup is finished only says so", async ({ app }) => {
  await app.start(scenario({ deepLink: unlisted, settings: settings({ onboarded: false }) }));
  await app.expectDialog(["Finish setting up the app first, then open the link again."], "ok");
  await app.waitForCall("deep_link_done");
  expect(await app.calls("fetch_linked_assignment")).toEqual([]);
});

test("a link needing gh-student says the tools are missing", async ({ app }) => {
  await app.start(scenario({ deepLink: unlisted, tools: tools(["gh-student"]) }));
  const dialog = await app.expectDialog([/needs the tools listed under Setup/], "ok");
  expect(dialog.kind).toBe("warning");
  expect(await app.calls("fetch_linked_assignment")).toEqual([]);
});

test("a link while signed out asks to sign in first", async ({ app }) => {
  await app.start(
    scenario({
      deepLink: unlisted,
      auth: { loggedIn: false, login: null, missingScopes: [], envToken: null },
    }),
  );
  await app.expectDialog([/^Sign in to GitHub first/], "ok");
  expect(await app.calls("fetch_linked_assignment")).toEqual([]);
});

test("an open link to a project without a clone highlights it and offers to clone", async ({
  app,
}) => {
  const remoteOnly = project({ slug: "lab2", name: "Lab 2", local: null });
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [remoteOnly] }),
      deepLink: {
        kind: "project",
        id: remoteOnly.id,
        link: { kind: "open", owner: "cs50-school", repo: "cs101-lab2-student" },
      },
    }),
  );
  const offer = await app.dialog();
  expect(offer.message).toBe(
    "cs50-school/cs101-lab2-student isn't on this computer yet. Clone it now into ~/Classroom/cs101-lab2-student?",
  );
  await expect(app.row("Lab 2")).toHaveClass(/linked/);
  await expect(app.row("Lab 2").locator("[data-row]")).toHaveAttribute("aria-expanded", "true");
  await app.answer("cancel");
  expect(await app.writes()).toEqual([]);
});
