// Settings > Tools: always there, with each tool's version, Install for a missing one,
// Update for an outdated one (gh installs again, gh-student upgrades) or a newer gh-student,
// Check again, and a job Rust is running (maybe started elsewhere) with its bar and Cancel.
import { expect, test, type App } from "./app";
import { fail, hold, ok, scenario, tools } from "./fixtures";
import type { Reply, Scenario } from "./fixtures";
import type { ToolStatus } from "../../src/lib/api";

async function openTools(app: App, over: Partial<Scenario> = {}) {
  await app.start(scenario(over));
  await app.openSettings();
  const section = app.page
    .locator("section")
    .filter({ has: app.page.getByRole("heading", { name: "Tools", exact: true }) });
  await expect(section).toBeVisible();
  return section;
}

/** A tool's row by its exact name ("Git" isn't "GitHub CLI"). */
const row = (app: App, name: string) =>
  app.page
    .locator(".tool")
    .filter({ hasText: new RegExp(`^\\s*${name.replace(/[()]/g, "\\$&")}(,|\\s|$)`) });

function withTool(tool: ToolStatus["tool"], over: Partial<ToolStatus>): ToolStatus[] {
  return tools().map((status) => (status.tool === tool ? { ...status, ...over } : status));
}

test("every tool is listed with its version, and nothing to do", async ({ app }) => {
  const section = await openTools(app);
  await expect(row(app, "Git")).toContainText("2.50.1");
  await expect(row(app, "GitHub CLI")).toContainText("2.83.0");
  await expect(row(app, "Classroom 50 (gh student)")).toContainText("v1.4.0");
  // Only the versions, which open their notes.
  await expect(section.locator(".tool button:not(.version)")).toHaveCount(0);
  // Settings asks whether a newer gh-student exists; git and gh are the package manager's.
  expect((await app.calls("tool_update")).map((c) => c.args)).toEqual([{ tool: "gh-student" }]);
});

test("Check again reads the statuses again and shows them", async ({ app }) => {
  const section = await openTools(app, { tools: tools(["gh"]) });
  await expect(row(app, "GitHub CLI").getByRole("button", { name: "Install" })).toBeVisible();
  const before = (await app.calls("tool_statuses")).length;
  await app.respond("tool_statuses", ok(tools()));
  await section.getByRole("button", { name: "Check again" }).click();
  await app.waitForCall("tool_statuses", before + 1);
  await expect(row(app, "GitHub CLI")).toContainText("2.83.0");
  await expect(row(app, "GitHub CLI").locator("button:not(.version)")).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
});

test("Check again spins until the versions and the newer-version lookup are both in", async ({
  app,
}) => {
  const section = await openTools(app);
  const check = section.getByRole("button", { name: "Check again" });
  // Opening Settings asks for a newer gh-student once; wait for it to settle.
  await app.waitForCall("tool_update");
  await expect(check).toHaveAttribute("aria-busy", "false");
  const lookups = (await app.calls("tool_update")).length;
  await app.respond("tool_statuses", hold());
  await app.respond("tool_update", hold());
  await check.click();
  await expect(check).toHaveAttribute("aria-busy", "true");
  await expect(check.locator(".spinner")).toBeVisible();
  await expect(check).toBeDisabled();
  // The versions are in; the row now asks GitHub, and the button keeps spinning.
  await app.release("tool_statuses", ok(tools()));
  await app.waitForCall("tool_update", lookups + 1);
  await expect(check).toHaveAttribute("aria-busy", "true");
  await app.release("tool_update", ok(null));
  await expect(check).toHaveAttribute("aria-busy", "false");
  await expect(check.locator(".spinner")).toHaveCount(0);
  await expect(check).toBeEnabled();
  expect(await app.writes()).toEqual([]);
});

test("Install follows the job: bar, Cancel, then the new version", async ({ app, page }) => {
  await openTools(app, { tools: tools(["gh"]), responses: { install_tool: [hold()] } });
  const gh = row(app, "GitHub CLI");
  await gh.getByRole("button", { name: "Install" }).click();
  expect((await app.waitForCall("install_tool"))[0].args).toEqual({ tool: "gh" });
  await expect(gh).toContainText("Installing…");
  // Cancel only works once Rust reports the job.
  await expect(gh.getByRole("button", { name: "Cancel" })).toBeDisabled();
  const bar = gh.getByRole("progressbar", { name: "Installing…" });
  await expect(bar).not.toHaveAttribute("aria-valuenow");

  await app.emit("tool-jobs", [{ tool: "gh", kind: "install", progress: 0.25 }]);
  await expect(bar).toHaveAttribute("aria-valuenow", "25");
  await expect(gh.getByRole("button", { name: "Cancel" })).toBeEnabled();

  // Rust reports the new status everywhere once the job ends.
  await app.respond("tool_statuses", ok(tools()));
  await app.release("install_tool", ok(tools()));
  await app.emit("tool-jobs", []);
  await expect(gh).toContainText("2.83.0");
  await expect(bar).toHaveCount(0);
  await expect(page.getByRole("status")).toHaveText("GitHub CLI installed");
});

test("a job started elsewhere shows on open, and Cancel stops it", async ({ app }) => {
  await openTools(app, {
    tools: tools(["gh-student"]),
    responses: { tool_jobs: [ok([{ tool: "gh-student", kind: "install", progress: 0.6 }])] },
  });
  const student = row(app, "Classroom 50 (gh student)");
  await expect(student.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "60");
  await expect(student.getByRole("button", { name: "Install" })).toHaveCount(0);
  await student.getByRole("button", { name: "Cancel" }).click();
  expect((await app.waitForCall("cancel_tool_job"))[0].args).toEqual({ tool: "gh-student" });
  // Not this row's request: nothing was installed from here.
  expect(await app.writes()).toEqual([]);
});

test("an outdated gh installs again, which upgrades it", async ({ app }) => {
  await openTools(app, {
    tools: withTool("gh", { version: "2.60.0", outdated: true }),
    responses: { install_tool: [ok(tools())] },
  });
  const gh = row(app, "GitHub CLI");
  await expect(gh).toContainText("2.60.0, too old");
  await gh.getByRole("button", { name: "Update" }).click();
  expect((await app.waitForCall("install_tool"))[0].args).toEqual({ tool: "gh" });
  expect(await app.calls("upgrade_tool")).toEqual([]);
  await expect(gh).toContainText("2.83.0");
});

test("an outdated gh-student upgrades, never installs", async ({ app, page }) => {
  await openTools(app, {
    tools: withTool("gh-student", { version: "v1.52.2", outdated: true }),
    responses: {
      upgrade_tool: [
        ok({ tool: "gh-student", installed: true, version: "v1.58.1", outdated: false }),
      ],
    },
  });
  const student = row(app, "Classroom 50 (gh student)");
  await student.getByRole("button", { name: "Update" }).click();
  expect((await app.waitForCall("upgrade_tool"))[0].args).toEqual({ tool: "gh-student" });
  expect(await app.calls("install_tool")).toEqual([]);
  await expect(student).toContainText("v1.58.1");
  await expect(page.getByRole("status")).toHaveText("Classroom 50 (gh student) updated");
});

test("a newer gh-student on GitHub offers Update", async ({ app }) => {
  await openTools(app, {
    responses: {
      // Checked again after the update, when it's the latest.
      tool_update: [ok("v1.60.0"), ok(null)],
      upgrade_tool: [
        ok({ tool: "gh-student", installed: true, version: "v1.60.0", outdated: false }),
      ],
    },
  });
  const student = row(app, "Classroom 50 (gh student)");
  await expect(student).toContainText("v1.4.0 → v1.60.0");
  await student.getByRole("button", { name: "Update" }).click();
  await app.waitForCall("upgrade_tool");
  await expect(student.locator(".meta")).toHaveText("v1.60.0");
  await expect(student.getByRole("button", { name: "Update" })).toHaveCount(0);
  expect(await app.calls("tool_update")).toHaveLength(2);
});

test("a version opens its release notes, the update's when there is one", async ({ app }) => {
  await openTools(app, { responses: { tool_update: [ok("v1.60.0")] } });
  await row(app, "GitHub CLI").getByRole("button", { name: "2.83.0" }).click();
  let opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened[0].args.url).toBe("https://github.com/cli/cli/releases/tag/v2.83.0");

  const student = row(app, "Classroom 50 (gh student)").getByRole("button", {
    name: "v1.4.0 → v1.60.0",
  });
  await expect(student).toHaveAttribute("title", "What's new in v1.60.0");
  await student.click();
  opened = await app.waitForCall("plugin:opener|open_url", 2);
  expect(opened[1].args.url).toBe(
    "https://github.com/foundation50/classroom50/releases/tag/cli-v1.60.0",
  );
  // Git's notes aren't GitHub releases.
  await expect(row(app, "Git").getByRole("button")).toHaveCount(0);
  expect(await app.writes()).toEqual([]);
});

test("a version too old with nothing newer known links nowhere", async ({ app }) => {
  await openTools(app, {
    tools: withTool("gh-student", { version: "v1.52.2", outdated: true }),
    responses: { tool_update: [ok(null)] },
  });
  const student = row(app, "Classroom 50 (gh student)");
  await expect(student.locator(".meta")).toHaveText("v1.52.2, too old");
  await expect(student.getByRole("button")).toHaveText(["Update"]);
});

test("an update signed out says to sign in, without the terminal command", async ({ app }) => {
  await openTools(app, {
    tools: withTool("gh-student", { version: "v1.52.2", outdated: true }),
    responses: { upgrade_tool: [fail("notSignedIn")] },
  });
  await row(app, "Classroom 50 (gh student)").getByRole("button", { name: "Update" }).click();
  const dialog = await app.dialog();
  expect(dialog.kind).toBe("error");
  expect(dialog.message).toBe(
    "Couldn't update Classroom 50 (gh student).\n\nSign in to GitHub first.",
  );
  await app.answer("ok");
});

test("a failed update names the command to run in a terminal", async ({ app }) => {
  await openTools(app, {
    tools: withTool("gh-student", { version: "v1.52.2", outdated: true }),
    responses: { upgrade_tool: [fail("commandFailed", "network is unreachable")] },
  });
  await row(app, "Classroom 50 (gh student)").getByRole("button", { name: "Update" }).click();
  await app.expectDialog(["network is unreachable", "gh extension upgrade gh-student"], "ok");
});

test("a cancelled install says nothing more", async ({ app }) => {
  await openTools(app, { tools: tools(["gh"]), responses: { install_tool: [fail("cancelled")] } });
  const pinned = (await app.calls("set_popover_pinned")).length;
  await row(app, "GitHub CLI").getByRole("button", { name: "Install" }).click();
  // Only the failure branch reads the manual command, and Install only comes back once that
  // branch has returned: a dialog would have pinned the popover before then.
  await app.waitForCall("install_command");
  await expect(row(app, "GitHub CLI").getByRole("button", { name: "Install" })).toBeEnabled();
  expect(await app.calls("set_popover_pinned")).toHaveLength(pinned);
  expect(await app.pendingDialogs()).toEqual([]);
});

const manualInstall: Reply = {
  error: {
    kind: "manualInstall",
    message: "no supported package manager",
    url: "https://github.com/cli/cli#installation",
    command: "sudo apt install gh",
    params: { reason: "noPackageManager" },
  },
};

test("ManualInstall offers the download page, worded by reason", async ({ app }) => {
  await openTools(app, { tools: tools(["gh"]), responses: { install_tool: [manualInstall] } });
  await row(app, "GitHub CLI").getByRole("button", { name: "Install" }).click();
  const dialog = await app.dialog();
  expect(dialog.message).toContain(
    "The toolbox can't install GitHub CLI on this computer: no supported package manager found (apt, dnf, pacman, zypper).",
  );
  expect(dialog.message).toContain("sudo apt install gh");
  expect(dialog.labels).toEqual({ ok: "Open download page", cancel: "Not now" });
  await app.answer("ok");
  const opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened[0].args.url).toBe("https://github.com/cli/cli#installation");
});

test("gh-student on Windows on Arm points to the x64 GitHub CLI", async ({ app }) => {
  const windowsArm: Reply = {
    error: {
      kind: "manualInstall",
      message: "gh-student has no Windows on Arm version yet",
      url: "https://github.com/cli/cli/releases/latest",
      command:
        "winget uninstall --id GitHub.cli --exact\nwinget install --id GitHub.cli --exact --source winget --architecture x64",
      params: { reason: "windowsArm" },
    },
  };
  await openTools(app, {
    tools: tools(["gh-student"]),
    responses: { install_tool: [windowsArm] },
  });
  await row(app, "Classroom 50 (gh student)").getByRole("button", { name: "Install" }).click();
  const dialog = await app.dialog();
  expect(dialog.message).toContain(
    "The toolbox can't install Classroom 50 (gh student) on this computer: gh-student has no Windows on Arm version yet; it works with the x64 GitHub CLI instead, which Windows runs emulated.",
  );
  expect(dialog.message).toContain("--architecture x64");
  await app.answer("ok");
  const opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened[0].args.url).toBe("https://github.com/cli/cli/releases/latest");
});

test("ManualInstall, Not now: no page opens", async ({ app }) => {
  await openTools(app, { tools: tools(["gh"]), responses: { install_tool: [manualInstall] } });
  await row(app, "GitHub CLI").getByRole("button", { name: "Install" }).click();
  await app.answer("cancel");
  expect(await app.calls("plugin:opener|open_url")).toEqual([]);
});
