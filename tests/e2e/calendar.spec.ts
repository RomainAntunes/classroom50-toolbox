// Settings > Calendar: Export writes the due dates as an .ics file Rust then keeps updated
// (`calendarFile`); without a calendar store (Windows, Linux with neither GNOME's nor KDE's)
// "Open in Calendar" exports first when needed, then opens the file; on macOS a real
// calendar can be chosen. Cancelling the save dialog exports nothing.
import { expect, test, type App } from "./app";
import {
  HOME,
  VERSION,
  fail,
  ok,
  project,
  scenario,
  settings,
  snapshot,
  unaccepted,
} from "./fixtures";
import type { Scenario } from "./fixtures";

const FILE = `${HOME}/Documents/classroom50.ics`;
const list = snapshot({
  projects: [project({ slug: "lab1", name: "Lab 1" }), project({ slug: "lab2", name: "Lab 2" })],
  unaccepted: [unaccepted({ slug: "lab3", name: "Lab 3" })],
});

async function openCalendar(app: App, over: Partial<Scenario> = {}) {
  await app.start(scenario({ snapshot: list, ...over }));
  await app.openSettings();
  const section = app.page
    .locator("section")
    .filter({ has: app.page.getByRole("heading", { name: "Calendar", exact: true }) });
  await expect(section).toBeVisible();
  return section;
}

/** No calendar store to write into, as on Windows. */
const windows = {
  appInfo: { os: "windows", version: VERSION, nativeCalendar: false, globalShortcuts: true },
} as const;

test("Export asks where, writes the file and keeps it updated", async ({ app }) => {
  const section = await openCalendar(app);
  await section.getByRole("button", { name: "Export..." }).click();

  const save = await app.dialog();
  expect(save.type).toBe("save");
  expect(save.options).toMatchObject({
    defaultPath: "classroom50.ics",
    filters: [{ name: "Calendar", extensions: ["ics"] }],
  });
  await app.answer(FILE);

  expect((await app.waitForCall("export_calendar"))[0].args).toEqual({ path: FILE });
  await expect(section).toContainText("Exported 3 due dates. Open the file to add them");
  // Settings are read again: the file is now kept up to date, and Stop ends that.
  await expect(section).toContainText("Kept up to date after each sync");
  await expect(section.locator(".path")).toHaveText(FILE);
  await section.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(section).not.toContainText("Kept up to date after each sync");
  expect((await app.settings()).calendarFile).toBeNull();
});

test("cancelling the save dialog exports nothing", async ({ app }) => {
  const section = await openCalendar(app);
  await section.getByRole("button", { name: "Export..." }).click();
  await app.answer(null);
  // A second save dialog only opens once the first click's handler has settled, so an export
  // after the cancel would have been called by then.
  await section.getByRole("button", { name: "Export..." }).click();
  expect((await app.dialog()).type).toBe("save");
  expect(await app.calls("export_calendar")).toEqual([]);
  await app.answer(null);
  await expect(section).toContainText("An .ics file for Calendar, Outlook or Google Calendar");
  expect((await app.settings()).calendarFile).toBeNull();
});

test("a failed export shows Rust's words", async ({ app }) => {
  const section = await openCalendar(app, {
    responses: { export_calendar: [fail("io", "Permission denied (os error 13)")] },
  });
  await section.getByRole("button", { name: "Export..." }).click();
  await app.answer(FILE);
  const dialog = await app.expectDialog(["Permission denied (os error 13)"], "ok");
  expect(dialog.kind).toBe("error");
  await expect(section).not.toContainText("Kept up to date after each sync");
});

test("with a calendar store, Open in Calendar isn't offered", async ({ app }) => {
  const section = await openCalendar(app);
  await expect(section).toContainText("Calendar app");
  await expect(section.getByRole("button", { name: "Open in Calendar" })).toHaveCount(0);
});

test("no calendar store and no file yet: Open in Calendar exports, then opens", async ({ app }) => {
  const section = await openCalendar(app, windows);
  await expect(section).not.toContainText("Calendar app");
  await section.getByRole("button", { name: "Open in Calendar" }).click();
  expect((await app.dialog()).type).toBe("save");
  await app.answer(FILE);
  await app.waitForCall("open_calendar_file");
  const order = (await app.commands()).filter((cmd) =>
    ["export_calendar", "open_calendar_file"].includes(cmd),
  );
  expect(order).toEqual(["export_calendar", "open_calendar_file"]);
});

test("Open in Calendar, save cancelled: nothing is exported or opened", async ({ app }) => {
  const section = await openCalendar(app, windows);
  await section.getByRole("button", { name: "Open in Calendar" }).click();
  await app.answer(null);
  // Asked again: the first click's handler has settled by the time the second dialog opens.
  await section.getByRole("button", { name: "Open in Calendar" }).click();
  expect((await app.dialog()).type).toBe("save");
  expect(await app.calls("export_calendar")).toEqual([]);
  expect(await app.calls("open_calendar_file")).toEqual([]);
  await app.answer(null);
});

test("Open in Calendar with a file already kept opens it at once", async ({ app }) => {
  const section = await openCalendar(app, {
    ...windows,
    settings: settings({ calendarFile: FILE }),
  });
  const pinned = (await app.calls("set_popover_pinned")).length;
  await section.getByRole("button", { name: "Open in Calendar" }).click();
  await app.waitForCall("open_calendar_file");
  // Opened again: the first click's handler has returned by the time the second call lands,
  // and any dialog would have pinned the popover first.
  await section.getByRole("button", { name: "Open in Calendar" }).click();
  await app.waitForCall("open_calendar_file", 2);
  expect(await app.calls("export_calendar")).toEqual([]);
  expect(await app.calls("set_popover_pinned")).toHaveLength(pinned);
  expect(await app.pendingDialogs()).toEqual([]);
});

test("a file the calendar app can't open shows Rust's words", async ({ app }) => {
  const section = await openCalendar(app, {
    ...windows,
    settings: settings({ calendarFile: FILE }),
    responses: { open_calendar_file: [fail("io", "No application knows how to open it")] },
  });
  await section.getByRole("button", { name: "Open in Calendar" }).click();
  const dialog = await app.expectDialog(["No application knows how to open it"], "ok");
  expect(dialog.kind).toBe("error");
});

test("Choose... lists the calendars and syncs into the one picked", async ({ app }) => {
  const section = await openCalendar(app, {
    responses: {
      native_calendars: [
        ok([
          { id: "cal-1", title: "Work", account: "iCloud" },
          { id: "cal-2", title: "School", account: "On My Mac" },
        ]),
      ],
      set_native_calendar: [ok(settings({ nativeCalendar: "cal-2" }))],
    },
  });
  await section.getByRole("button", { name: "Choose..." }).click();
  const menu = await app.menu();
  expect(menu.items.map((item) => item.text)).toEqual(["Work (iCloud)", "School (On My Mac)"]);
  await app.pickMenu("School (On My Mac)");
  expect((await app.waitForCall("set_native_calendar"))[0].args).toEqual({ id: "cal-2" });
  await expect(section).toContainText('Due dates are kept in "School (On My Mac)".');
  await expect(section.getByRole("button", { name: "Change..." })).toBeVisible();
});

test("calendar access refused is worded by kind", async ({ app }) => {
  const section = await openCalendar(app, {
    responses: { native_calendars: [fail("calendarAccess")] },
  });
  await section.getByRole("button", { name: "Choose..." }).click();
  await app.expectDialog(["Allow it in System Settings > Privacy & Security > Calendars."], "ok");
  expect(await app.calls("set_native_calendar")).toEqual([]);
});
