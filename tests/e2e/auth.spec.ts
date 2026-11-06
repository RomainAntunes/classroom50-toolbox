// Signing in through gh's device flow: the code arrives as `login-device-code`, the app
// opens GitHub, and the result (or the failure) shows in the footer. Every sign-in first
// asks HTTPS or SSH (gh's current protocol first) and checks SSH when picked.
import { expect, test, type App } from "./app";
import { fail, failPlugin, hold, ok, pass, scenario, snapshot } from "./fixtures";

/** Answers the sign-in question (HTTPS by default, gh's protocol in the scenarios). */
async function pickProtocol(app: App, label = "HTTPS") {
  const dialog = await app.dialog();
  expect(dialog.title).toBe("How should git reach GitHub?");
  expect(dialog.message).toContain("gh in a terminal uses it too, for every account");
  await app.answer(label);
}

const signedOut = {
  loggedIn: false,
  login: null,
  missingScopes: [],
  envToken: null,
  canAddSshKeys: false,
};
const signedIn = {
  loggedIn: true,
  login: "student",
  missingScopes: [],
  envToken: null,
  canAddSshKeys: false,
};
const code = { code: "ABCD-1234", url: "https://github.com/login/device" };

test("the device code shows, opens GitHub, and success fills the list", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: signedOut,
      snapshot: snapshot({ login: null }),
      responses: { login: [hold()] },
    }),
  );
  const footer = page.locator("footer");
  await expect(footer).toContainText("Not signed in to GitHub");
  await footer.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app);
  const [call] = await app.waitForCall("login");
  expect(call.args).toEqual({ protocol: "https", sshKey: false });

  await app.emit("login-device-code", code);
  await expect(footer.locator("code")).toHaveText("ABCD-1234");
  await expect(footer).toContainText("Or open https://github.com/login/device in a browser");
  await footer.getByRole("button", { name: "Copy code and open GitHub" }).click();
  const opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened[0].args.url).toBe("https://github.com/login/device");

  await app.release("login", ok(signedIn));
  await expect(footer.getByRole("button", { name: "Signed in as student" })).toBeVisible();
  await expect(footer.locator("code")).toHaveCount(0);
  await app.waitForCall("refresh_projects");
  // The browser took focus, so success is also a system notification.
  await expect
    .poll(() => app.notifications())
    .toEqual([
      {
        title: "Signed in to GitHub",
        body: "Classroom 50 Toolbox is now connected as student.",
      },
    ]);
});

test("no browser: the URL is spelled out", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: signedOut,
      responses: { login: [hold()], "plugin:opener|open_url": [failPlugin("no xdg-open")] },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app);
  await app.emit("login-device-code", code);
  await page.getByRole("button", { name: "Copy code and open GitHub" }).click();
  await expect(page.locator("footer .url.error")).toHaveText(
    "No browser could be opened. Open https://github.com/login/device yourself and type the code.",
  );
});

test("Cancel stops gh and says nothing more", async ({ app, page }) => {
  await app.start(scenario({ auth: signedOut, responses: { login: [hold()] } }));
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app);
  await app.emit("login-device-code", code);
  await page.locator("footer").getByRole("button", { name: "Cancel" }).click();
  await app.waitForCall("cancel_login");
  await app.release("login", fail("cancelled"));
  await app.emit("login-ended", null);
  await expect(page.getByRole("button", { name: "Sign in" })).toBeEnabled();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("an expired code is worded", async ({ app, page }) => {
  await app.start(scenario({ auth: signedOut, responses: { login: [fail("timedOut")] } }));
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app);
  await expect(page.getByRole("alert")).toHaveText(
    "The code expired. Sign in again to get a new one.",
  );
});

test("a token from the environment names the variable", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: { ...signedOut, envToken: "GH_TOKEN" },
      responses: { login: [fail("tokenFromEnv")] },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "The GitHub CLI is using the GH_TOKEN environment variable",
  );
  // gh can't store a sign-in: nothing is asked, probed or fixed first.
  expect(await app.pendingDialogs()).toEqual([]);
  expect(await app.calls("ssh_status")).toEqual([]);
  expect(await app.writes()).toEqual([]);
});

test("an unknown failure shows gh's own words", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: signedOut,
      responses: { login: [fail("commandFailed", "gh: HTTP 502 from api.github.com")] },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app);
  await expect(page.getByRole("alert")).toHaveText("gh: HTTP 502 from api.github.com");
});

test("missing scopes offer Grant access through the same flow", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: { ...signedIn, missingScopes: ["admin:org", "workflow"] },
      responses: { grant_scopes: [hold()] },
    }),
  );
  await expect(page.locator("footer")).toContainText("needs more access to your GitHub account");
  await page.getByRole("button", { name: "Grant access" }).click();
  const [call] = await app.waitForCall("grant_scopes");
  expect(call.args).toEqual({ extra: [] });
  // No protocol question: the account and its protocol stay.
  expect(await app.pendingDialogs()).toEqual([]);
  expect(await app.calls("login")).toEqual([]);
  await app.emit("login-device-code", code);
  await expect(page.locator("footer code")).toHaveText("ABCD-1234");
});

test("signing out is confirmed first; Cancel keeps the account", async ({ app, page }) => {
  await app.start(scenario());
  await page.getByRole("button", { name: "Signed in as student" }).click();
  const menu = await app.menu();
  expect(menu.items.map((item) => item.text)).toEqual([
    "student",
    "Separator",
    "Add another account…",
    "Sign out of student",
  ]);
  await app.pickMenu("Sign out of student");
  const dialog = await app.expectDialog([/The terminal is signed out too\./], "cancel");
  expect(dialog.kind).toBe("warning");
  expect(await app.writes()).toEqual([]);
});

test("cancelling the protocol question signs nothing in", async ({ app, page }) => {
  await app.start(scenario({ auth: signedOut, responses: { login: [hold()] } }));
  await page.getByRole("button", { name: "Sign in" }).click();
  const dialog = await app.dialog();
  expect(dialog.labels).toEqual({ yes: "HTTPS", no: "SSH", cancel: "Cancel" });
  await app.answer("cancel");
  await expect(page.getByRole("button", { name: "Sign in" })).toBeEnabled();
  expect(await app.writes()).toEqual([]);
  expect(await app.calls("ssh_status")).toEqual([]);
});

test("SSH that works signs in with SSH", async ({ app, page }) => {
  await app.start(scenario({ auth: signedOut, responses: { login: [ok(signedIn)] } }));
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  const [probe] = await app.waitForCall("ssh_status");
  expect(probe.args).toEqual({ fresh: true });
  const [call] = await app.waitForCall("login");
  expect(call.args).toEqual({ protocol: "ssh", sshKey: false });
  await expect(page.getByRole("button", { name: "Signed in as student" })).toBeVisible();
  expect(await app.writes()).toEqual(["login"]);
});

test("gh's protocol comes first; SSH blocked by the network falls back to HTTPS", async ({
  app,
  page,
}) => {
  await app.start(
    scenario({
      auth: signedOut,
      gitProtocol: "ssh",
      ssh: { state: "unreachable", message: "connect to host github.com port 22: timed out" },
      responses: { login: [ok(signedIn)] },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  const question = await app.dialog();
  expect(question.labels).toEqual({ yes: "SSH", no: "HTTPS", cancel: "Cancel" });
  await app.answer("yes");
  const blocked = await app.expectDialog(
    ["Can't reach GitHub over SSH: this network may block it. HTTPS still works."],
    "Use HTTPS",
  );
  expect(blocked.labels).toEqual({ yes: "Use SSH anyway", no: "Use HTTPS", cancel: "Cancel" });
  const [call] = await app.waitForCall("login");
  expect(call.args).toEqual({ protocol: "https", sshKey: false });
});

test("no SSH key: signs in with the key scope, then adds the key", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: signedOut,
      ssh: { state: "noKey", localKey: null },
      responses: { login: [ok({ ...signedIn, canAddSshKeys: true })] },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  await app.expectDialog(
    ["No SSH key of this computer is on your GitHub account.", /without a passphrase/],
    "Sign in and set up SSH",
  );
  const [call] = await app.waitForCall("login");
  expect(call.args).toEqual({ protocol: "ssh", sshKey: true });
  await app.waitForCall("set_up_ssh_key");
  expect(await app.writes()).toEqual(["login", "set_up_ssh_key"]);
});

test("Cancel at the missing key signs nothing in", async ({ app, page }) => {
  await app.start(scenario({ auth: signedOut, ssh: { state: "noKey", localKey: null } }));
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  await app.expectDialog(["No SSH key"], "cancel");
  await expect(page.getByRole("button", { name: "Sign in" })).toBeEnabled();
  expect(await app.writes()).toEqual([]);
});

test("an unknown GitHub host key is trusted first, then SSH signs in", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: signedOut,
      ssh: { state: "hostKeyUnknown" },
      responses: { login: [ok(signedIn)] },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  await app.expectDialog(
    ["doesn't know GitHub's SSH host key yet", "~/.ssh/known_hosts"],
    "Trust GitHub and continue",
  );
  const [call] = await app.waitForCall("login");
  expect(call.args).toEqual({ protocol: "ssh", sshKey: false });
  expect(await app.writes()).toEqual(["trust_github_host", "login"]);
});

test("Add another account asks the same question", async ({ app, page }) => {
  await app.start(scenario({ responses: { login: [hold()] } }));
  await page.getByRole("button", { name: "Signed in as student" }).click();
  await app.pickMenu("Add another account…");
  await pickProtocol(app);
  const [call] = await app.waitForCall("login");
  expect(call.args).toEqual({ protocol: "https", sshKey: false });
});

test("Add another account checks SSH again as the new account and says whose key it is", async ({
  app,
  page,
}) => {
  const second = { ...signedIn, login: "second" };
  await app.start(
    scenario({
      responses: {
        login: [ok(second)],
        // Before the sign-in SSH can only answer for gh's account so far; after it, for the
        // new one.
        ssh_status: [
          ok({ state: "ready", login: "student" }),
          ok({ state: "otherAccount", login: "student", active: "second" }),
        ],
      },
    }),
  );
  await page.getByRole("button", { name: "Signed in as student" }).click();
  await app.pickMenu("Add another account…");
  await pickProtocol(app, "SSH");
  const [call] = await app.waitForCall("login");
  expect(call.args).toEqual({ protocol: "ssh", sshKey: false });
  const said = await app.expectDialog(["signs in to GitHub as student, not second"], "cancel");
  expect(said.labels).toEqual({ ok: "SSH settings…", cancel: "Later" });
  expect((await app.calls("ssh_status")).map((c) => c.args)).toEqual([
    { fresh: true },
    { fresh: true },
  ]);
  expect(await app.writes()).toEqual(["login"]);
});

test("a key on the account that ssh didn't use is explained, never uploaded", async ({
  app,
  page,
}) => {
  await app.start(
    scenario({
      auth: signedOut,
      responses: {
        login: [ok({ ...signedIn, canAddSshKeys: true })],
        // Signed out, the key can't be compared with the account's yet.
        ssh_status: [
          ok({ state: "noKey", localKey: "~/.ssh/id_ed25519.pub" }),
          ok({ state: "keyNotOffered", localKey: "~/.ssh/id_ed25519.pub" }),
        ],
      },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  await app.expectDialog(["No SSH key"], "Sign in and set up SSH");
  await app.expectDialog(["~/.ssh/id_ed25519.pub is on GitHub", "ssh-add"], "cancel");
  await expect(page.getByRole("button", { name: "Signed in as student" })).toBeVisible();
  expect(await app.calls("set_up_ssh_key")).toEqual([]);
  expect(await app.writes()).toEqual(["login"]);
});

for (const [kind, params, words] of [
  ["blockedInDev", undefined, "Couldn't set up SSH: blockedInDev (from Rust)"],
  [
    "ssh",
    { problem: "keyInUse" },
    "Couldn't set up SSH: this SSH key is already on another GitHub account",
  ],
] as const) {
  test(`a sign-in stands when setting up the key fails (${kind})`, async ({ app, page }) => {
    await app.start(
      scenario({
        auth: signedOut,
        ssh: { state: "noKey", localKey: null },
        responses: {
          login: [ok({ ...signedIn, canAddSshKeys: true })],
          set_up_ssh_key: [fail(kind, `${kind} (from Rust)`, params)],
        },
      }),
    );
    await page.getByRole("button", { name: "Sign in" }).click();
    await pickProtocol(app, "SSH");
    await app.expectDialog(["No SSH key"], "Sign in and set up SSH");
    const dialog = await app.expectDialog([words], null);
    expect(dialog.kind).toBe("error");
    await expect(page.getByRole("button", { name: "Signed in as student" })).toBeVisible();
    expect(await app.writes()).toEqual(["login", "set_up_ssh_key"]);
    expect(await app.pendingDialogs()).toEqual([]);
  });
}

test("GitHub wanting the key scope at set-up asks for it once, then adds the key", async ({
  app,
  page,
}) => {
  const needsScope = fail("ssh", "needs scope (from Rust)", { problem: "needsScope" });
  await app.start(
    scenario({
      auth: signedOut,
      ssh: { state: "noKey", localKey: null },
      responses: {
        login: [ok({ ...signedIn, canAddSshKeys: true })],
        set_up_ssh_key: [needsScope, pass()],
        grant_scopes: [ok({ ...signedIn, canAddSshKeys: true })],
      },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  await app.expectDialog(["No SSH key"], "Sign in and set up SSH");
  const [granted] = await app.waitForCall("grant_scopes");
  expect(granted.args).toEqual({ extra: ["writePublicKey"] });
  await expect
    .poll(() => app.writes())
    .toEqual(["login", "set_up_ssh_key", "grant_scopes", "set_up_ssh_key"]);
  await expect(page.locator("footer")).not.toContainText("Setting up SSH");
  expect(await app.pendingDialogs()).toEqual([]);
});

test("GitHub refusing the key scope twice is said, not asked again", async ({ app, page }) => {
  const needsScope = fail("ssh", "needs scope (from Rust)", { problem: "needsScope" });
  await app.start(
    scenario({
      auth: signedOut,
      ssh: { state: "noKey", localKey: null },
      responses: {
        login: [ok({ ...signedIn, canAddSshKeys: true })],
        set_up_ssh_key: [needsScope],
        grant_scopes: [ok({ ...signedIn, canAddSshKeys: true })],
      },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  await app.expectDialog(["No SSH key"], "Sign in and set up SSH");
  await app.expectDialog(["Couldn't set up SSH: adding an SSH key needs access"], null);
  expect(await app.writes()).toEqual(["login", "set_up_ssh_key", "grant_scopes", "set_up_ssh_key"]);
});

test("a key set up that SSH still doesn't use is said, not announced as done", async ({
  app,
  page,
}) => {
  await app.start(
    scenario({
      auth: signedOut,
      ssh: { state: "noKey", localKey: "~/.ssh/id_ed25519.pub" },
      responses: {
        login: [ok({ ...signedIn, canAddSshKeys: true })],
        set_up_ssh_key: [ok({ state: "keyNotOffered", localKey: "~/.ssh/id_ed25519.pub" })],
      },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  await app.expectDialog(["No SSH key"], "Sign in and set up SSH");
  await app.expectDialog(["is on GitHub, but ssh didn't use it", "ssh-add"], "SSH settings…");
  await expect(page).toHaveURL(/\/settings#ssh$/);
  // On the tab that holds the row.
  await expect(page.getByRole("tab", { name: "Tools" })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#ssh")).toBeFocused();
  await expect(page.locator(".sr-only[role=status]")).not.toContainText("SSH is set up");
});

test("a sign-in still setting up SSH can't be started twice", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: signedOut,
      ssh: { state: "noKey", localKey: null },
      responses: {
        login: [ok({ ...signedIn, canAddSshKeys: true })],
        set_up_ssh_key: [hold()],
      },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app, "SSH");
  await app.expectDialog(["No SSH key"], "Sign in and set up SSH");
  await app.waitForCall("set_up_ssh_key");
  // gh's flow ended before the set-up did: the sign-in still runs.
  await app.emit("login-ended", null);
  const footer = page.locator("footer");
  await expect(footer).toContainText("Setting up SSH…");
  await footer.getByRole("button", { name: "Signed in as student" }).click();
  const menu = await app.menu();
  expect(menu.items.find((i) => i.text === "Add another account…")?.enabled).toBe(false);
  await app.release("set_up_ssh_key", ok({ state: "ready", login: "student" }));
  await expect(footer).not.toContainText("Setting up SSH…");
  expect(await app.writes()).toEqual(["login", "set_up_ssh_key"]);
});

test("a cancelled sign-in's end doesn't wipe the next one's code", async ({ app, page }) => {
  await app.start(scenario({ auth: signedOut, responses: { login: [hold(), hold()] } }));
  const footer = page.locator("footer");
  await footer.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app);
  await app.waitForCall("login");
  await app.emit("login-device-code", code);
  await footer.getByRole("button", { name: "Cancel" }).click();
  await app.waitForCall("cancel_login");
  await footer.getByRole("button", { name: "Sign in" }).click();
  await pickProtocol(app);
  await expect.poll(async () => (await app.calls("login")).length).toBe(2);
  await app.emit("login-device-code", { ...code, code: "WXYZ-9876" });
  // The first gh only now reports it was cancelled.
  await app.release("login", fail("cancelled"));
  await expect(footer.locator("code")).toHaveText("WXYZ-9876");
  await expect(footer.getByRole("alert")).toHaveCount(0);
});
