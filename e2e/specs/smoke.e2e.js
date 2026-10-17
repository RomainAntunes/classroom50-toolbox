// Smoke tests of the real app against the mock GitHub (`mock/world.js`) and the fake gh
// (`bin/gh`). One file, so one app runs for all of them, in this order: the last one edits
// the fixture clone.

import { appendFileSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const en = JSON.parse(readFileSync(new URL("../../src/lib/i18n/en.json", import.meta.url), "utf8"));
/** English, as the app words it (the run's settings pick `en`). */
function t(key, params = {}) {
  const text = en[key] ?? en[`${key}.other`];
  if (text === undefined) throw new Error(`no string ${key}`);
  return text.replace(/\{(\w+)\}/g, (_, name) => String(params[name]));
}

const HELLO_ID = "c50-e2e-school/algo-hello-ada-e2e";
const clone = process.env.C50_E2E_CLONE ?? "";
const ghLog = process.env.C50_E2E_GH_LOG ?? "";
const mockUrl = process.env.C50_E2E_MOCK ?? "";

/** A row of the list (the `<li>`), by the assignment name its button shows. */
const item = (name) => $(`//li[@data-item][.//button[@data-row][contains(., "${name}")]]`);
const text = async (name) => (await item(name)).getText();
const ghCalls = () => (existsSync(ghLog) ? readFileSync(ghLog, "utf8").split("\n") : []);

describe("Classroom 50 Toolbox", () => {
  before(async () => {
    // The first full sync runs at start; the list shows once it has read GitHub.
    await item("Hello, World").waitForDisplayed({ timeout: 90_000 });
    // The grade comes with the status read that follows discovery.
    await browser.waitUntil(async () => (await text("Hello, World")).includes("7/12"), {
      timeout: 60_000,
      timeoutMsg: "the hello grade never showed",
    });
  });

  it("lists the classes and assignments from the mock GitHub", async () => {
    for (const title of ["Algorithms", "Web Programming"]) {
      await expect($(`//h2[contains(., "${title}")]`)).toBeDisplayed();
    }
    expect(await text("Hello, World")).toContain(t("projects.pending.unsubmitted"));
    // Pushed a day after the deadline: marked late.
    await expect((await item("Sorting")).$(".late")).toBeDisplayed();
    expect(await text("Sorting")).toContain(t("projects.notCloned"));
    // In the manifest, no repo: offered to accept.
    expect(await text("Graphs")).toContain(t("projects.notAccepted"));
    // An `empty_repo` assignment, found by its repo's name.
    expect(await text("Portfolio")).toContain(t("projects.notCloned"));
    // A repo outside Classroom 50 never shows.
    expect(await $("main").getText()).not.toContain("dotfiles");

    // Everything came from the mock, through the hooks.
    const asked = await (await fetch(`${mockUrl}/__requests`)).json();
    const kinds = [...new Set(asked.map((r) => r.kind ?? r.path))];
    for (const kind of ["list", "teams", "details", "status", "releaseBody"]) {
      expect(kinds).toContain(kind);
    }
    expect(kinds).toContain("/pages/c50-e2e-school/classroom50/algo/assignments.json");
    expect(kinds).toContain("/pages/c50-e2e-lab/classroom50/web/assignments.json");
    expect(kinds.some((k) => String(k).endsWith("/result.json"))).toBe(true);
  });

  it("opens a row's details on the failed test's output", async () => {
    const hello = await item("Hello, World");
    await hello.$("button[data-row]").click();
    await expect(hello.$("p.heading")).toHaveText(t("details.failedTests"));
    const test = hello.$('.//button[contains(@class, "test")][contains(., "prints goodbye")]');
    await test.click();
    const output = hello.$(".output pre");
    await output.waitForDisplayed();
    expect(await output.getText()).toContain("-goodbye, world");
    expect(await output.getText()).toContain("+hello, world");
    // Folded again, so the row is back to its line.
    await hello.$("button[data-row]").click();
  });

  it("shows the fake gh's versions in Settings", async () => {
    await $(`button[aria-label="${t("app.settings")}"]`).click();
    const main = $("main");
    await browser.waitUntil(async () => (await main.getText()).includes("2.100.0"), {
      timeoutMsg: "gh's version never showed",
    });
    expect(await main.getText()).toContain("v1.58.1");
    await $(`button[aria-label="${t("app.back")}"]`).click();
    await item("Hello, World").waitForDisplayed();
  });

  it("refuses Submit in a development build before gh runs", async () => {
    // The clone is clean and its head isn't submitted: the row offers Submit.
    const hello = await item("Hello, World");
    await expect(hello.$(`.//button[normalize-space()="${t("submit.ok")}"]`)).toBeDisplayed();
    // The button asks through a native dialog WebDriver can't reach, so the commands it
    // runs are called the same way the UI calls them. No `error` key in the result: wdio
    // takes a response whose value has one for a failed command.
    const outcome = await browser.executeAsync((id, done) => {
      const { invoke } = window.__TAURI_INTERNALS__;
      invoke("submit_check", { id }).then(
        (check) =>
          invoke("submit_project", { id }).then(
            (submitted) => done({ check, submitted }),
            (refused) => done({ check, refused }),
          ),
        (refused) => done({ refused }),
      );
    }, HELLO_ID);
    expect(outcome.check).toEqual({ hasMarker: true, uncommitted: [], editorFiles: [] });
    expect(outcome.submitted).toBeUndefined();
    expect(outcome.refused.kind).toBe("blockedInDev");
    expect(outcome.refused.params.command).toBe("gh student submit");
    // `process::checked_args` stopped it: the fake gh never heard of it.
    expect(ghCalls().filter((call) => call.startsWith("student"))).toEqual([]);
    // gh itself was used all along (the fake one, through the PATH hook).
    expect(ghCalls()).toContain("auth token --hostname github.com");
  });

  it("shows uncommitted changes after a refresh", async () => {
    appendFileSync(join(clone, "hello.c"), "// one more line\n");
    await $(`button[aria-label="${t("app.refresh")}"]`).click();
    await browser.waitUntil(
      async () => (await text("Hello, World")).includes(t("projects.pending.uncommitted")),
      { timeout: 60_000, timeoutMsg: "the change never showed" },
    );
    // Submit only sends committed work: the button is gone.
    await expect(
      (await item("Hello, World")).$(`.//button[normalize-space()="${t("submit.ok")}"]`),
    ).not.toBeExisting();
  });
});
