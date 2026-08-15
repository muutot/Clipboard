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
 *   6. Local-only: NO remote push — print the push commands for the user to run manually
 *
 * Regenerate mode (--regenerate):
 *   Before normal flow, drops the old release commit + tag from history via
 *   `git rebase --committer-date-is-author-date --onto <parent> <commit> <branch>`
 *   (preserving other commits' content and timestamps), then runs normal flow.
 *
 * Usage:
 *   node skills/version-release/scripts/release.mjs <version|patch|minor|major>
 *   node skills/version-release/scripts/release.mjs --regenerate <version>
 *   node skills/version-release/scripts/release.mjs --dry-run <version>
 */

import { execSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import { argv, exit } from "node:process";

const ROOT = execSync("git rev-parse --show-toplevel", { encoding: "utf-8" }).trim();
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
    `Usage: node skills/version-release/scripts/release.mjs [--regenerate] [--dry-run] <version|patch|minor|major>`,
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

  const gitOutput = (cmd) => execSync(cmd, { cwd: ROOT, encoding: "utf-8", stdio: "pipe" }).trim();

  const isCleanTree = () => {
    try {
      execSync("git diff --quiet", { cwd: ROOT, stdio: "pipe" });
      execSync("git diff --cached --quiet", { cwd: ROOT, stdio: "pipe" });
      return true;
    } catch {
      return false;
    }
  };

  // Find the old release commit for the target version: from the local tag, or by
  // scanning history (including remote-tracking refs) for the release message.
  let tagCommit = "";
  if (localTag) tagCommit = gitOutput(`git rev-list -n 1 "${tagVer}"`);
  if (!tagCommit) {
    const match = gitOutput(
      `git log --exclude="refs/backup/*" --all --format="%H %s" --grep="bump version to ${versionArg}" -n 1`,
    );
    if (match) tagCommit = match.split(" ")[0];
  }

  if (tagCommit) {
    const shortSha = tagCommit.slice(0, 7);
    const commitMsg = gitOutput(`git log --format="%s" -1 "${tagCommit}"`);

    if (commitMsg.includes("chore[release]") || commitMsg.includes("bump version to")) {
      console.log(`\n[Regenerate] Found old release commit ${shortSha}: "${commitMsg}"`);
      if (!isDryRun) {
        if (isAncestorOfHead(tagCommit)) {
          let parentSha = "";
          try {
            parentSha = gitOutput(`git rev-parse --verify --quiet "${tagCommit}^"`);
          } catch {
            // root commit — handled below
          }
          if (!parentSha) {
            console.error(
              `  ERROR: commit ${shortSha} is the root commit and cannot be removed this way.`,
            );
            exit(1);
          }
          if (!isCleanTree()) {
            console.error(
              "  ERROR: working tree is not clean. Commit or stash changes before regenerating.",
            );
            exit(1);
          }

          const tip = gitOutput(`git rev-parse ${BRANCH}`);
          const backupRef = `refs/backup/pre-release-delete-${shortSha}`;
          run(`git update-ref ${backupRef} ${tip}`);
          console.log(
            `  Backup of '${BRANCH}' saved at ${backupRef} (old tip ${tip.slice(0, 12)})`,
          );

          // Warn if any replayed commit has author date != committer date; the rebase
          // would rewrite those committer timestamps to the author date.
          const DATE_RE = /^(?:author|committer) .* (\d+) [+\-]\d{4}/gm;
          let mismatch = 0;
          for (const sha of gitOutput(`git rev-list "${tagCommit}..${tip}"`)
            .split(/\r?\n/)
            .filter(Boolean)) {
            const matches = gitOutput(`git cat-file commit ${sha}`).match(DATE_RE) || [];
            const ts = matches.map((m) => m.match(/(\d+) [+\-]\d{4}$/)[1]);
            if (ts.length >= 2 && ts[0] !== ts[ts.length - 1]) mismatch++;
          }
          if (mismatch) {
            console.warn(
              `  WARNING: ${mismatch} commit(s) in the replayed range have author date != ` +
                "committer date; their committer timestamps will be rewritten to the author date.",
            );
          } else {
            console.log(
              "  All replayed commits have author date == committer date: timestamps preserved exactly.",
            );
          }

          // Standard single-commit removal: replay everything after the old release
          // commit onto its parent. --committer-date-is-author-date keeps every
          // surviving commit's original timestamp.
          console.log(
            `  Dropping commit ${shortSha} via rebase --committer-date-is-author-date (preserving timestamps)...`,
          );
          try {
            execSync(
              `git rebase --committer-date-is-author-date --onto ${parentSha} ${tagCommit} ${BRANCH}`,
              { cwd: ROOT, encoding: "utf-8", stdio: "inherit", shell: true },
            );
          } catch {
            try {
              execSync(`git rebase --abort`, {
                cwd: ROOT,
                encoding: "utf-8",
                stdio: "inherit",
                shell: true,
              });
            } catch {
              // no rebase in progress
            }
            execSync(`git update-ref refs/heads/${BRANCH} ${tip}`, {
              cwd: ROOT,
              encoding: "utf-8",
              stdio: "pipe",
            });
            console.error(
              `\n  ERROR: rebase failed (likely conflicts); '${BRANCH}' restored to its previous tip.`,
            );
            console.error(`  Restore if needed: git update-ref refs/heads/${BRANCH} ${backupRef}`);
            exit(1);
          }

          const newTip = gitOutput(`git rev-parse ${BRANCH}`);
          console.log(
            `  ✓ Dropped ${shortSha}: '${BRANCH}' ${tip.slice(0, 12)} → ${newTip.slice(0, 12)}`,
          );
        } else {
          console.log(
            `  Old release commit is not in the current branch (only on a remote ref); ` +
              `drop it from the remote by force-pushing / deleting the remote tag manually.`,
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
  run(`node skills/version-release/scripts/version.mjs ${versionArg}`);
  currentVersion = getVersion();
  tagVersion = `v${currentVersion}`;
  run("cargo generate-lockfile --manifest-path src-tauri/Cargo.toml", { silent: true });
  console.log(`  ✓ ${currentVersion}`);
} else {
  console.log(`  ✓ Already at ${currentVersion}`);
}

// Step 2: Generate changelog
console.log("\n[2/6] Generating changelog...");
run("node skills/version-release/scripts/changelog.mjs");

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

// Step 6: Remote push — local-only by design.
// All git operations (bump, commit, tag, history rewrite) happen locally. This
// script NEVER pushes to any remote. The user pushes manually when ready; doing so
// triggers the GitHub Actions release workflow. After --regenerate (history rewrite)
// the user must force-push.
if (!isDryRun) {
  console.log("\n[6/6] Remote push — skipped (local-only by design)");
  console.log("  The release commit and tag exist only locally. Push them yourself:");
  console.log(`    git push origin ${BRANCH}`);
  console.log(`    git push origin ${tagVersion}`);
  if (forcePush) {
    console.log("  History was rewritten by --regenerate; use --force-with-lease when pushing:");
    console.log(`    git push origin ${BRANCH} --force-with-lease`);
    console.log(`    git push origin ${tagVersion} --force`);
  }
} else {
  console.log("\n[6/6] Push (skipped in dry-run mode)");
}

console.log(`\n✓ Release ${currentVersion} complete! Tag: ${tagVersion}`);
