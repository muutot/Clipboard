#!/usr/bin/env node
/**
 * version.mjs — Bump version across all config files in one atomic operation.
 *
 * Usage:
 *   node scripts/version.mjs <new-version>          # bump to specific version
 *   node scripts/version.mjs patch|minor|major      # semantic bump
 *   node scripts/version.mjs --current              # print current version
 */

import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");

const VERSION_FILES = [
  { path: "package.json", key: "version", format: "json" },
  { path: "src-tauri/tauri.conf.json", key: "version", format: "json" },
  { path: "src-tauri/Cargo.toml", key: "version", format: "toml" },
];

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf-8"));
}

function writeJson(filePath, data) {
  writeFileSync(filePath, JSON.stringify(data, null, 2) + "\n", "utf-8");
  try {
    execSync(`npx prettier --write "${filePath}"`, {
      cwd: ROOT,
      encoding: "utf-8",
      stdio: "pipe",
    });
  } catch {
    // non-fatal — file is valid JSON regardless
  }
}

function readToml(filePath) {
  return readFileSync(filePath, "utf-8");
}

function writeToml(filePath, content) {
  writeFileSync(filePath, content, "utf-8");
}

function getCurrentVersion() {
  const pkg = readJson(resolve(ROOT, "package.json"));
  return pkg.version;
}

function parseSemver(v) {
  const match = v.match(/^(\d+)\.(\d+)\.(\d+)(-.+)?$/);
  if (!match) throw new Error(`Invalid semver: ${v}`);
  return {
    major: parseInt(match[1]),
    minor: parseInt(match[2]),
    patch: parseInt(match[3]),
    pre: match[4] || "",
  };
}

function formatSemver({ major, minor, patch, pre }) {
  return `${major}.${minor}.${patch}${pre}`;
}

function bumpVersion(current, target) {
  const cur = parseSemver(current);
  if (target === "major") {
    cur.major += 1;
    cur.minor = 0;
    cur.patch = 0;
    cur.pre = "";
  } else if (target === "minor") {
    cur.minor += 1;
    cur.patch = 0;
    cur.pre = "";
  } else if (target === "patch") {
    cur.patch += 1;
    cur.pre = "";
  } else {
    // Assume it's a specific version string like "0.2.0" or "0.2.0-beta.1"
    return target.replace(/^v/, "");
  }
  return formatSemver(cur);
}

function setVersion(newVersion) {
  console.log(`Bumping version: ${getCurrentVersion()} → ${newVersion}`);

  for (const file of VERSION_FILES) {
    const filePath = resolve(ROOT, file.path);

    if (file.format === "json") {
      const data = readJson(filePath);
      data[file.key] = newVersion;
      writeJson(filePath, data);
    } else if (file.format === "toml") {
      let content = readToml(filePath);
      content = content.replace(/^version\s*=\s*"[^"]*"/m, `version = "${newVersion}"`);
      writeToml(filePath, content);
    }

    console.log(`  ✓ ${file.path}`);
  }

  console.log(`\nVersion set to ${newVersion}`);
  return newVersion;
}

// --- Main ---
const arg = process.argv[2];

if (!arg || arg === "--current") {
  console.log(getCurrentVersion());
  process.exit(0);
}

if (arg === "--help" || arg === "-h") {
  console.log(`Usage: node scripts/version.mjs <new-version|patch|minor|major|--current>`);
  console.log(`Current version: ${getCurrentVersion()}`);
  process.exit(0);
}

try {
  const current = getCurrentVersion();
  const newVersion = bumpVersion(current, arg);

  if (newVersion === current) {
    console.log(`Version already at ${current}, nothing to do.`);
    process.exit(0);
  }

  setVersion(newVersion);
} catch (err) {
  console.error(`Error: ${err.message}`);
  process.exit(1);
}
