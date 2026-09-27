#!/usr/bin/env node
// Keeps the version identical in package.json, tauri.conf.json, Cargo.toml and
// Cargo.lock (CI builds with --locked, so a stale lock fails the build).
//   node scripts/bump.mjs 1.0.0-test.1    set the version everywhere
//   node scripts/bump.mjs --check 1.0.0    fail if any file differs
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const files = {
  pkg: join(root, "package.json"),
  conf: join(root, "src-tauri", "tauri.conf.json"),
  cargo: join(root, "src-tauri", "Cargo.toml"),
  lock: join(root, "src-tauri", "Cargo.lock"),
};

const read = () => ({
  pkg: JSON.parse(readFileSync(files.pkg, "utf8")).version,
  conf: JSON.parse(readFileSync(files.conf, "utf8")).version,
  cargo: /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(files.cargo, "utf8"))?.[1],
  lock: /\[\[package\]\]\nname = "byte"\nversion = "([^"]+)"/.exec(readFileSync(files.lock, "utf8"))?.[1],
});

const args = process.argv.slice(2);
const check = args[0] === "--check";
const version = check ? args[1] : args[0];

if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(version)) {
  console.error("usage: bump.mjs [--check] <semver>");
  process.exit(2);
}

if (check) {
  const found = read();
  const bad = Object.entries(found).filter(([, v]) => v !== version);
  if (bad.length) {
    console.error(`version mismatch, expected ${version}:`, found);
    process.exit(1);
  }
  console.log(`all versions are ${version}`);
} else {
  const pkg = JSON.parse(readFileSync(files.pkg, "utf8"));
  pkg.version = version;
  writeFileSync(files.pkg, `${JSON.stringify(pkg, null, 2)}\n`);
  const conf = JSON.parse(readFileSync(files.conf, "utf8"));
  conf.version = version;
  writeFileSync(files.conf, `${JSON.stringify(conf, null, 2)}\n`);
  const cargo = readFileSync(files.cargo, "utf8").replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`);
  writeFileSync(files.cargo, cargo);
  const lock = readFileSync(files.lock, "utf8").replace(/(\[\[package\]\]\nname = "byte"\nversion = )"[^"]+"/, `$1"${version}"`);
  writeFileSync(files.lock, lock);
  console.log(`set version ${version}`);
}
