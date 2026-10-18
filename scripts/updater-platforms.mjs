// Checks that a release's latest.json names every platform the release builds, so a matrix
// leg that failed to add its entry (or two legs racing on the file) can't ship a feed that
// leaves some installs without updates.
// Usage (release.yml): node scripts/updater-platforms.mjs latest.json <prerelease: true|false>
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const TARGETS = [
  { os: "darwin", arch: "aarch64", bundles: ["app"] },
  { os: "darwin", arch: "x86_64", bundles: ["app"] },
  { os: "linux", arch: "x86_64", bundles: ["appimage", "deb", "rpm"] },
  { os: "linux", arch: "aarch64", bundles: ["appimage", "deb", "rpm"] },
  { os: "windows", arch: "x86_64", bundles: ["nsis", "msi"] },
  { os: "windows", arch: "aarch64", bundles: ["nsis", "msi"] },
];

/**
 * The `platforms` keys a release's latest.json must hold: `{os}-{arch}` and
 * `{os}-{arch}-{bundle}` for each target. Pre-releases build no MSI.
 * @param {boolean} prerelease
 * @returns {string[]}
 */
export function expectedPlatforms(prerelease) {
  return TARGETS.flatMap(({ os, arch, bundles }) => [
    `${os}-${arch}`,
    ...bundles
      .filter((bundle) => !(prerelease && bundle === "msi"))
      .map((bundle) => `${os}-${arch}-${bundle}`),
  ]);
}

/**
 * The expected keys a latest.json lacks, or whose entry has no url or signature.
 * @param {{ platforms?: Record<string, { url?: string, signature?: string }> }} latest
 * @param {boolean} prerelease
 * @returns {string[]}
 */
export function missingPlatforms(latest, prerelease) {
  const platforms = latest?.platforms ?? {};
  return expectedPlatforms(prerelease).filter(
    (key) => !platforms[key]?.url || !platforms[key]?.signature,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const latest = JSON.parse(readFileSync(process.argv[2], "utf8"));
  const missing = missingPlatforms(latest, process.argv[3] === "true");
  if (missing.length > 0) {
    console.error(`latest.json lacks ${missing.join(", ")}`);
    process.exit(1);
  }
  console.log(`latest.json covers ${Object.keys(latest.platforms).length} platform entries`);
}
