#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { execSync, execFileSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");

const CARGO_TOML = resolve(root, "Cargo.toml");
const CARGO_LOCK = resolve(root, "Cargo.lock");

function bumpCargoToml(path, version) {
  let cargo = readFileSync(path, "utf8");
  const pkgIdx = cargo.indexOf("[package]");
  const nextSection = cargo.indexOf("\n[", pkgIdx + 1);
  const pkgSection = cargo.substring(pkgIdx, nextSection === -1 ? undefined : nextSection);
  const updatedSection = pkgSection.replace(/^version = ".*"$/m, `version = "${version}"`);
  cargo = cargo.substring(0, pkgIdx) + updatedSection + (nextSection === -1 ? "" : cargo.substring(nextSection));
  writeFileSync(path, cargo);
}

const SEMVER_RE = /^\d+\.\d+\.\d+(-[a-zA-Z0-9.]+)?$/;

const version = process.argv[2];
const shouldTag = process.argv.includes("--tag");

if (!version) {
  console.error("Usage: node scripts/bump-version.mjs <version> [--tag]");
  console.error("  <version>  Semver string, e.g. 0.2.0 or 1.0.0-rc1");
  console.error("  --tag      Git add, commit, and tag after updating files");
  process.exit(1);
}

if (!SEMVER_RE.test(version)) {
  console.error(`Invalid semver: "${version}". Expected format: X.Y.Z or X.Y.Z-pre`);
  process.exit(1);
}

bumpCargoToml(CARGO_TOML, version);
console.log(`  Cargo.toml → ${version}`);

// Record only our own package's version; do not upgrade dependencies.
execSync("cargo update --workspace", { cwd: root, stdio: "inherit" });
console.log(`  Cargo.lock updated`);

console.log(`\nVersion bumped to ${version}`);

if (shouldTag) {
  const files = [CARGO_TOML, CARGO_LOCK].map((f) => relative(root, f));
  // Use execFileSync (no shell) so a future file path with spaces or
  // glob metachars can't break quoting.
  execFileSync("git", ["add", "--", ...files], { cwd: root, stdio: "inherit" });
  let hasStaged = false;
  try { execFileSync("git", ["diff", "--cached", "--quiet"], { cwd: root, stdio: "ignore" }); } catch { hasStaged = true; }
  if (hasStaged) {
    execFileSync("git", ["commit", "-m", `chore: bump version to ${version}`], {
      cwd: root,
      stdio: "inherit",
    });
  } else {
    console.log("  No changes to commit (version already current)");
  }
  execFileSync("git", ["tag", `v${version}`], { cwd: root, stdio: "inherit" });
  console.log(`Created tag v${version}`);
}
