// The Setup section: shown while a tool is missing; installs report progress through
// `tool-jobs` and failures through a dialog.
import { expect, test } from "./app";
import { fail, hold, ok, scenario, tools } from "./fixtures";

test("no Setup section when every tool is installed", async ({ app, page }) => {
  await app.start(scenario());
  await expect(page.getByRole("searchbox")).toBeVisible();
  await expect(page.getByText("Setup", { exact: true })).toHaveCount(0);
});

test("a missing tool shows Setup, and Install follows tool-jobs to the end", async ({
  app,
  page,
}) => {
  await app.start(
    scenario({ tools: tools(["gh-student"]), responses: { install_tool: [hold()] } }),
  );
  await expect(page.getByText("Setup", { exact: true })).toBeVisible();
  const row = page.locator(".tool").filter({ hasText: "Classroom 50 (gh student)" });
  await row.getByRole("button", { name: "Install" }).click();

  const call = await app.waitForCall("install_tool");
  expect(call[0].args).toEqual({ tool: "gh-student" });
  await expect(row.getByText("Installing…")).toBeVisible();
  const bar = row.getByRole("progressbar");
  await expect(bar).toBeVisible();
  // Indeterminate until a step shows up.
  await expect(bar).not.toHaveAttribute("aria-valuenow");

  await app.emit("tool-jobs", [{ tool: "gh-student", kind: "install", progress: 0.4 }]);
  await expect(bar).toHaveAttribute("aria-valuenow", "40");
  await expect(row.getByRole("button", { name: "Cancel" })).toBeEnabled();

  await app.emit("tool-jobs", [{ tool: "gh-student", kind: "install", progress: 0.9 }]);
  await expect(bar).toHaveAttribute("aria-valuenow", "90");

  await app.respond("tool_statuses", ok(tools()));
  await app.release("install_tool", ok(tools()));
  await app.emit("tool-jobs", []);
  // Everything installed: the section goes away.
  await expect(page.getByText("Setup", { exact: true })).toHaveCount(0);
});

test("Cancel asks Rust to stop the job", async ({ app, page }) => {
  await app.start(scenario({ tools: tools(["gh"]), responses: { install_tool: [hold()] } }));
  const row = page.locator(".tool").filter({ hasText: "GitHub CLI" });
  await row.getByRole("button", { name: "Install" }).click();
  await app.emit("tool-jobs", [{ tool: "gh", kind: "install", progress: null }]);
  await row.getByRole("button", { name: "Cancel" }).click();
  expect((await app.waitForCall("cancel_tool_job"))[0].args).toEqual({ tool: "gh" });
  // A cancelled install says nothing more.
  await app.release("install_tool", fail("cancelled"));
  await app.emit("tool-jobs", []);
  await expect(row.getByRole("button", { name: "Install" })).toBeVisible();
  expect(await app.pendingDialogs()).toEqual([]);
});

test("an outdated gh-student shows Setup, and Update upgrades it instead of installing", async ({
  app,
  page,
}) => {
  const old = tools().map((tool) =>
    tool.tool === "gh-student" ? { ...tool, version: "v1.52.2", outdated: true } : tool,
  );
  const updated = { tool: "gh-student", installed: true, version: "v1.58.1", outdated: false };
  await app.start(scenario({ tools: old, responses: { upgrade_tool: [ok(updated)] } }));
  await expect(page.getByText("Setup", { exact: true })).toBeVisible();
  const row = page.locator(".tool").filter({ hasText: "Classroom 50 (gh student)" });
  await expect(row.getByText("v1.52.2, too old")).toBeVisible();
  await row.getByRole("button", { name: "Update" }).click();
  // `gh extension install` refuses an installed extension: it's upgraded.
  expect((await app.waitForCall("upgrade_tool"))[0].args).toEqual({ tool: "gh-student" });
  expect(await app.calls("install_tool")).toEqual([]);
  await expect(page.getByText("Setup", { exact: true })).toHaveCount(0);
});

test("gh-student waits for gh", async ({ app, page }) => {
  await app.start(scenario({ tools: tools(["gh", "gh-student"]) }));
  const row = page.locator(".tool").filter({ hasText: "Classroom 50 (gh student)" });
  await expect(row.getByText("Install the GitHub CLI first")).toBeVisible();
  await expect(row.getByRole("button", { name: "Install" })).toBeDisabled();
});

test("a failed install shows the command's words and the manual command", async ({ app, page }) => {
  await app.start(
    scenario({
      tools: tools(["gh"]),
      responses: {
        install_tool: [fail("commandFailed", "brew: formula not found")],
        install_command: [
          ok({ program: "brew", args: ["install", "gh"], manual: "brew install gh" }),
        ],
      },
    }),
  );
  await page
    .locator(".tool")
    .filter({ hasText: "GitHub CLI" })
    .getByRole("button", { name: "Install" })
    .click();
  const dialog = await app.dialog();
  expect(dialog.kind).toBe("error");
  expect(dialog.message).toContain("Couldn't install GitHub CLI.");
  expect(dialog.message).toContain("brew: formula not found");
  expect(dialog.message).toContain("brew install gh");
  await app.answer("ok");
});

test("no package manager: the download page is offered, worded by reason", async ({
  app,
  page,
}) => {
  await app.start(
    scenario({
      tools: tools(["gh"]),
      responses: {
        install_tool: [
          {
            error: {
              kind: "manualInstall",
              message: "Homebrew isn't installed",
              url: "https://github.com/cli/cli/releases",
              command: "brew install gh",
              params: { reason: "noHomebrew" },
            },
          },
        ],
        install_command: [ok({ program: "brew", args: [], manual: "brew install gh" })],
      },
    }),
  );
  await page
    .locator(".tool")
    .filter({ hasText: "GitHub CLI" })
    .getByRole("button", { name: "Install" })
    .click();
  const dialog = await app.dialog();
  expect(dialog.message).toContain(
    "can't install GitHub CLI on this computer: Homebrew isn't installed",
  );
  expect(dialog.labels).toEqual({ ok: "Open download page", cancel: "Not now" });
  await app.answer("ok");
  const opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened[0].args.url).toBe("https://github.com/cli/cli/releases");
});
