import { describe, expect, it } from "vitest";
import { channelRelease, compareVersions, parseVersion } from "./beta-channel.mjs";

const feed = [{ name: "latest.json" }];
const release = (tag_name, extra = {}) => ({ tag_name, draft: false, assets: feed, ...extra });

describe("beta channel", () => {
  it("reads release tags only", () => {
    expect(parseVersion("v0.11.0-beta.2")).toEqual({ major: 0, minor: 11, patch: 0, beta: 2 });
    expect(parseVersion("v0.10.0")).toEqual({ major: 0, minor: 10, patch: 0, beta: null });
    for (const tag of ["beta-channel", "0.10.0", "v0.10.0-rc.1", "v0.10", undefined]) {
      expect(parseVersion(tag)).toBeNull();
    }
  });

  it("orders like semver", () => {
    const order = ["v0.9.9", "v0.10.0-beta.2", "v0.10.0-beta.10", "v0.10.0", "v0.10.1-beta.1"];
    const sorted = [...order].sort((a, b) => compareVersions(parseVersion(a), parseVersion(b)));
    expect(sorted).toEqual(order);
  });

  it("follows the newest beta, then the stable after it", () => {
    expect(channelRelease([release("v0.10.0"), release("v0.11.0-beta.1")])?.tag_name).toBe(
      "v0.11.0-beta.1",
    );
    expect(
      channelRelease([release("v0.11.0-beta.2"), release("v0.11.0"), release("v0.10.0")])?.tag_name,
    ).toBe("v0.11.0");
  });

  it("skips drafts, the channel itself and releases without a feed", () => {
    const releases = [
      release("v0.10.0"),
      release("v0.12.0", { draft: true }),
      release("v0.11.0-beta.1", { assets: [] }),
      release("beta-channel", { prerelease: true }),
    ];
    expect(channelRelease(releases)?.tag_name).toBe("v0.10.0");
    expect(channelRelease([])).toBeNull();
  });
});
