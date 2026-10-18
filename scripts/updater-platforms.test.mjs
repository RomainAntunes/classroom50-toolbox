import { describe, expect, it } from "vitest";
import { expectedPlatforms, missingPlatforms } from "./updater-platforms.mjs";

const entry = { url: "https://example.invalid/asset", signature: "sig" };
const feed = (keys) => ({ platforms: Object.fromEntries(keys.map((key) => [key, entry])) });

describe("updater platforms", () => {
  it("expects every target, Arm included", () => {
    const stable = expectedPlatforms(false);
    for (const key of [
      "darwin-aarch64",
      "darwin-x86_64-app",
      "linux-x86_64-appimage",
      "linux-aarch64",
      "linux-aarch64-appimage",
      "linux-aarch64-deb",
      "linux-aarch64-rpm",
      "windows-x86_64-msi",
      "windows-aarch64",
      "windows-aarch64-nsis",
      "windows-aarch64-msi",
    ]) {
      expect(stable).toContain(key);
    }
    expect(new Set(stable).size).toBe(stable.length);
  });

  it("expects no MSI in a pre-release", () => {
    const beta = expectedPlatforms(true);
    expect(beta.filter((key) => key.endsWith("-msi"))).toEqual([]);
    expect(beta).toContain("windows-aarch64-nsis");
    expect(beta).toContain("windows-x86_64");
  });

  it("names what a feed lacks", () => {
    expect(missingPlatforms(feed(expectedPlatforms(false)), false)).toEqual([]);
    expect(missingPlatforms(feed(expectedPlatforms(true)), true)).toEqual([]);
    // A beta's feed isn't enough for a stable release.
    expect(missingPlatforms(feed(expectedPlatforms(true)), false)).toEqual([
      "windows-x86_64-msi",
      "windows-aarch64-msi",
    ]);
    const withoutArm = expectedPlatforms(false).filter((key) => !key.includes("aarch64"));
    expect(missingPlatforms(feed(withoutArm), false)).toEqual(
      expectedPlatforms(false).filter((key) => key.includes("aarch64")),
    );
    expect(missingPlatforms({}, true)).toEqual(expectedPlatforms(true));
  });

  it("counts an entry without a url or signature as missing", () => {
    const latest = feed(expectedPlatforms(true));
    latest.platforms["linux-aarch64-deb"] = { url: entry.url };
    latest.platforms["windows-aarch64"] = { signature: "sig" };
    expect(missingPlatforms(latest, true)).toEqual(["linux-aarch64-deb", "windows-aarch64"]);
  });
});
