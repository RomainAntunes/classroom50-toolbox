// Installer sizes of a release, next to the same installers of the previous published
// release. Writes a table to $GITHUB_STEP_SUMMARY (or stdout) and a `::warning::` for each
// installer that grew by more than 20%. Never fails the build: sizes are information.
// Usage (release.yml): node scripts/installer-sizes.mjs releases.json <tag> [<release id>],
// where the file holds `gh api repos/<repo>/releases --paginate --slurp`.
import { appendFileSync, readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { channelRelease, compareVersions, parseVersion } from "./beta-channel.mjs";
import { formatBytes } from "./size-budget.mjs";

/** Growth over which an installer gets a warning, as a fraction of the previous size. */
export const WARN_GROWTH = 0.2;

/** Installers people download; signatures, updater archives and latest.json are left out. */
export function isInstaller(name) {
  return /(\.dmg|\.exe|\.msi|\.AppImage|\.deb|\.rpm)$/.test(name);
}

/**
 * The asset's name with its version replaced, so the same installer of two releases matches:
 * `Classroom.50.Toolbox_0.12.0_x64-setup.exe` -> `Classroom.50.Toolbox_{version}_x64-setup.exe`.
 */
export function assetKey(name, version) {
  return name.split(version).join("{version}");
}

/**
 * One row per installer of `current`, with the previous release's size of the same installer
 * when it had one.
 * @param {{ tag_name: string, assets: { name: string, size: number }[] }} current
 * @param {{ tag_name: string, assets: { name: string, size: number }[] } | null} previous
 */
export function compareInstallers(current, previous) {
  const version = current.tag_name.replace(/^v/, "");
  const before = new Map();
  if (previous) {
    const previousVersion = previous.tag_name.replace(/^v/, "");
    for (const asset of previous.assets ?? []) {
      if (isInstaller(asset.name)) before.set(assetKey(asset.name, previousVersion), asset.size);
    }
  }
  return (
    (current.assets ?? [])
      .filter((asset) => isInstaller(asset.name))
      .map((asset) => {
        const previousSize = before.get(assetKey(asset.name, version)) ?? null;
        const growth = previousSize ? asset.size / previousSize - 1 : null;
        return {
          name: asset.name,
          size: asset.size,
          previousSize,
          growth,
          warn: growth !== null && growth > WARN_GROWTH,
        };
      })
      // Code-unit order, the same on every machine (localeCompare depends on the locale).
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
  );
}

/** `+4.2%`, `-0.3%`, `0.0%`. */
export function formatGrowth(growth) {
  const percent = (growth * 100).toFixed(1);
  if (Number(percent) === 0) return "0.0%";
  return growth > 0 ? `+${percent}%` : `${percent}%`;
}

/** The rows as a Markdown table under a heading naming both releases. */
export function formatReport(rows, tag, previousTag) {
  const lines = [
    `### Installer sizes, ${tag}${previousTag ? ` against ${previousTag}` : ""}`,
    "",
    "| Installer | Size | Before | Change |",
    "| --- | ---: | ---: | ---: |",
    ...rows.map(({ name, size, previousSize, growth, warn }) => {
      const before = previousSize === null ? "new" : formatBytes(previousSize);
      const change = growth === null ? "" : formatGrowth(growth);
      return `| \`${name}\` | ${formatBytes(size)} | ${before} | ${warn ? `**${change}**` : change} |`;
    }),
  ];
  return lines.join("\n");
}

/**
 * The release with this id (the draft release.yml made), else the one with this tag, and the
 * newest published release of a lower version (pre-release or stable, as the beta channel
 * picks them), or null.
 */
export function releasesFor(releases, tag, id = null) {
  const current =
    releases.find((release) =>
      id ? String(release.id) === String(id) : release.tag_name === tag,
    ) ?? null;
  const version = parseVersion(tag);
  const earlier = releases.filter((release) => {
    const other = parseVersion(release.tag_name);
    return version && other && compareVersions(other, version) < 0;
  });
  return { current, previous: channelRelease(earlier) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const tag = process.argv[3];
  const id = process.argv[4] || null;
  const pages = JSON.parse(readFileSync(process.argv[2], "utf8"));
  const { current, previous } = releasesFor(pages.flat(), tag, id);
  if (!current) {
    console.log(`::warning::no release ${tag} to measure`);
    process.exit(0);
  }
  const rows = compareInstallers(current, previous);
  const report = formatReport(rows, tag, previous?.tag_name);
  if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${report}\n`);
  }
  console.log(report);
  for (const row of rows.filter((row) => row.warn)) {
    console.log(
      `::warning title=Installer size::${row.name} is ${formatBytes(row.size)}, ${formatGrowth(row.growth)} over ${previous.tag_name}`,
    );
  }
}
