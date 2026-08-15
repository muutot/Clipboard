#!/usr/bin/env node
/**
 * changelog.mjs — Generate CHANGELOG.md from gitmoji commit history.
 *
 * Usage:
 *   node skills/version-release/scripts/changelog.mjs                     # generate for latest version
 *   node skills/version-release/scripts/changelog.mjs --from v0.1.0       # from a specific tag
 *   node skills/version-release/scripts/changelog.mjs --all               # full history (first release)
 *   node skills/version-release/scripts/changelog.mjs --preview           # preview without writing
 */

import { execSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";

const ROOT = execSync("git rev-parse --show-toplevel", { encoding: "utf-8" }).trim();
const CHANGELOG_PATH = resolve(ROOT, "CHANGELOG.md");

// Gitmoji → changelog section mapping
const TYPE_SECTION = {
  feat: "### ✨ Features",
  fix: "### 🐛 Bug Fixes",
  perf: "### 🚀 Performance",
  refactor: "### ♻️ Refactoring",
  style: "### 🎨 Styling",
  docs: "### 📝 Documentation",
  test: "### ✅ Testing",
  chore: "### 🔧 Chores",
  build: "### 🔧 Build",
  revert: "### ⏪ Reverts",
  i18n: "### 🌐 Internationalization",
  cleanup: "### 🧹 Cleanup",
};

function gitLog(fromRef) {
  const range = fromRef ? `${fromRef}..HEAD` : "HEAD";
  try {
    const output = execSync(`git log ${range} --format="%H||%ai||%s" --no-merges`, {
      cwd: ROOT,
      encoding: "utf-8",
      maxBuffer: 10 * 1024 * 1024,
      shell: true,
    });
    return output.trim().split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

function getLatestTag() {
  try {
    const tags = execSync("git tag --sort=-creatordate", {
      cwd: ROOT,
      encoding: "utf-8",
      shell: true,
    }).trim();
    return tags.split("\n").filter(Boolean)[0] || "";
  } catch {
    return "";
  }
}

function parseCommit(line) {
  // Format: <hash>||<date>||<gitmoji> <type>[<scope>]: <message>
  const parts = line.split("||");
  if (parts.length < 3) return null;
  const [hash, date, fullMsg] = parts;

  // Parse gitmoji format: emoji type[scope]: message
  const match = fullMsg.match(/^[^\x00-\x7f\x20]+[\x20]+([a-z0-9]+)(?:\[([^\]]+)\])?:\s+(.+)/);
  if (!match) return null;

  return {
    hash: hash.slice(0, 8),
    date: date.split(" ")[0], // YYYY-MM-DD
    type: match[1],
    scope: match[2] || "",
    message: match[3].trim(),
  };
}

function groupCommits(commits) {
  const sections = {};
  for (const c of commits) {
    const section = TYPE_SECTION[c.type] || "### 🔧 Chores";
    if (!sections[section]) sections[section] = [];
    const scope = c.scope ? `**${c.scope}**: ` : "";
    sections[section].push(`- ${scope}${c.message} (${c.hash})`);
  }
  return sections;
}

function generateChangelog(version, date, commits) {
  let md = `# Changelog\n\n`;
  md += `## ${version} (${date})\n\n`;

  const sections = groupCommits(commits);
  const sectionOrder = Object.keys(TYPE_SECTION); // preserve type order

  for (const sectionType of sectionOrder) {
    const sectionName = TYPE_SECTION[sectionType];
    if (sections[sectionName] && sections[sectionName].length > 0) {
      md += `${sectionName}\n\n`;
      for (const line of sections[sectionName]) {
        md += `${line}\n`;
      }
      md += "\n";
    }
  }

  // Add any uncategorized sections
  for (const [name, entries] of Object.entries(sections)) {
    if (!Object.values(TYPE_SECTION).includes(name) && entries.length > 0) {
      md += `${name}\n\n`;
      for (const line of entries) {
        md += `${line}\n`;
      }
      md += "\n";
    }
  }

  return md;
}

function getLatestVersion() {
  const pkg = JSON.parse(readFileSync(resolve(ROOT, "package.json"), "utf-8"));
  return pkg.version;
}

// --- Main ---
const args = process.argv.slice(2);
const isPreview = args.includes("--preview");
const isAll = args.includes("--all");
const fromIdx = args.indexOf("--from");
const fromRef = fromIdx >= 0 ? args[fromIdx + 1] : getLatestTag();

const lines = gitLog(isAll ? null : fromRef || null);
const commits = lines.map(parseCommit).filter(Boolean);

if (commits.length === 0) {
  console.log("No commits found for changelog.");
  process.exit(0);
}

const version = getLatestVersion();
const today = new Date().toISOString().split("T")[0];
const changelog = generateChangelog(version, today, commits);

if (isPreview) {
  console.log(changelog);
} else {
  // Prepend to existing or create new
  if (existsSync(CHANGELOG_PATH)) {
    const existing = readFileSync(CHANGELOG_PATH, "utf-8");
    // Remove the header line if it exists, then prepend new content
    const existingBody = existing.replace(/^# Changelog\n\n/, "");
    writeFileSync(CHANGELOG_PATH, changelog + existingBody, "utf-8");
  } else {
    writeFileSync(CHANGELOG_PATH, changelog, "utf-8");
  }

  // Re-format with prettier to keep lint clean
  try {
    execSync(`npx prettier --write "${CHANGELOG_PATH}"`, {
      cwd: ROOT,
      encoding: "utf-8",
      stdio: "pipe",
    });
  } catch {
    // non-fatal
  }

  console.log(`CHANGELOG.md updated with ${commits.length} commits for v${version}`);
  console.log(`  Sections: ${Object.keys(groupCommits(commits)).join(", ")}`);
}
