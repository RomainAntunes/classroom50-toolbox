// Assignment descriptions in a row's details: GitHub-flavored Markdown with math, as Rust
// parses it (`markdown.rs`), folded to a few lines, links opening in the browser.
import { expect, test } from "./app";
import { overflow } from "./checks";
import {
  markdownDescription,
  project,
  scenario,
  snapshot,
  unaccepted,
  type Scenario,
} from "./fixtures";
import type { MarkdownBlock } from "../../src/lib/api";

const text = (value: string) => ({ kind: "text" as const, text: value });

// Wider than the popover: a table and a code line that only scroll inside their box.
const wide: MarkdownBlock[] = [
  {
    kind: "table",
    align: [null, "end"],
    head: [[text("Input")], [text("Expected output")]],
    rows: [[[text("a".repeat(60))], [text("b".repeat(60))]]],
  },
  { kind: "code", lang: "sh", text: `./mario ${"--height 8 ".repeat(12)}\n` },
];

function described(): Scenario {
  return scenario({
    snapshot: snapshot({
      projects: [
        project({ slug: "lab1", name: "Lab 1", assignment: markdownDescription }),
        project({
          slug: "lab2",
          name: "Lab 2",
          assignment: { description: "wide", descriptionDoc: wide },
        }),
        // A snapshot saved before descriptions were parsed shows the text as it is.
        project({
          slug: "lab3",
          name: "Lab 3",
          assignment: { description: "Plain **text**\non two lines", descriptionDoc: null },
        }),
      ],
      unaccepted: [unaccepted({ slug: "lab4", name: "Lab 4", ...markdownDescription })],
    }),
  });
}

test("shows a Markdown description with its table, task list, math and footnote", async ({
  app,
  page,
}) => {
  await app.start(described());
  const row = app.row("Lab 1");
  await row.locator("[data-row]").click();
  const description = row.locator(".description");

  // Folded to a few lines first, opened by Show more.
  const more = row.getByRole("button", { name: "Show more" });
  await expect(more).toHaveAttribute("aria-expanded", "false");
  await more.click();
  await expect(row.getByRole("button", { name: "Show less" })).toHaveAttribute(
    "aria-expanded",
    "true",
  );

  await expect(description.getByRole("heading", { name: "Goal" })).toBeVisible();
  await expect(description.locator("strong")).toHaveText("greeting");
  await expect(description.locator("em")).toHaveText("sum");
  // Dollar amounts stay prose; math shows its LaTeX source, left to right.
  await expect(description).toContainText("for amounts like $5 and $10.");
  await expect(description.locator("code.math")).toHaveText("a+b");
  await expect(description.locator("pre.math")).toHaveText("\\sum_{i=1}^n i = n(n+1)/2");
  await expect(description.locator("pre.math")).toHaveAttribute("dir", "ltr");

  const cells = description.locator("table td");
  await expect(cells).toHaveText(["compiles", "3", "prints hi prints hello", "5"]);
  await expect(description.locator("table s")).toHaveText("prints hi");
  await expect(description.locator("table td").nth(1)).toHaveCSS("text-align", "end");

  const boxes = description.getByRole("checkbox");
  await expect(boxes).toHaveCount(2);
  await expect(boxes.nth(0)).toBeChecked();
  await expect(boxes.nth(1)).not.toBeChecked();
  await expect(boxes.nth(0)).toBeDisabled();

  await expect(description.locator("blockquote")).toHaveText("Late work counts, marked late.");
  await expect(description.locator("sup")).toHaveText("1");
  await expect(description.getByRole("region", { name: "Footnotes" })).toContainText(
    "Also on the course page.",
  );

  expect(await overflow(page)).toEqual([]);
  expect(await app.writes()).toEqual([]);
});

test("a link in a description opens in the browser and nothing else", async ({ app, page }) => {
  await app.start(described());
  const row = app.row("Lab 1");
  await row.locator("[data-row]").click();
  await row.getByRole("button", { name: "Show more" }).click();
  const url = page.url();

  await row.locator(".description").getByRole("link", { name: "spec" }).click();
  const opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened.map((call) => call.args.url)).toEqual(["https://example.org/spec"]);
  // The popover stays where it was.
  expect(page.url()).toBe(url);
  await expect(row.locator(".description")).toBeVisible();
  expect(await app.writes()).toEqual([]);
});

test("a middle click on a link opens it in the browser too, never in the webview", async ({
  app,
  page,
}) => {
  await app.start(described());
  const row = app.row("Lab 1");
  await row.locator("[data-row]").click();
  await row.getByRole("button", { name: "Show more" }).click();
  const url = page.url();
  const pages = page.context().pages().length;

  await row.locator(".description").getByRole("link", { name: "spec" }).click({ button: "middle" });
  const opened = await app.waitForCall("plugin:opener|open_url");
  expect(opened.map((call) => call.args.url)).toEqual(["https://example.org/spec"]);
  expect(page.url()).toBe(url);
  expect(page.context().pages()).toHaveLength(pages);
  await expect(row.locator(".description")).toBeVisible();
  expect(await app.writes()).toEqual([]);
});

test("a link below the fold opens the description when it gets the focus", async ({ app }) => {
  await app.start(described());
  const row = app.row("Lab 1");
  await row.locator("[data-row]").click();
  await row.locator(".description").getByRole("link", { name: "spec" }).focus();
  await expect(row.getByRole("button", { name: "Show less" })).toBeVisible();
});

test("wide tables and code scroll inside their box, not the popover", async ({ app, page }) => {
  await app.start(described());
  const row = app.row("Lab 2");
  await row.locator("[data-row]").click();
  const box = row.locator(".description .scroll");
  await expect(box).toBeVisible();
  for (const scroller of [box, row.locator(".description pre")]) {
    const [scroll, client] = await scroller.evaluate((el) => [el.scrollWidth, el.clientWidth]);
    expect(scroll).toBeGreaterThan(client);
  }
  // Only those two scroll sideways; nothing sticks out of the popover.
  const found = await overflow(page);
  expect(found.filter((line) => !line.startsWith("scrolls sideways"))).toEqual([]);
  expect(found).toHaveLength(2);
});

test("a description not parsed yet shows as text, and unaccepted rows render Markdown", async ({
  app,
}) => {
  await app.start(described());
  const lab3 = app.row("Lab 3");
  await lab3.locator("[data-row]").click();
  await expect(lab3.locator(".description .plain")).toHaveText("Plain **text**\non two lines");
  await expect(lab3.getByRole("button", { name: "Show more" })).toHaveCount(0);

  const lab4 = app.row("Lab 4");
  await lab4.locator("[data-row]").click();
  await expect(lab4.locator(".description strong")).toHaveText("greeting");
});
