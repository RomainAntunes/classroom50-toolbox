// The tray menu's deadlines: the nearest ones of unfinished work, worded by the hour, and a
// click (`tray-action` `focus`, after Rust showed the popover) that pins and opens the row.
import { expect, test, type App } from "./app";
import { classroom, project, scenario, settings, snapshot, unaccepted } from "./fixtures";
import type { TrayMenu } from "../../src/lib/api";

// Zurich time: 28 October 2026, the app started at 15:00.
test.use({ timezoneId: "Europe/Zurich" });
const SYNCED = "2026-10-28T13:32:00Z";

// ICU puts a narrow no-break space before "PM".
const plain = (text: string) => text.replace(/\s/g, " ");

/** Every menu sent so far, the last one last. */
async function menus(app: App): Promise<TrayMenu[]> {
  return (await app.calls("set_tray_menu")).map((call) => call.args.menu as TrayMenu);
}

async function deadlines(app: App) {
  return (await menus(app)).at(-1)?.deadlines.map((d) => ({ ...d, label: plain(d.label) }));
}

const math = classroom({ slug: "math", name: "Math" });
// Not cloned, so it needs work; due at 6 PM today.
const lab3 = project({
  slug: "lab3",
  name: "Lab 3",
  local: null,
  assignment: { due: "2026-10-28T17:00:00Z" },
});
// Cloned and clean: done, so never listed.
const lab4 = project({ slug: "lab4", name: "Lab 4", assignment: { due: "2026-10-28T18:00:00Z" } });
// In a folded class, due tomorrow at 9 AM.
const proof = unaccepted({
  slug: "proof",
  name: "Proof 1",
  classroom: math,
  due: "2026-10-29T08:00:00Z",
});
// Was due this morning.
const quiz = unaccepted({ slug: "quiz", name: "Quiz", due: "2026-10-28T08:00:00Z" });

const world = scenario({
  snapshot: snapshot({
    projects: [lab3, lab4],
    unaccepted: [proof, quiz],
    syncedAt: SYNCED,
    checkedAt: SYNCED,
  }),
  settings: settings({ collapsedClasses: ["cs50-school/math"] }),
});

test.beforeEach(async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-28T14:00:00Z") });
});

test("the menu lists the nearest deadlines of unfinished work", async ({ app }) => {
  await app.start(world);
  await expect
    .poll(() => deadlines(app))
    .toEqual([
      { id: quiz.id, label: "Quiz · Was due today at 9:00 AM" },
      { id: lab3.id, label: "Lab 3 · Due today at 6:00 PM" },
      { id: proof.id, label: "Proof 1 · Due tomorrow at 9:00 AM" },
    ]);
});

test("a passing deadline changes its line once Rust says the menu expired", async ({ app }) => {
  await app.start(world);
  await expect
    .poll(async () => (await deadlines(app))?.[1].label)
    .toBe("Lab 3 · Due today at 6:00 PM");
  // Valid until Lab 3's deadline, 6 PM in Zurich.
  expect((await menus(app)).at(-1)?.validUntil).toBe("2026-10-28T17:00:00.000Z");
  const sent = (await menus(app)).length;

  // Past the deadline the UI sends nothing by itself; Rust's tick asks.
  await app.emit("popover-hidden", null);
  await app.page.clock.runFor(3.1 * 3_600_000);
  expect(await menus(app)).toHaveLength(sent);
  await app.emit("tray-expired", null);
  await expect
    .poll(async () => (await deadlines(app))?.[1].label)
    .toBe("Lab 3 · Was due today at 6:00 PM");
});

test("a project's deadline opens its row, from any page", async ({ app, page }) => {
  await app.start(world, "/settings");
  await expect(page.locator("main section").first()).toBeVisible();
  await app.emit("tray-action", { kind: "focus", id: lab3.id });
  await expect(page).toHaveURL(/\/$/);
  await expect(app.row("Lab 3")).toHaveClass(/linked/);
  await expect(app.row("Lab 3").locator("[data-row]")).toHaveAttribute("aria-expanded", "true");
  expect(await app.writes()).toEqual([]);
});

test("an unaccepted assignment's deadline opens its row in a folded class", async ({ app }) => {
  await app.start(world);
  await expect(app.page.getByRole("searchbox")).toBeVisible();
  await expect(app.row("Proof 1")).toHaveCount(0);
  await app.emit("tray-action", { kind: "focus", id: proof.id });
  await expect(app.row("Proof 1")).toHaveClass(/linked/);
  await expect(app.row("Proof 1").locator("[data-row]")).toHaveAttribute("aria-expanded", "true");
  expect(await app.writes()).toEqual([]);
});
