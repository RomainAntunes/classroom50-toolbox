// Right to left, through the dev build's preview (Settings > Developer), until a
// right-to-left catalog is registered (`layout.spec.ts` then covers it in the shipped build
// too): `dir="rtl"`, data isolated inside sentences while bare numbers aren't, and nothing
// wider than the popover in any skin.
import { expect, test } from "./app";
import { overflow, untranslated } from "./checks";
import { richScenario } from "./samples";

const FSI = "⁨";
const PDI = "⁩";
const SKINS = ["macos", "windows", "linux"] as const;

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("dev.previewRtl", "1"));
});

test("the UI reads right to left: names isolated, scores left alone", async ({ app, page }) => {
  await app.start(richScenario());
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  await expect(page.locator("li[data-item]").first()).toBeVisible();

  // A login inside a sentence keeps its own order...
  const account = page.locator("footer button.account");
  await expect(account).toHaveAttribute("aria-label", new RegExp(`${FSI}student${PDI}`));
  // ...while "8/10" stays one run of numbers, so it doesn't read "10/8".
  const grade = app.row("Lab 1: Hello, World").locator(".grade");
  await expect(grade.locator("[aria-hidden=true]")).toHaveText("8/10");
  const spoken = (await grade.locator(".sr-only").textContent()) ?? "";
  expect(spoken).toContain("8");
  expect(spoken).not.toContain(`${FSI}8${PDI}`);
  expect(spoken).not.toContain(`${FSI}10${PDI}`);
});

for (const os of SKINS) {
  test(`right to left on ${os}: main page and Settings fit`, async ({ app, page }) => {
    const scenario = richScenario();
    scenario.appInfo.os = os;
    await app.start(scenario);
    await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
    await expect(page.locator("html")).toHaveAttribute("data-os", os);
    await expect(page.locator("li[data-item]").first()).toBeVisible();
    await page.locator("[data-row]").first().click();
    expect(await untranslated(page), "raw keys on the main page").toEqual([]);
    expect(await overflow(page), "overflow on the main page").toEqual([]);

    await app.openSettings();
    expect(await untranslated(page), "raw keys in Settings").toEqual([]);
    expect(await overflow(page), "overflow in Settings").toEqual([]);
  });
}
