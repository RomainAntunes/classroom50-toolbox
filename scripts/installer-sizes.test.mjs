import { describe, expect, it } from "vitest";
import {
  assetKey,
  compareInstallers,
  formatGrowth,
  formatReport,
  isInstaller,
  releasesFor,
} from "./installer-sizes.mjs";

const feed = { name: "latest.json", size: 8000 };
const release = (tag_name, assets, extra = {}) => ({
  tag_name,
  draft: false,
  assets: [...assets, feed],
  ...extra,
});
const asset = (name, size) => ({ name, size });

describe("installer sizes", () => {
  it("keeps installers only", () => {
    for (const name of [
      "Classroom.50.Toolbox_0.12.0_aarch64.dmg",
      "Classroom.50.Toolbox_0.12.0_x64-setup.exe",
      "Classroom.50.Toolbox_0.12.0_arm64_en-US.msi",
      "Classroom.50.Toolbox_0.12.0_amd64.AppImage",
      "Classroom.50.Toolbox_0.12.0_arm64.deb",
      "Classroom.50.Toolbox-0.12.0-1.aarch64.rpm",
    ]) {
      expect(isInstaller(name)).toBe(true);
    }
    for (const name of [
      "Classroom.50.Toolbox_0.12.0_x64-setup.exe.sig",
      "Classroom.50.Toolbox_0.12.0_x64.app.tar.gz",
      "latest.json",
    ]) {
      expect(isInstaller(name)).toBe(false);
    }
  });

  it("matches the same installer across versions", () => {
    expect(assetKey("Classroom.50.Toolbox-0.12.0-beta.2-1.x86_64.rpm", "0.12.0-beta.2")).toBe(
      assetKey("Classroom.50.Toolbox-0.12.0-1.x86_64.rpm", "0.12.0"),
    );
    expect(assetKey("Classroom.50.Toolbox_0.12.0_x64-setup.exe", "0.12.0")).toBe(
      "Classroom.50.Toolbox_{version}_x64-setup.exe",
    );
  });

  it("compares with the previous release and warns above +20%", () => {
    const previous = release("v0.11.0", [
      asset("App_0.11.0_x64-setup.exe", 1000),
      asset("App_0.11.0_x64.dmg", 1000),
      asset("App_0.11.0_x64-setup.exe.sig", 400),
    ]);
    const current = release("v0.12.0", [
      asset("App_0.12.0_x64.dmg", 1201),
      asset("App_0.12.0_x64-setup.exe", 1200),
      asset("App_0.12.0_arm64-setup.exe", 900),
    ]);
    const rows = compareInstallers(current, previous);
    expect(rows.map(({ name, previousSize, warn }) => [name, previousSize, warn])).toEqual([
      ["App_0.12.0_arm64-setup.exe", null, false],
      ["App_0.12.0_x64-setup.exe", 1000, false],
      ["App_0.12.0_x64.dmg", 1000, true],
    ]);
    expect(rows[1].growth).toBeCloseTo(0.2);
    expect(compareInstallers(current, null).every((row) => row.previousSize === null)).toBe(true);
  });

  it("picks the newest published release of a lower version", () => {
    const releases = [
      release("v0.12.0", [], { draft: true }),
      release("v0.12.0-beta.2", []),
      release("v0.12.0-beta.3", [], { draft: true }),
      release("v0.11.0", []),
      release("beta-channel", [], { prerelease: true }),
    ];
    expect(releasesFor(releases, "v0.12.0").previous?.tag_name).toBe("v0.12.0-beta.2");
    expect(releasesFor(releases, "v0.12.0-beta.2").previous?.tag_name).toBe("v0.11.0");
    expect(releasesFor(releases, "v0.11.0").previous).toBeNull();
    expect(releasesFor(releases, "v0.13.0").current).toBeNull();
  });

  it("finds the current release by its id when given", () => {
    const releases = [
      release("v0.12.0", [asset("App_0.12.0_x64.dmg", 2)], { id: 7, draft: true }),
      release("v0.12.0", [asset("App_0.12.0_x64.dmg", 1)], { id: 8, draft: true }),
      release("v0.11.0", [], { id: 3 }),
    ];
    expect(releasesFor(releases, "v0.12.0", "8").current?.id).toBe(8);
    expect(releasesFor(releases, "v0.12.0", "8").previous?.tag_name).toBe("v0.11.0");
    expect(releasesFor(releases, "v0.12.0", 9).current).toBeNull();
    expect(releasesFor(releases, "v0.12.0").current?.id).toBe(7);
  });

  it("formats the report", () => {
    expect(formatGrowth(0.042)).toBe("+4.2%");
    expect(formatGrowth(-0.003)).toBe("-0.3%");
    expect(formatGrowth(-0.0001)).toBe("0.0%");
    const rows = compareInstallers(
      release("v0.12.0", [
        asset("App_0.12.0_x64.dmg", 3_000_000),
        asset("App_0.12.0_x64_en-US.msi", 5000),
      ]),
      release("v0.11.0", [asset("App_0.11.0_x64.dmg", 2_000_000)]),
    );
    expect(formatReport(rows, "v0.12.0", "v0.11.0").split("\n")).toEqual([
      "### Installer sizes, v0.12.0 against v0.11.0",
      "",
      "| Installer | Size | Before | Change |",
      "| --- | ---: | ---: | ---: |",
      "| `App_0.12.0_x64.dmg` | 3.00 MB | 2.00 MB | **+50.0%** |",
      "| `App_0.12.0_x64_en-US.msi` | 5.0 kB | new |  |",
    ]);
    expect(formatReport([], "v0.1.0", undefined).split("\n")[0]).toBe(
      "### Installer sizes, v0.1.0",
    );
  });
});
