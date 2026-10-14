// Sets the app version everywhere it lives: package.json (read by tauri.conf.json and the
// release workflow) and src-tauri/Cargo.toml. Usage: pnpm bump 0.2.0
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? "")) {
  console.error("usage: pnpm bump <major.minor.patch>");
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
