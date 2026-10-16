// The screens `pnpm e2e:shots` captures, per skin and color scheme.
import process from "node:process";
import { App, expect, test } from "./app";
import { richScenario } from "./samples";
import { ok, settings, scenario, tools } from "./fixtures";

const OUT = new URL("../../e2e-shots/", import.meta.url).pathname;
const LOCALE = process.env.SHOTS_LOCALE ?? "en";
const SKINS = ["macos", "windows", "linux"] as const;
const SCHEMES = ["light", "dark"] as const;

for (const os of SKINS) {
  for (const scheme of SCHEMES) {
    test(`${os} ${scheme}`, async ({ app, page }) => {
      const name = (screen: string) => `${OUT}${LOCALE}-${os}-${scheme}-${screen}.png`;
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });

      const main = richScenario({ language: LOCALE });
      main.appInfo.os = os;
      main.responses.commit_check = [
        ok({
          files: ["mario.c", "cash.c", "README.md"],
          untracked: ["cash.c"],
          skipped: [".DS_Store"],
          large: [],
          tooLarge: [],
          ahead: 0,
          hasIdentity: true,
          pushBlocked: "tagMode",
        }),
      ];
      await app.start(main);
      await expect(page.locator("li[data-item]").first()).toBeVisible();
      await page.screenshot({ path: name("list") });

      await page.locator("[data-row]").first().click();
      await page.screenshot({ path: name("details") });

      // The in-popover prompt, from a row's menu.
      await app.rowMenu("Lab 2");
      await app.pickMenu("Commit…");
      await expect(app.prompt).toBeVisible();
      await page.screenshot({ path: name("prompt") });
      await app.prompt.getByRole("button").first().click();

      await page.locator(".titlebar .icon").last().click();
      await expect(page.locator("main section").first()).toBeVisible();
      await page.screenshot({ path: name("settings"), fullPage: true });

      const first = scenario({
        settings: settings({ onboarded: false, language: LOCALE }),
        tools: tools(["gh-student"]),
      });
      first.appInfo.os = os;
      const guide = await page.context().newPage();
      const guideApp = new App(guide);
      await guide.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
      await guideApp.start(first);
      await expect(guide.locator("footer .nav .primary")).toBeVisible();
      await guide.screenshot({ path: name("welcome") });
      await guide.locator("footer .nav .primary").click();
      await guide.screenshot({ path: name("welcome-tools") });
      await guide.close();
    });
  }
}
