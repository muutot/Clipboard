#!/usr/bin/env node
/**
 * release.mjs — Release orchestration.
 *
 * Normal flow:
 *   1. Bump version across all configs
 *   2. Generate changelog from commits since last tag
 *   3. Verify RELEASE.md references the target version (exit for LLM to curate)
 *   4. Commit version files + CHANGELOG.md + RELEASE.md
 *   5. Create git tag
 *   6. Push to origin (triggers CI/CD)
 *
 * Regenerate mode (--regenerate):
 *   Before normal flow, drops the old release commit + tag from history
 *   (preserving other commits' content and timestamps), then runs normal flow.
 *
 * Usage:
 *   node scripts/release.mjs <version|patch|minor|major>
 *   node scripts/release.mjs --regenerate <version>
 *   node scripts/release.mjs --dry-run <version>
 */

import { execSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { argv, exit } from "node:process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");
const RELEASE_PATH = resolve(ROOT, "RELEASE.md");
const BRANCH = execSync("git rev-parse --abbrev-ref HEAD", { cwd: ROOT, encoding: "utf-8" }).trim();

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
    console.error(`\n  ERROR: ${err.stderr || err.message}`);
    exit(1);
  }
}

function getVersion() {
  return JSON.parse(readFileSync(resolve(ROOT, "package.json"), "utf-8")).version;
}

function checkReleaseMd(ver) {
  if (!existsSync(RELEASE_PATH)) return false;
  return readFileSync(RELEASE_PATH, "utf-8").includes(`v${ver}`);
}

// --- Parse args ---
const args = argv.slice(2);
const isRegenerate = args.includes("--regenerate");
const isDryRun = args.includes("--dry-run");
const versionArg = args.filter((a) => !a.startsWith("--"))[0];

if (!versionArg) {
  console.log(
    `Usage: node scripts/release.mjs [--regenerate] [--dry-run] <version|patch|minor|major>`,
  );
  console.log(`Current version: ${getVersion()}`);
  exit(1);
}

// --- Regenerate: drop old release commit + tag before normal flow ---
let forcePush = false;
if (isRegenerate) {
  const tagVer = `v${versionArg}`;
  const localTag =
    execSync(`git tag -l "${tagVer}"`, { cwd: ROOT, encoding: "utf-8" }).trim() === tagVer;

  const isAncestorOfHead = (sha) => {
    try {
      execSync(`git merge-base --is-ancestor "${sha}" HEAD`, {
        cwd: ROOT,
        encoding: "utf-8",
        stdio: "pipe",
      });
      return true;
    } catch {
      return false;
    }
  };

  // Find the old release commit for the target version: from the local tag, or by
  // scanning history (including remote-tracking refs) for the release message.
  let tagCommit = "";
  if (localTag) {
    tagCommit = execSync(`git rev-list -n 1 "${tagVer}"`, { cwd: ROOT, encoding: "utf-8" }).trim();
  }
  if (!tagCommit) {
    const match = execSync(
      `git log --all --format="%H %s" --grep="bump version to ${versionArg}" -n 1`,
      { cwd: ROOT, encoding: "utf-8" },
    ).trim();
    if (match) tagCommit = match.split(" ")[0];
  }

  if (tagCommit) {
    const shortSha = tagCommit.slice(0, 7);
    const commitMsg = execSync(`git log --format="%s" -1 "${tagCommit}"`, {
      cwd: ROOT,
      encoding: "utf-8",
    }).trim();

    if (commitMsg.includes("chore[release]") || commitMsg.includes("bump version to")) {
      console.log(`\n[Regenerate] Found old release commit ${shortSha}: "${commitMsg}"`);
      if (!isDryRun) {
        if (isAncestorOfHead(tagCommit)) {
          const parentSha = execSync(`git rev-list --parents -n 1 "${tagCommit}"`, {
            cwd: ROOT,
            encoding: "utf-8",
          })
            .trim()
            .split(" ")[1];
          console.log(
            `  Dropping commit ${shortSha} via filter-branch (preserving other commits' timestamps)...`,
          );
          // skip_commit "$@" re-parents later commits past the dropped one while
          // git commit-tree keeps author + committer dates of every surviving commit.
          const filter = `if [ "$GIT_COMMIT" = "${tagCommit}" ]; then skip_commit "$@"; else git commit-tree "$@"; fi`;
          run(`git filter-branch --force --commit-filter "${filter}" -- ${parentSha}..HEAD`, {
            env: { ...process.env, FILTER_BRANCH_SQUELCH_WARNING: "1" },
          });
          try {
            execSync(`git update-ref -d refs/original/refs/heads/${BRANCH}`, {
              cwd: ROOT,
              encoding: "utf-8",
              stdio: "pipe",
              shell: true,
            });
          } catch {
            // refs/original ref already removed
          }
        } else {
          console.log(
            `  Old release commit is not in the current branch (only on a remote ref); ` +
              `it will be dropped by the forced push.`,
          );
        }
        if (localTag) {
          run(`git tag -d ${tagVer}`);
        }
        forcePush = true;
        console.log(`  ✓ Old release commit removed, tag ${tagVer} deleted\n`);
      } else {
        console.log(`  (would drop ${shortSha} and tag ${tagVer} in real run)\n`);
      }
    }
  } else {
    console.log(
      `\n[Regenerate] No old release commit found for v${versionArg} — creating a fresh release.`,
    );
  }
}

// --- Normal flow ---
let currentVersion = getVersion();
let tagVersion = `v${currentVersion}`;

// Step 1: Bump version
console.log(`\n[1/6] Bumping version (${BRANCH})...`);
if (currentVersion !== versionArg) {
  run(`node scripts/version.mjs ${versionArg}`);
  currentVersion = getVersion();
  tagVersion = `v${currentVersion}`;
  run("cargo generate-lockfile --manifest-path src-tauri/Cargo.toml", { silent: true });
  console.log(`  ✓ ${currentVersion}`);
} else {
  console.log(`  ✓ Already at ${currentVersion}`);
}

// Step 2: Generate changelog
console.log("\n[2/6] Generating changelog...");
run("node scripts/changelog.mjs");

// Step 3: RELEASE.md check
console.log("\n[3/6] Checking RELEASE.md...");
if (!checkReleaseMd(currentVersion)) {
  console.log(`  RELEASE.md needs update for v${currentVersion}.`);
  console.log("  → Read CHANGELOG.md and curate RELEASE.md, then re-run.");
  process.exit(0);
}
console.log(`  ✓ RELEASE.md matches v${currentVersion}`);

// Step 4: Commit
if (!isDryRun) {
  console.log("\n[4/6] Committing...");
  const changedFiles = execSync("git diff --name-only", { cwd: ROOT, encoding: "utf-8" }).trim();
  if (changedFiles) {
    run(`git add ${changedFiles.split("\n").join(" ")}`);
    run(`git commit -m "\u{1F516} chore[release]: bump version to ${currentVersion}"`);
    console.log("  ✓ Committed");
  } else {
    console.log("  No changes to commit.");
  }
} else {
  console.log("\n[4/6] Commit (skipped in dry-run mode)");
}

// Step 5: Tag
if (!isDryRun) {
  console.log("\n[5/6] Tagging...");
  const exists =
    execSync(`git tag -l "${tagVersion}"`, { cwd: ROOT, encoding: "utf-8" }).trim() === tagVersion;
  if (!exists) {
    run(`git tag -a ${tagVersion} -m "Release ${tagVersion}"`);
    console.log(`  ✓ ${tagVersion}`);
  } else {
    console.log(`  ✓ Tag ${tagVersion} already exists`);
  }
} else {
  console.log("\n[5/6] Tag (skipped in dry-run mode)");
}

// Step 6: Push
if (!isDryRun) {
  console.log("\n[6/6] Pushing...");

  // Auto-detect if force push is needed (regenerate rewrites history)
  let needsForce = forcePush;
  if (!needsForce) {
    try {
      const aheadBehind = execSync(`git rev-list --count --left-right origin/${BRANCH}...HEAD`, {
        cwd: ROOT,
        encoding: "utf-8",
      }).trim();
      const parts = aheadBehind.split(/\s+/).filter(Boolean);
      if (parts.length > 1) needsForce = true;
    } catch {
      // remote branch doesn't exist — first push, no force needed
    }
  }

  const branchFlag = needsForce ? "--force-with-lease" : "";
  const tagFlag = needsForce ? "--force" : "";
  run(`git push origin ${BRANCH} ${branchFlag}`.trim());
  run(`git push origin ${tagVersion} ${tagFlag}`.trim());
  console.log(`  ✓ Pushed ${BRANCH} and ${tagVersion}`);
} else {
  console.log("\n[6/6] Push (skipped in dry-run mode)");
}

console.log(`\n✓ Release ${currentVersion} complete! Tag: ${tagVersion}`);
