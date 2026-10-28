// Times follow the OS's 12- or 24-hour clock (`system_hour_cycle`), whatever the language.
import { expect, test } from "./app";
import { project, scenario, settings, snapshot } from "./fixtures";

// Zurich time: 28 October 2026 at 08:00, a deadline the next day at 22:30.
test.use({ timezoneId: "Europe/Zurich" });
const dates = snapshot({
  projects: [project({ slug: "lab1", name: "Lab 1", assignment: { due: "2026-10-29T21:30:00Z" } })],
});

test.beforeEach(async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-28T07:00:00Z") });
});

test("an English UI on a 24-hour clock says 22:30", async ({ app }) => {
  await app.start(scenario({ snapshot: dates, hourCycle: "h23" }));
  const due = app.row("Lab 1").getByText(/^Due /);
  await expect(due).toHaveText("Due tomorrow at 22:30");
  await expect(due).toHaveAttribute("title", /Oct 29\D+22:30$/);
});

test("midnight on a 24-hour clock is 00:05, never 24:05", async ({ app }) => {
  const midnight = snapshot({
    projects: [
      project({ slug: "lab2", name: "Lab 2", assignment: { due: "2026-10-28T23:05:00Z" } }),
    ],
  });
  await app.start(scenario({ snapshot: midnight, hourCycle: "h23" }));
  const due = app.row("Lab 2").getByText(/^Due /);
  await expect(due).toHaveText("Due tomorrow at 00:05");
  await expect(due).toHaveAttribute("title", /Oct 29\D+00:05$/);
});

test("a French UI on a 12-hour clock says 10:30 PM", async ({ app }) => {
  await app.start(
    scenario({ snapshot: dates, hourCycle: "h12", settings: settings({ language: "fr" }) }),
  );
  await expect(app.row("Lab 1").getByText(/^À rendre /)).toHaveText(
    /^À rendre demain à 10:30\sPM$/,
  );
});

test("an unknown clock keeps the language's", async ({ app }) => {
  await app.start(scenario({ snapshot: dates }));
  await expect(app.row("Lab 1").getByText(/^Due /)).toHaveText(/^Due tomorrow at 10:30\sPM$/);
});

test("a clock changed while the app runs applies at once", async ({ app }) => {
  await app.start(scenario({ snapshot: dates }));
  const due = app.row("Lab 1").getByText(/^Due /);
  await expect(due).toHaveText(/^Due tomorrow at 10:30\sPM$/);
  await app.emit("hour-cycle", "h23");
  await expect(due).toHaveText("Due tomorrow at 22:30");
  await app.emit("hour-cycle", null);
  await expect(due).toHaveText(/^Due tomorrow at 10:30\sPM$/);
});
