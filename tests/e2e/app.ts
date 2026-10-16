// The Playwright side of the harness: `test` with an `app` fixture that boots the real UI on
// a scenario and drives the fake Rust side (`harness/browser.ts`). After each test it fails
// on any command the fake doesn't know, so a new command gets a mock (and a scenario).
import { test as base, expect, type Page } from "@playwright/test";
import { harnessScript } from "./harness/bundle";
import type { Call, MenuNode, PendingDialog } from "./harness/browser";
import type { Reply, Scenario } from "./fixtures";

export { expect };

/** Commands that write to GitHub, a clone or the machine: a flow the student cancels must
 *  invoke none of them. */
export const WRITES = [
  "accept_assignment",
  "submit_project",
  "commit_push",
  "commit_only",
  "clone_project",
  "pull_project",
  "add_teammates",
  "set_git_identity",
  "install_tool",
  "upgrade_tool",
  "install_update",
  "login",
  "grant_scopes",
  "sign_out",
  "switch_account",
];

export class App {
  constructor(readonly page: Page) {}

  /** Boots the app at `path` with the fake Rust side answering from `scenario`. */
  async start(scenario: Scenario, path = "/"): Promise<void> {
    const harness = await harnessScript();
    // One script: the order of several init scripts isn't defined.
    await this.page.addInitScript({
      content: `${harness}\n__E2E_HARNESS__.install(${JSON.stringify(scenario)});`,
    });
    await this.page.goto(path);
  }

  /** Every recorded invoke, or those of `cmd`. */
  async calls(cmd?: string): Promise<Call[]> {
    const calls = await this.page.evaluate(() => window.__E2E__.calls);
    return cmd ? calls.filter((call) => call.cmd === cmd) : calls;
  }

  /** The commands invoked, in order, plugin calls aside. */
  async commands(): Promise<string[]> {
    return (await this.calls()).map((c) => c.cmd).filter((cmd) => !cmd.startsWith("plugin:"));
  }

  /** Waits until `cmd` was invoked (at least `times` times) and returns its calls. */
  async waitForCall(cmd: string, times = 1): Promise<Call[]> {
    await expect.poll(async () => (await this.calls(cmd)).length).toBeGreaterThanOrEqual(times);
    return this.calls(cmd);
  }

  /** The `WRITES` invoked so far, in order. */
  async writes(): Promise<string[]> {
    return (await this.commands()).filter((cmd) => WRITES.includes(cmd));
  }

  async unmocked(): Promise<string[]> {
    return this.page.evaluate(() => window.__E2E__.unmocked);
  }

  async notifications() {
    return this.page.evaluate(() => window.__E2E__.notifications);
  }

  /** Settings as Rust holds them now. */
  async settings() {
    return this.page.evaluate(() => window.__E2E__.settings);
  }

  /** Sends a Rust event (`tool-jobs`, `login-device-code`, `projects-updated`...). */
  async emit(event: string, payload: unknown): Promise<void> {
    await this.page.evaluate(([e, p]) => window.__E2E__.emit(e, p), [event, payload] as const);
  }

  /** How `cmd` answers from now on (replies in order, the last one repeating). */
  async respond(cmd: string, ...replies: Reply[]): Promise<void> {
    await this.page.evaluate(([c, r]) => window.__E2E__.respond(c, r), [cmd, replies] as const);
  }

  /** Settles the oldest held call of `cmd`. */
  async release(cmd: string, reply: Reply): Promise<void> {
    await this.page.evaluate(([c, r]) => window.__E2E__.release(c, r), [cmd, reply] as const);
  }

  /** Waits for the next native dialog and returns it, still open. */
  async dialog(): Promise<PendingDialog> {
    await this.page.waitForFunction(() => window.__E2E__.pendingDialogs().length > 0);
    return (await this.page.evaluate(() => window.__E2E__.pendingDialogs()))[0];
  }

  /** Answers the open dialog: `ok`, `cancel`, `yes`, `no` or a label; a path for pickers. */
  async answer(choice: string | null): Promise<void> {
    await this.dialog();
    await this.page.evaluate((c) => window.__E2E__.answerDialog(c), choice);
  }

  /** Waits for the next dialog, checks its text holds every `parts`, and answers it. */
  async expectDialog(parts: (string | RegExp)[], choice: string | null): Promise<PendingDialog> {
    const dialog = await this.dialog();
    for (const part of parts) expect(dialog.message ?? "").toMatch(part);
    await this.answer(choice);
    return dialog;
  }

  /** Dialogs still waiting for an answer. */
  async pendingDialogs(): Promise<PendingDialog[]> {
    return this.page.evaluate(() => window.__E2E__.pendingDialogs());
  }

  /** Waits for a native menu to pop up and returns it (its items, nested). */
  async menu(): Promise<MenuNode> {
    await this.page.waitForFunction(() => window.__E2E__.menus().length > 0);
    const menus = await this.page.evaluate(() => window.__E2E__.menus());
    return menus[menus.length - 1];
  }

  /** Clicks an item of the open native menu, by its text or id. */
  async pickMenu(key: string): Promise<void> {
    await this.menu();
    await this.page.evaluate((k) => window.__E2E__.pickMenu(k), key);
  }

  /** The list row (`<li data-item>`) whose name is `name`. */
  row(name: string) {
    return this.page.locator("li[data-item]").filter({
      has: this.page.locator(".name", { hasText: name }),
    });
  }

  /** Opens a row's native menu with a right click. */
  async rowMenu(name: string): Promise<MenuNode> {
    await this.row(name).locator(".row").click({ button: "right" });
    return this.menu();
  }

  /** Goes to Settings from the main page's gear, and waits for it. */
  async openSettings(): Promise<void> {
    await this.page.locator(".titlebar .icon").last().click();
    await expect(this.page).toHaveURL(/\/settings$/);
    await expect(this.page.locator("main section").first()).toBeVisible();
  }

  /** The in-popover text prompt. */
  get prompt() {
    return this.page.getByRole("dialog");
  }
}

/** Texts of a menu's items, nested ones included. */
export function menuTexts(menu: MenuNode): string[] {
  return menu.items.flatMap((item) => [item.text ?? "", ...menuTexts(item)]);
}

export const test = base.extend<{ app: App }>({
  app: async ({ page }, use) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    const app = new App(page);
    await use(app);
    // A command the fake doesn't know means the test ran on a default it never meant.
    if (page.url().startsWith("http"))
      expect(await app.unmocked(), "commands with no e2e mock").toEqual([]);
    expect(errors, "uncaught errors in the page").toEqual([]);
  },
});
