// Tells ci.yml how much a change needs checking. A version bump (`pnpm bump`: the version
// in package.json, Cargo.toml and Cargo.lock, plus its changelog file) or a changelog edit
// leaves the code as the commit before had it, already checked, so it is `bump`: only the
// versions and changelogs are checked. Anything else is `full`.
// Usage (ci.yml): node scripts/ci-scope.mjs <base sha>, which prints `bump` or `full`;
// node scripts/ci-scope.mjs --versions fails when the three versions disagree.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const CHANGELOG = /^src\/lib\/changelog\/[^/]+\.json$/;
const CARGO_VERSION = /^version = "[^"]*"$/m;
const LOCK_VERSION = /^name = "classroom50-toolbox"\nversion = "[^"]*"$/m;

/**
 * Whether a file's two sides differ by the app version at most.
 * @param {string} path
 * @param {string} before
 * @param {string} after
 * @returns {boolean}
 */
function versionOnly(path, before, after) {
  if (path === "package.json") {
    try {
      const [old, next] = [JSON.parse(before), JSON.parse(after)];
      delete old.version;
      delete next.version;
      return JSON.stringify(old) === JSON.stringify(next);
    } catch {
      return false;
    }
  }
  const pattern = path === "src-tauri/Cargo.toml" ? CARGO_VERSION : LOCK_VERSION;
  return before.replace(pattern, "") === after.replace(pattern, "");
}

/**
 * `bump` when every changed file is a changelog (added or edited) or one of the version's
 * homes changed in its version only, else `full`. A side is null when the file is absent.
 * @param {{ path: string, before: string | null, after: string | null }[]} changes
 * @returns {"bump" | "full"}
 */
export function ciScope(changes) {
  if (changes.length === 0) return "full";
  const light = changes.every(({ path, before, after }) => {
    if (after === null) return false;
    if (CHANGELOG.test(path)) return true;
    if (!["package.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"].includes(path)) {
      return false;
    }
    return before !== null && versionOnly(path, before, after);
  });
  return light ? "bump" : "full";
}

/**
 * The app version as each of its homes states it, null where none is found.
 * @param {{ pkg: string, cargo: string, lock: string }} files
 * @returns {{ pkg: string | null, cargo: string | null, lock: string | null }}
 */
export function versions({ pkg, cargo, lock }) {
  const quoted = (/** @type {string | undefined} */ line) => line?.match(/"([^"]*)"$/)?.[1];
  return {
    pkg: JSON.parse(pkg).version ?? null,
    cargo: quoted(cargo.match(CARGO_VERSION)?.[0]) ?? null,
    lock: quoted(lock.match(LOCK_VERSION)?.[0]) ?? null,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const git = (/** @type {string[]} */ args) =>
    execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  const read = (/** @type {string} */ spec) => {
    try {
      return git(["show", spec]);
    } catch {
      return null;
    }
  };
  if (process.argv[2] === "--versions") {
    const found = versions({
      pkg: readFileSync("package.json", "utf8"),
      cargo: readFileSync("src-tauri/Cargo.toml", "utf8"),
      lock: readFileSync("src-tauri/Cargo.lock", "utf8"),
    });
    if (!found.pkg || found.cargo !== found.pkg || found.lock !== found.pkg) {
      console.error(
        `versions disagree: package.json ${found.pkg}, Cargo.toml ${found.cargo}, Cargo.lock ${found.lock} (run cargo check in src-tauri)`,
      );
      process.exit(1);
    }
    console.log(`version ${found.pkg} everywhere`);
  } else {
    const base = process.argv[2];
    const paths = git(["diff", "--name-only", "--no-renames", "-z", base, "HEAD"])
      .split("\0")
      .filter(Boolean);
    const changes = paths.map((path) => ({
      path,
      before: read(`${base}:${path}`),
      after: read(`HEAD:${path}`),
    }));
    console.log(ciScope(changes));
  }
}
