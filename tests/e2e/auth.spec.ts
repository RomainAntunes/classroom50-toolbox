// Signing in through gh's device flow: the code arrives as `login-device-code`, the app
// opens GitHub, and the result (or the failure) shows in the footer.
import { expect, test } from "./app";
import { fail, failPlugin, hold, ok, scenario, snapshot } from "./fixtures";

const signedOut = { loggedIn: false, login: null, missingScopes: [], envToken: null };
const signedIn = { loggedIn: true, login: "student", missingScopes: [], envToken: null };
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
  await app.waitForCall("login");

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
  await app.emit("login-device-code", code);
  await page.getByRole("button", { name: "Copy code and open GitHub" }).click();
  await expect(page.locator("footer .url.error")).toHaveText(
    "No browser could be opened. Open https://github.com/login/device yourself and type the code.",
  );
});

test("Cancel stops gh and says nothing more", async ({ app, page }) => {
  await app.start(scenario({ auth: signedOut, responses: { login: [hold()] } }));
  await page.getByRole("button", { name: "Sign in" }).click();
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
});

test("an unknown failure shows gh's own words", async ({ app, page }) => {
  await app.start(
    scenario({
      auth: signedOut,
      responses: { login: [fail("commandFailed", "gh: HTTP 502 from api.github.com")] },
    }),
  );
  await page.getByRole("button", { name: "Sign in" }).click();
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
  await app.waitForCall("grant_scopes");
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
