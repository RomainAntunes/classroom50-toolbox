// Sets the app version everywhere it lives: package.json (read by tauri.conf.json and the
// release workflow) and src-tauri/Cargo.toml. Usage: pnpm bump 0.2.0, or pnpm bump
// 0.3.0-beta.1 for a pre-release (beta channel only; the release workflow accepts no other
// pre-release label).
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-beta\.\d+)?$/.test(version ?? "")) {
  console.error("usage: pnpm bump <major.minor.patch>[-beta.<n>]");
  process.exit(1);
}

const pkgPath = "package.json";
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
pkg.version = version;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

const cargoPath = "src-tauri/Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8");
const bumped = cargo.replace(/^version = ".*"$/m, `version = "${version}"`);
if (bumped === cargo && !cargo.includes(`version = "${version}"`)) {
  console.error(`no package version found in ${cargoPath}`);
  process.exit(1);
}
writeFileSync(cargoPath, bumped);

console.log(`version set to ${version}; run cargo check in src-tauri to refresh Cargo.lock`);
