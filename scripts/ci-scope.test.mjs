import { describe, expect, it } from "vitest";
import { ciScope, versions, withVersion } from "./ci-scope.mjs";

const pkg = (version, extra = {}) =>
  JSON.stringify({ name: "classroom50-toolbox", version, private: true, ...extra }, null, 2);
const cargo = (version, deps = 'serde = "1"') =>
  `[package]\nname = "classroom50-toolbox"\nversion = "${version}"\n\n[dependencies]\n${deps}\n`;
const lock = (version, serde = "1.0.0") =>
  `[[package]]\nname = "classroom50-toolbox"\nversion = "${version}"\n\n[[package]]\nname = "serde"\nversion = "${serde}"\n`;

const bump = [
  { path: "package.json", before: pkg("0.12.0-beta.2"), after: pkg("0.12.0") },
  { path: "src-tauri/Cargo.toml", before: cargo("0.12.0-beta.2"), after: cargo("0.12.0") },
  { path: "src-tauri/Cargo.lock", before: lock("0.12.0-beta.2"), after: lock("0.12.0") },
  { path: "src/lib/changelog/0.12.0.json", before: null, after: "{}" },
];

describe("ci scope", () => {
  it("keeps a version bump light", () => {
    expect(ciScope(bump)).toBe("bump");
  });

  it("keeps a changelog edit light", () => {
    expect(
      ciScope([{ path: "src/lib/changelog/0.11.0.json", before: "{}", after: '{"date":""}' }]),
    ).toBe("bump");
  });

  it("checks everything when another file changed", () => {
    expect(ciScope([...bump, { path: "src/app.html", before: "a", after: "b" }])).toBe("full");
    expect(
      ciScope([...bump, { path: "src/lib/changelog/index.ts", before: "a", after: "b" }]),
    ).toBe("full");
    expect(ciScope([{ path: "src/lib/changelog/x/y.json", before: null, after: "{}" }])).toBe(
      "full",
    );
  });

  it("checks everything when a version file changed more than its version", () => {
    const pkgDeps = { path: "package.json", before: pkg("1.0.0"), after: pkg("1.0.1", { a: 1 }) };
    expect(ciScope([pkgDeps])).toBe("full");
    const cargoDeps = {
      path: "src-tauri/Cargo.toml",
      before: cargo("1.0.0"),
      after: cargo("1.0.1", 'serde = "2"'),
    };
    expect(ciScope([cargoDeps])).toBe("full");
    const lockDeps = {
      path: "src-tauri/Cargo.lock",
      before: lock("1.0.0"),
      after: lock("1.0.1", "1.0.1"),
    };
    expect(ciScope([lockDeps])).toBe("full");
    // Another crate's version alone, the app's unchanged.
    expect(
      ciScope([
        { path: "src-tauri/Cargo.lock", before: lock("1.0.0"), after: lock("1.0.0", "1.0.1") },
      ]),
    ).toBe("full");
  });

  it("checks everything for a deleted, added or unreadable version file", () => {
    expect(ciScope([{ path: "src/lib/changelog/0.1.0.json", before: "{}", after: null }])).toBe(
      "full",
    );
    expect(ciScope([{ path: "package.json", before: null, after: pkg("1.0.0") }])).toBe("full");
    expect(ciScope([{ path: "package.json", before: "{", after: pkg("1.0.0") }])).toBe("full");
  });

  it("checks everything when nothing changed", () => {
    expect(ciScope([])).toBe("full");
  });
});

describe("versions", () => {
  it("reads the version from each file", () => {
    expect(versions({ pkg: pkg("1.2.3"), cargo: cargo("1.2.3"), lock: lock("1.2.4") })).toEqual({
      pkg: "1.2.3",
      cargo: "1.2.3",
      lock: "1.2.4",
    });
  });

  it("reads the app's own lock entry, not another crate's", () => {
    expect(versions({ pkg: "{}", cargo: "", lock: lock("1.0.0", "9.9.9") }).lock).toBe("1.0.0");
    expect(versions({ pkg: "{}", cargo: "", lock: "" })).toEqual({
      pkg: null,
      cargo: null,
      lock: null,
    });
  });

  it("writes a version into all three homes, so they agree", () => {
    const files = { pkg: pkg("0.12.1"), cargo: cargo("0.12.1"), lock: lock("0.12.1") };
    const bumped = withVersion(files, "0.13.0-beta.1");
    expect(versions(bumped)).toEqual({
      pkg: "0.13.0-beta.1",
      cargo: "0.13.0-beta.1",
      lock: "0.13.0-beta.1",
    });
    expect(bumped.lock).toContain('name = "serde"\nversion = "1.0.0"');
    expect(
      ciScope(
        Object.entries({
          "package.json": "pkg",
          "src-tauri/Cargo.toml": "cargo",
          "src-tauri/Cargo.lock": "lock",
        }).map(([path, key]) => ({ path, before: files[key], after: bumped[key] })),
      ),
    ).toBe("bump");
    expect(() =>
      withVersion({ ...files, lock: lock("1").replace("classroom50-toolbox", "x") }, "1.0.0"),
    ).toThrow();
  });
});
