// The screenshots `pnpm site:shots` writes to `site/screenshots/` for the download page and
// the README: made-up classes and classmates, each skin over a drawn desktop (macOS menu bar,
// Windows taskbar, GNOME top bar), with the translucency the real window gets from the OS.
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import type { Page } from "@playwright/test";
import { App, expect, test } from "./app";
import {
  HOME,
  classroom,
  daysFromNow,
  grade,
  hold,
  hoursFromNow,
  ok,
  project,
  scenario,
  settings,
  snapshot,
  tools,
  unaccepted,
  type ProjectOptions,
  type Scenario,
} from "./fixtures";
import type { Project } from "../../src/lib/api";

const OUT = new URL("../../site/screenshots/", import.meta.url).pathname;
const TMP = new URL("../../e2e-shots/showcase/", import.meta.url).pathname;
const TRAY = readFileSync(new URL("../../assets/tray.svg", import.meta.url), "utf8");
/** The maintainer's account, with his real avatar; the classes and classmates are made up. */
const LOGIN = "RomainAntunes";

type Os = "macos" | "windows" | "linux";
type Scheme = "light" | "dark";

const cs = classroom({
  org: "northfield-cs",
  slug: "cs101",
  name: "CS 101: Intro to Programming",
  term: "Fall 2026",
});
const web = classroom({ org: "northfield-cs", slug: "web220", name: "Web Development" });

/** `project()` with the showcase student's login in the repo name. */
function row(options: ProjectOptions): Project {
  const made = project(options);
  const repo = (made.repo ?? "").replace(/-student$/, `-${LOGIN.toLowerCase()}`);
  const id = `${made.owner}/${repo}`;
  return {
    ...made,
    id,
    repo,
    path: made.path ? `${HOME}/Classroom/${repo}` : null,
    remote: made.remote
      ? {
          ...made.remote,
          name: repo,
          url: `https://github.com/${id}`,
          head: made.remote.head ? { ...made.remote.head, author: LOGIN } : null,
        }
      : null,
  };
}

/** The account's GitHub avatar, fetched once per run (`beforeAll`); a letter when offline. */
let avatarUrl = "";

const avatar = (letter: string, from: string, to: string) =>
  "data:image/svg+xml," +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x2="0" y2="1"><stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/></linearGradient></defs><rect width="64" height="64" fill="url(#g)"/><text x="32" y="43" font-family="-apple-system, sans-serif" font-size="30" font-weight="600" fill="#fff" text-anchor="middle">${letter}</text></svg>`,
  );

function showcase(over: Partial<Scenario> = {}): Scenario {
  const base = scenario({
    settings: settings({ commitActions: "push", defaultEditor: "vscode" }),
    auth: { loggedIn: true, login: LOGIN, missingScopes: [], envToken: null },
    accounts: [{ login: LOGIN, active: true }],
    snapshot: snapshot({
      login: LOGIN,
      projects: [
        row({
          slug: "pset3",
          name: "Problem Set 3: Sorting",
          classroom: cs,
          assignment: { due: daysFromNow(-2), feedbackPr: true },
          remote: {
            head: { oid: "a1b2c3d4e5f6", at: daysFromNow(-2.2), author: LOGIN },
            lastSubmission: {
              tag: "submit/20261014T180000Z-a1b2c3d",
              commit: "a1b2c3d4e5f6",
              at: daysFromNow(-2.2),
            },
            grade: grade(8, 10, {
              testsPassed: 4,
              testsTotal: 5,
              publishedAt: daysFromNow(-2.1),
              tests: [
                {
                  name: "compiles",
                  passed: true,
                  score: 1,
                  maxScore: 1,
                  output: null,
                  outputClipped: false,
                  timedOutAfter: null,
                },
                {
                  name: "bubble sort",
                  passed: true,
                  score: 2,
                  maxScore: 2,
                  output: null,
                  outputClipped: false,
                  timedOutAfter: null,
                },
                {
                  name: "selection sort",
                  passed: true,
                  score: 2,
                  maxScore: 2,
                  output: null,
                  outputClipped: false,
                  timedOutAfter: null,
                },
                {
                  name: "merge sort keeps duplicates",
                  passed: false,
                  score: 0,
                  maxScore: 2,
                  output: "$ ./sort merge 5 3 3 1\nexpected: 1 3 3 5\n     got: 1 3 5",
                  outputClipped: false,
                  timedOutAfter: null,
                },
                {
                  name: "handles empty input",
                  passed: true,
                  score: 3,
                  maxScore: 3,
                  output: null,
                  outputClipped: false,
                  timedOutAfter: null,
                },
              ],
            }),
            feedback: { url: "https://github.com/pr/1", comments: 2, latestAt: hoursFromNow(-3) },
          },
          editor: "vscode",
        }),
        row({
          slug: "lab3",
          name: "Lab 3: Arrays",
          classroom: cs,
          assignment: { due: daysFromNow(-9) },
          remote: { grade: grade(10, 10, { testsPassed: 4, testsTotal: 4 }) },
        }),
        row({
          slug: "lab4",
          name: "Lab 4: Recursion",
          classroom: cs,
          assignment: { submissionMode: "every-push", due: hoursFromNow(5) },
          pending: "uncommitted",
          local: { changedFiles: 2, changedPaths: ["fib.c", "hanoi.c"] },
          editor: "vscode",
        }),
        row({
          slug: "pset4",
          name: "Problem Set 4: Hash Tables",
          classroom: cs,
          assignment: { submissionMode: "every-push", due: daysFromNow(4) },
          remote: { grading: { state: "running", url: null } },
        }),
        row({
          slug: "portfolio",
          name: "Portfolio Site",
          classroom: web,
          assignment: { mode: "team", teamFormation: "student", due: daysFromNow(12) },
          local: { behind: 2 },
          group: {
            org: web.org,
            slug: "classroom50-group-7f3a-2",
            number: 2,
            name: "Night Owls",
            members: [
              { login: LOGIN, name: "Romain Antunes", email: null },
              { login: "lina-k", name: "Lina Karimi", email: null },
              { login: "theo-m", name: "Théo Martin", email: null },
            ],
            founders: [LOGIN],
            url: "https://github.com/orgs/northfield-cs/teams/x",
            classroom: null,
            assignment: null,
          },
        }),
      ],
      unaccepted: [
        unaccepted({
          slug: "lab5",
          name: "Lab 5: Linked Lists",
          classroom: cs,
          due: daysFromNow(9),
        }),
        unaccepted({
          slug: "final",
          name: "Final Project",
          classroom: web,
          availableFrom: daysFromNow(6),
          due: daysFromNow(40),
        }),
      ],
    }),
    responses: {
      account_avatar: [ok(avatarUrl || avatar("R", "#8e8e93", "#48484a"))],
      commit_check: [
        ok({
          files: ["fib.c", "hanoi.c"],
          untracked: ["hanoi.c"],
          skipped: [".DS_Store"],
          large: [],
          tooLarge: [],
          ahead: 0,
          hasIdentity: true,
          pushBlocked: null,
        }),
      ],
    },
    ...over,
  });
  base.syncDetails.login = LOGIN;
  return base;
}

/** The popover alone, transparent where the OS draws its material. */
async function popover(page: Page): Promise<string> {
  const png = await page.screenshot({ omitBackground: true });
  return `data:image/png;base64,${png.toString("base64")}`;
}

// Calm desktops, so the app is what stands out.
const WALLPAPER = {
  light: "linear-gradient(180deg, #e4e7ec 0%, #cfd5de 100%)",
  dark: "linear-gradient(180deg, #34363c 0%, #1f2024 100%)",
};

/** A desktop around the popover: what the OS draws, so the screenshot reads as the app. */
function stage(os: Os, scheme: Scheme, shot: string): string {
  const dark = scheme === "dark";
  const tray = (color: string) =>
    `<span class="tray">${TRAY.replace(/#000/g, color).replace(/width="18" height="18"/, 'width="16" height="16"')}</span>`;
  const chrome = {
    macos: `<div class="menubar">${tray(dark ? "#fff" : "#000")}<span class="clock">Fri Oct 16&nbsp;&nbsp;9:41</span></div>`,
    windows: `<div class="taskbar">${tray(dark ? "#fff" : "#000")}<span class="clock">9:41 AM<br>10/16/2026</span></div>`,
    linux: `<div class="topbar"><span class="clock">Oct 16&nbsp;&nbsp;9:41</span>${tray("#fff")}</div>`,
  }[os];
  const material = {
    macos: dark ? "rgb(36 36 40 / 0.55)" : "rgb(236 236 240 / 0.55)",
    windows: dark ? "rgb(32 32 32 / 0.88)" : "rgb(243 243 243 / 0.9)",
    linux: "transparent",
  }[os];
  const radius = { macos: 12, windows: 8, linux: 12 }[os];
  const place = { macos: "top: 36px", windows: "top: 24px", linux: "top: 40px" }[os];
  return `<!doctype html><html><head><style>
    * { box-sizing: border-box; }
    body { margin: 0; }
    #stage { position: relative; width: 500px; height: 644px; overflow: hidden; background: ${WALLPAPER[scheme]};
      font: 13px -apple-system, "Segoe UI", sans-serif; color: ${dark ? "#fff" : "#000"}; }
    .tray svg { width: 16px; height: 16px; }
    .tray { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 22px; border-radius: 5px; }
    .menubar { position: absolute; inset: 0 0 auto 0; height: 30px; display: flex; align-items: center; justify-content: flex-end;
      gap: 14px; padding: 0 16px; background: ${dark ? "rgb(0 0 0 / 0.28)" : "rgb(255 255 255 / 0.38)"};
      -webkit-backdrop-filter: blur(20px); backdrop-filter: blur(20px); font-weight: 500; }
    .menubar .tray { position: absolute; left: 236px; background: ${dark ? "rgb(255 255 255 / 0.22)" : "rgb(0 0 0 / 0.12)"}; }
    .taskbar { position: absolute; inset: auto 0 0 0; height: 48px; display: flex; align-items: center; justify-content: flex-end;
      gap: 12px; padding: 0 14px; background: ${dark ? "rgb(28 28 28 / 0.85)" : "rgb(243 243 243 / 0.85)"};
      -webkit-backdrop-filter: blur(40px); backdrop-filter: blur(40px); border-top: 1px solid ${dark ? "rgb(255 255 255 / 0.08)" : "rgb(0 0 0 / 0.06)"}; }
    .taskbar .clock { font-size: 12px; text-align: right; line-height: 1.35; }
    .taskbar .tray { position: absolute; left: 352px; width: 34px; height: 38px; border-radius: 4px; background: ${dark ? "rgb(255 255 255 / 0.08)" : "rgb(0 0 0 / 0.05)"}; }
    .topbar { position: absolute; inset: 0 0 auto 0; height: 32px; display: flex; align-items: center; justify-content: flex-end;
      padding: 0 12px; background: #000; color: #fff; font-weight: 700; }
    .topbar .clock { position: absolute; left: 50%; transform: translateX(-50%); }
    .topbar .tray { position: absolute; left: 380px; width: 32px; height: 26px; border-radius: 999px; background: rgb(255 255 255 / 0.18); }
    .window { position: absolute; ${place}; left: 60px; width: 380px; height: 560px; border-radius: ${radius}px; overflow: hidden;
      background: ${material}; -webkit-backdrop-filter: blur(40px) saturate(1.8); backdrop-filter: blur(40px) saturate(1.8);
      box-shadow: 0 0 0 0.5px ${dark ? "rgb(255 255 255 / 0.18)" : "rgb(0 0 0 / 0.18)"}, 0 24px 60px rgb(0 0 0 / ${dark ? 0.55 : 0.28}); }
    .window img { display: block; width: 380px; height: 560px; }
  </style></head><body><div id="stage">${chrome}<div class="window"><img src="${shot}"></div></div></body></html>`;
}

/** Puts the popover on its desktop and writes `site/screenshots/<name>.webp`. */
async function save(page: Page, os: Os, scheme: Scheme, name: string): Promise<void> {
  const shot = await popover(page);
  const desk = await page.context().newPage();
  await desk.setViewportSize({ width: 500, height: 644 });
  await desk.setContent(stage(os, scheme, shot));
  await desk.locator("img").evaluate((img: HTMLImageElement) => img.decode());
  const png = `${TMP}${name}.png`;
  await desk.locator("#stage").screenshot({ path: png });
  await desk.close();
  execFileSync("cwebp", ["-quiet", "-q", "88", "-m", "6", png, "-o", `${OUT}${name}.webp`]);
}

/** The popover alone with its shadow, on a transparent background, as
 *  `site/screenshots/popover-<name>.webp`: for pages that draw their own surroundings. */
async function cutout(page: Page, scheme: Scheme, name: string): Promise<void> {
  const shot = await popover(page);
  const dark = scheme === "dark";
  const desk = await page.context().newPage();
  await desk.setViewportSize({ width: 460, height: 640 });
  await desk.setContent(`<!doctype html><html><head><style>
    body { margin: 0; background: transparent; }
    #stage { position: relative; width: 460px; height: 640px; }
    .window { position: absolute; top: 12px; left: 40px; width: 380px; height: 560px; border-radius: 12px; overflow: hidden;
      background: ${dark ? "rgb(40 40 44)" : "rgb(244 244 246)"};
      box-shadow: 0 0 0 0.5px ${dark ? "rgb(255 255 255 / 0.2)" : "rgb(0 0 0 / 0.16)"}, 0 22px 50px -8px rgb(0 0 0 / ${dark ? 0.6 : 0.24}); }
    .window img { display: block; width: 380px; height: 560px; }
  </style></head><body><div id="stage"><div class="window"><img src="${shot}"></div></div></body></html>`);
  await desk.locator("img").evaluate((img: HTMLImageElement) => img.decode());
  const png = `${TMP}popover-${name}.png`;
  await desk.locator("#stage").screenshot({ path: png, omitBackground: true });
  await desk.close();
  execFileSync("cwebp", [
    "-quiet",
    "-q",
    "88",
    "-alpha_q",
    "100",
    "-m",
    "6",
    png,
    "-o",
    `${OUT}popover-${name}.webp`,
  ]);
}

async function boot(app: App, page: Page, os: Os, scheme: Scheme, data = showcase()) {
  await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
  data.appInfo.os = os;
  await app.start(data);
  await expect(page.locator("li[data-item]").first()).toBeVisible();
  // The avatar and the fonts settle.
  await page.waitForTimeout(300);
}

test.beforeAll(async () => {
  try {
    const response = await fetch(`https://github.com/${LOGIN}.png?size=96`);
    if (response.ok) {
      const bytes = Buffer.from(await response.arrayBuffer());
      avatarUrl = `data:${response.headers.get("content-type") ?? "image/png"};base64,${bytes.toString("base64")}`;
    }
  } catch {
    // Offline: the letter.
  }
  rmSync(TMP, { recursive: true, force: true });
  mkdirSync(TMP, { recursive: true });
  mkdirSync(OUT, { recursive: true });
  writeFileSync(`${TMP}.keep`, "");
});

for (const os of ["macos", "windows", "linux"] as const) {
  for (const scheme of ["light", "dark"] as const) {
    test(`list ${os} ${scheme}`, async ({ app, page }) => {
      await boot(app, page, os, scheme);
      await save(page, os, scheme, `list-${os}-${scheme}`);
    });
  }
}

for (const scheme of ["light", "dark"] as const) {
  test(`list ${scheme}`, async ({ app, page }) => {
    await boot(app, page, "macos", scheme);
    await cutout(page, scheme, `list-${scheme}`);
  });

  test(`details ${scheme}`, async ({ app, page }) => {
    await boot(app, page, "macos", scheme);
    await app.row("Problem Set 3").locator("[data-row]").click();
    await page.getByText("merge sort keeps duplicates").click();
    await expect(page.getByText("expected: 1 3 3 5")).toBeVisible();
    await cutout(page, scheme, `details-${scheme}`);
  });

  test(`commit ${scheme}`, async ({ app, page }) => {
    await boot(app, page, "macos", scheme);
    await app.rowMenu("Lab 4");
    await app.pickMenu("Commit & push…");
    await expect(app.prompt).toBeVisible();
    // Headless WebKit draws no backdrop blur; the frosted card the app shows, made opaque.
    await page.addStyleTag({
      content: `.card { background: ${scheme === "dark" ? "rgb(48 48 54 / 0.97)" : "rgb(250 250 252 / 0.97)"} !important; }`,
    });
    await app.prompt.getByRole("textbox").fill("Add the Towers of Hanoi");
    await cutout(page, scheme, `commit-${scheme}`);
  });

  test(`setup ${scheme}`, async ({ app, page }) => {
    await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
    const first = showcase({
      settings: settings({ onboarded: false }),
      tools: tools(["gh", "gh-student"]),
      responses: { install_tool: [hold()] },
    });
    await app.start(first);
    await expect(page.locator("footer .nav .primary")).toBeVisible();
    await page.locator("footer .nav .primary").click();
    const gh = page.locator(".tool").filter({ hasText: "GitHub CLI" }).first();
    await gh.getByRole("button", { name: "Install" }).click();
    await app.emit("tool-jobs", [{ tool: "gh", kind: "install", progress: 0.62 }]);
    await expect(gh.getByRole("progressbar")).toBeVisible();
    await page.waitForTimeout(300);
    await cutout(page, scheme, `setup-${scheme}`);
  });
}

// The link preview (`og:image`) of the download page: 1200x630, JPEG for every site that shows one.
test("social card", async ({ app, page }) => {
  await boot(app, page, "macos", "light");
  const shot = await popover(page);
  const icon = readFileSync(new URL("../../assets/icon.svg", import.meta.url), "utf8");
  const card = await page.context().newPage();
  await card.setViewportSize({ width: 1200, height: 630 });
  await card.setContent(`<!doctype html><html><head><style>
    body { margin: 0; }
    #card { position: relative; width: 1200px; height: 630px; overflow: hidden; background: #f5f5f7;
      font-family: -apple-system, "Segoe UI", sans-serif; color: #1d1d1f; }
    .text { position: absolute; left: 72px; top: 0; bottom: 0; width: 560px; display: flex; flex-direction: column; justify-content: center; }
    .text svg { width: 120px; height: 120px; margin: 0 0 12px -14px; }
    h1 { margin: 0; font-size: 64px; line-height: 1.02; letter-spacing: -0.03em; font-weight: 700; }
    p { margin: 22px 0 0; font-size: 28px; line-height: 1.3; color: #6e6e73; }
    .window { position: absolute; left: 720px; top: 56px; width: 380px; height: 560px; border-radius: 12px; overflow: hidden;
      background: rgb(244 244 246); box-shadow: 0 0 0 0.5px rgb(0 0 0 / 0.16), 0 30px 70px -10px rgb(0 0 0 / 0.25);
      transform: scale(1.12); transform-origin: top left; }
    .window img { display: block; width: 380px; height: 560px; }
  </style></head><body><div id="card">
    <div class="text">${icon}<h1>Classroom 50 Toolbox</h1>
    <p>Your assignments, due dates and grades in the menu bar. Free for macOS, Windows and Linux.</p></div>
    <div class="window"><img src="${shot}"></div>
  </div></body></html>`);
  await card.locator("img").evaluate((img: HTMLImageElement) => img.decode());
  await card
    .locator("#card")
    .screenshot({ path: `${OUT}social.jpg`, type: "jpeg", quality: 88, scale: "css" });
  await card.close();
});
