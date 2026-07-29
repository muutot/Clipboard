#!/usr/bin/env node
/**
 * release.mjs — Full release orchestration.
 *
 * Flow:
 *   1. Verify project health (npm run verify)
 *   2. Bump version across all configs
 *   3. Generate changelog from commits since last tag
 *   4. Generate changelog from commits since last tag (CHANGELOG.md)
 *   5. Verify RELEASE.md — exit if stale; YOU update it, then re-run (the script auto-skips done steps)
 *   6. Commit version bump + changelog + RELEASE.md
 *   7. Create git tag
 *   8. Build release artifacts
 *   9. Report results
 *
 * Special mode: --regenerate
 *   Re-generates changelog from scratch for the current version
 *   Useful for re-releasing with updated content
 *
 * Usage:
 *   node scripts/release.mjs <version>              # normal release
 *   node scripts/release.mjs patch|minor|major      # semantic bump
 *   node scripts/release.mjs --regenerate <version> # re-release current content
 *   node scripts/release.mjs --dry-run <version>    # preview without committing
 */

import { execSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { argv, exit } from "node:process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");
const RELEASE_PATH = resolve(ROOT, "RELEASE.md");

function run(cmd, opts = {}) {
  console.log(`  > ${cmd}`);
  try {
    return execSync(cmd, {
      cwd: ROOT,
      encoding: "utf-8",
      stdio: opts.silent ? "pipe" : "inherit",
      shell: true,
      ...opts,
    });
  } catch (err) {
    console.error(`\n  ERROR: Command failed: ${cmd}`);
    console.error(err.stderr || err.message);
    exit(1);
  }
}

function getCurrentVersion() {
  return JSON.parse(readFileSync(resolve(ROOT, "package.json"), "utf-8")).version;
}

function isDirty() {
  try {
    const status = execSync("git status --porcelain", { cwd: ROOT, encoding: "utf-8" });
    return status.trim().length > 0;
  } catch {
    return true;
  }
}

function hasUnpushedCommits() {
  try {
    const branch = execSync("git rev-parse --abbrev-ref HEAD", {
      cwd: ROOT,
      encoding: "utf-8",
    }).trim();
    const ahead = execSync(`git rev-list --count origin/${branch}..HEAD 2>nul`, {
      cwd: ROOT,
      encoding: "utf-8",
      shell: true,
    }).trim();
    return parseInt(ahead || "0") > 0;
  } catch {
    return true;
  }
}

function printBanner(version) {
  console.log(`
╔══════════════════════════════════════════╗
║      Clipboard Desktop Release          ║
║            v${version.padEnd(26)}║
╚══════════════════════════════════════════╝
`);
}

// --- Main ---
const args = argv.slice(2);
const isDryRun = args.includes("--dry-run");
const isRegenerate = args.includes("--regenerate");
const versionArg = args.filter((a) => !a.startsWith("--"))[0];

if (!versionArg) {
  console.log(
    `Usage: node scripts/release.mjs [--dry-run] [--regenerate] <version|patch|minor|major>`,
  );
  console.log(`Current version: ${getCurrentVersion()}`);
  exit(1);
}

let currentVersion = getCurrentVersion();
const mode = isRegenerate ? "REGENERATE" : isDryRun ? "DRY RUN" : "RELEASE";
let didDrop = false;

// Step 1: Pre-flight checks
console.log(`\n[1/7] Pre-flight checks (${mode})...`);
if (!isDryRun && !isRegenerate) {
  if (isDirty()) {
    console.error("  ERROR: Working directory is dirty. Commit or stash changes first.");
    exit(1);
  }
  console.log("  ✓ Working directory clean");
}

// Step 1b: In regenerate mode, drop the old release commit from history if tag exists
if (isRegenerate) {
  const tagVersion = `v${versionArg}`;
  const tagExists =
    execSync(`git tag -l "${tagVersion}"`, {
      cwd: ROOT,
      encoding: "utf-8",
    }).trim() === tagVersion;

  if (tagExists) {
    const tagCommit = execSync(`git rev-list -n 1 "${tagVersion}"`, {
      cwd: ROOT,
      encoding: "utf-8",
    }).trim();
    const commitMsg = execSync(`git log --format="%s" -1 "${tagCommit}"`, {
      cwd: ROOT,
      encoding: "utf-8",
    }).trim();
    const shortSha = tagCommit.slice(0, 7);

    if (commitMsg.includes("chore[release]") || commitMsg.includes("bump version to")) {
      console.log(`  Found existing ${tagVersion} at ${shortSha}: "${commitMsg}"`);
      if (!isDryRun) {
        const parentSha = execSync(`git rev-list --parents -n 1 "${tagCommit}"`, {
          cwd: ROOT,
          encoding: "utf-8",
        })
          .trim()
          .split(" ")[1]; // second word = first parent
        console.log(
          `  Dropping old release commit ${shortSha} via rebase (parent ${parentSha.slice(0, 7)})...`,
        );
        run(`git rebase --onto ${parentSha} ${tagCommit}`);
        run(`git tag -d ${tagVersion}`);
        didDrop = true;
        currentVersion = getCurrentVersion(); // re-read after rebase reverted version files
        console.log("  ✓ Old release commit removed from history, tag deleted\n");
      } else {
        console.log("  (would drop via rebase in real run)\n");
      }
    }
  }
}

// Step 2: Verify
console.log("\n[2/7] Running verification...");
if (!isDryRun) {
  run("npm run verify");
} else {
  console.log("  (skipped in dry-run mode)");
}

// Step 3: Bump version
console.log("\n[3/7] Bumping version...");
let newVersion;
if (isRegenerate && currentVersion === versionArg) {
  // No old tag to undo — version is already at target, keep it
  newVersion = currentVersion;
  console.log(`  Version stays at ${currentVersion} (regenerate mode)`);
} else {
  run(`node scripts/version.mjs ${versionArg}`);
  newVersion = getCurrentVersion();
}

// Step 3.5: Update Cargo.lock to match the new version
console.log("  > cargo generate-lockfile (sync Cargo.lock)");
execSync("cargo generate-lockfile --manifest-path src-tauri/Cargo.toml", {
  cwd: ROOT,
  encoding: "utf-8",
  stdio: "pipe",
});
console.log("  ✓ Cargo.lock updated");

// Step 4: Generate changelog
console.log("\n[4/7] Generating changelog...");
if (isRegenerate) {
  run("node scripts/changelog.mjs --all");
} else {
  run("node scripts/changelog.mjs");
}

// Step 5: Verify RELEASE.md (exit if stale — update it, commit, then re-run)
console.log("\n[5/7] Checking RELEASE.md...");
if (existsSync(RELEASE_PATH)) {
  const releaseBody = readFileSync(RELEASE_PATH, "utf-8");
  if (!releaseBody.includes(`v${newVersion}`)) {
    console.log(`  RELEASE.md is stale (still references a different version).`);
    console.log(`  → Read CHANGELOG.md and update RELEASE.md following the template.`);
    console.log(`  → git add RELEASE.md && git commit -m "docs: update RELEASE.md for v${newVersion}"`);
    console.log(`  → Then re-run this script (already-bumped steps will be skipped).`);
    process.exit(0);
  }
  console.log(`  ✓ RELEASE.md matches v${newVersion}`);
} else {
  console.log("  (RELEASE.md not found, skipping)");
}

// Step 6: Commit and tag
if (!isDryRun) {
  // Include RELEASE.md in changed files even if prettier didn't touch it
  const changedFiles = execSync("git diff --name-only", { cwd: ROOT, encoding: "utf-8" }).trim();

  if (changedFiles) {
    const tagVersion = `v${newVersion}`;
    const releaseBranch = `release/${tagVersion}`;

    console.log("\n[6/7] Committing and tagging...");

    // Stage changed files
    const files = changedFiles.split("\n").join(" ");
    run(`git add ${files}`);

    const commitMsg = `🔖 chore[release]: bump version to ${newVersion}`;
    run(`git commit -m "${commitMsg}"`);

    // Create tag
    const tagMsg = `Release ${tagVersion}`;
    run(`git tag -a ${tagVersion} -m "${tagMsg}"`);

    console.log(`\n  ✓ Committed and tagged ${tagVersion}`);
  } else {
    console.log("\n[6/7] No changes to commit (version already at target).");
  }
} else {
  console.log("\n[6/7] Commit and tag (skipped in dry-run mode)");
}

// Step 7: Build
console.log("\n[7/7] Building release artifacts...");
if (!isDryRun) {
  run("npm run tauri build");
} else {
  console.log("  (skipped in dry-run mode)");
}

// Done
printBanner(newVersion);
console.log(`Release ${newVersion} complete!`);
console.log(`\n  Tag: v${newVersion}`);
console.log(`  Bundle: src-tauri/target/release/bundle/`);
console.log(`\n  To publish:`);
if (didDrop) {
  console.log(`    git push origin core --force-with-lease  # rewrite the old release commit`);
  console.log(`    git push origin v${newVersion} --force    # replace remote tag`);
} else {
  console.log(`    git push origin core`);
  console.log(`    git push origin v${newVersion}`);
}
