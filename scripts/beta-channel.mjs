// Picks the release the beta channel's latest.json copies: the newest published release,
// pre-release or stable, so beta users also move onto the stable that follows their beta.
// Usage (beta-channel.yml): node scripts/beta-channel.mjs releases.json, where the file
// holds `gh api repos/<repo>/releases --paginate --slurp`; prints the tag.
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

/** The tag's version (`v0.11.0` or `v0.11.0-beta.2`), or null for any other tag. */
export function parseVersion(tag) {
  const match = /^v(\d+)\.(\d+)\.(\d+)(?:-beta\.(\d+))?$/.exec(tag ?? "");
  if (!match) return null;
  const [major, minor, patch] = match.slice(1, 4).map(Number);
  const beta = match[4] === undefined ? null : Number(match[4]);
  return { major, minor, patch, beta };
}

/** Semver order: a beta comes before the release of the same number. */
export function compareVersions(a, b) {
  for (const key of ["major", "minor", "patch"]) {
    if (a[key] !== b[key]) return a[key] - b[key];
  }
  if (a.beta === b.beta) return 0;
  if (a.beta === null) return 1;
  if (b.beta === null) return -1;
  return a.beta - b.beta;
}

/** The newest published release with a latest.json, or null. Drafts don't count. */
export function channelRelease(releases) {
  let best = null;
  for (const release of releases) {
    if (release.draft) continue;
    const version = parseVersion(release.tag_name);
    if (!version) continue;
    if (!(release.assets ?? []).some((asset) => asset.name === "latest.json")) continue;
    if (!best || compareVersions(version, best.version) > 0) best = { release, version };
  }
  return best?.release ?? null;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const pages = JSON.parse(readFileSync(process.argv[2], "utf8"));
  const release = channelRelease(pages.flat());
  if (!release) {
    console.error("no published release with a latest.json");
    process.exit(1);
  }
  console.log(release.tag_name);
}
