// The world on disk for one run: a fresh home (so settings, caches and `clones.json` start
// empty), the settings the app reads at start, and a real git clone of the hello assignment
// in a watched folder. Everything lives in one temp folder.

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repo = new URL("../../", import.meta.url);
/** Tauri's app dirs are named after the bundle identifier. */
export const IDENTIFIER = JSON.parse(
  readFileSync(new URL("src-tauri/tauri.conf.json", repo), "utf8"),
).identifier;

/** Runs git in `cwd` with a fixed identity and dates, ignoring the runner's own config. */
function git(cwd, args, date) {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    env: {
      ...process.env,
      GIT_CONFIG_NOSYSTEM: "1",
      GIT_CONFIG_GLOBAL: "/dev/null",
      GIT_AUTHOR_NAME: "Ada E2E",
      GIT_AUTHOR_EMAIL: "4242+ada-e2e@users.noreply.github.com",
      GIT_COMMITTER_NAME: "Ada E2E",
      GIT_COMMITTER_EMAIL: "4242+ada-e2e@users.noreply.github.com",
      ...(date ? { GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date } : {}),
    },
  }).trim();
}

/**
 * Creates the run's folders and the hello clone: a first commit (the one submitted and
 * graded) and a second one (GitHub's head, not submitted yet), origin on GitHub.
 */
export function prepare() {
  const root = mkdtempSync(join(tmpdir(), "c50-e2e-"));
  const home = join(root, "home");
  const work = join(root, "work");
  const env = {
    HOME: home,
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_CACHE_HOME: join(home, ".cache"),
    XDG_DATA_HOME: join(home, ".local", "share"),
    XDG_STATE_HOME: join(home, ".local", "state"),
  };
  for (const dir of [work, ...Object.values(env)]) mkdirSync(dir, { recursive: true });

  const configDir = join(env.XDG_CONFIG_HOME, IDENTIFIER);
  mkdirSync(configDir, { recursive: true });
  writeFileSync(
    join(configDir, "settings.json"),
    JSON.stringify(
      {
        watchedFolders: [work],
        scanEditorProjects: false,
        launchAtLogin: false,
        language: "en",
        appearance: "light",
        onboarded: true,
      },
      null,
      2,
    ),
  );

  const clone = join(work, "algo-hello-ada-e2e");
  mkdirSync(clone);
  git(clone, ["init", "--quiet", "--initial-branch=main"]);
  writeFileSync(join(clone, ".classroom50.yaml"), "classroom: algo\nassignment: hello\n");
  writeFileSync(
    join(clone, "hello.c"),
    '#include <stdio.h>\n\nint main(void) { puts("hello, world"); }\n',
  );
  git(clone, ["add", "--all"]);
  git(clone, ["commit", "--quiet", "--message=hello"], "2026-10-01T10:00:00Z");
  const helloSubmitted = git(clone, ["rev-parse", "HEAD"]);
  writeFileSync(
    join(clone, "hello.c"),
    '#include <stdio.h>\n\nint main(void) {\n  puts("hello, world");\n  puts("goodbye, world");\n}\n',
  );
  git(clone, ["commit", "--quiet", "--all", "--message=say goodbye"], "2026-10-02T10:00:00Z");
  const hello = git(clone, ["rev-parse", "HEAD"]);
  git(clone, [
    "remote",
    "add",
    "origin",
    "https://github.com/c50-e2e-school/algo-hello-ada-e2e.git",
  ]);

  return { root, env, work, clone, heads: { hello, helloSubmitted } };
}
