#!/usr/bin/env node
/**
 * delete-commit.mjs — Locally drop a single commit from history via rebase.
 *
 * This is a LOCAL-ONLY, history-rewriting operation. It NEVER touches the
 * remote. The branch tip is backed up to `refs/backup/pre-delete-<sha>` first,
 * and on any failure the branch is restored automatically.
 *
 * Improvements over the one-off Python version:
 *   - The target commit is a CLI argument (no hardcoded hash).
 *   - Repo-consistent: .mjs, same exec style as release.mjs.
 *   - `--dry-run` previews the replay range + date-mismatch count without
 *     touching anything.
 *   - `--branch <name>` overrides the current branch.
 *   - Uses a single `matchAll` pass for the author/committer date check.
 *
 * Usage:
 *   node scripts/delete-commit.mjs <commit> [--branch <name>] [--dry-run]
 */

import { execSync } from "node:child_process";
import { argv, exit } from "node:process";

const ROOT = process.cwd();
const BACKUP_PREFIX = "refs/backup/pre-delete";

function git(args, { silent = false } = {}) {
  const cmd = `git ${args}`;
  if (!silent) console.log(`  > ${cmd}`);
  return execSync(cmd, {
    cwd: ROOT,
    encoding: "utf-8",
    stdio: silent ? "pipe" : "inherit",
    shell: true,
  });
}
function gitOut(args) {
  return git(args, { silent: true }).trim();
}
function gitOk(args) {
  try {
    execSync(`git ${args}`, { cwd: ROOT, encoding: "utf-8", stdio: "pipe", shell: true });
    return true;
  } catch {
    return false;
  }
}

// --- Parse args ---
const args = argv.slice(2);
const isDryRun = args.includes("--dry-run");
let branchArg = null;
const bi = args.indexOf("--branch");
if (bi !== -1) branchArg = args[bi + 1];
const commit = args.find((a) => a !== "--dry-run" && a !== "--branch" && a !== branchArg);

if (!commit) {
  console.log("Usage: node scripts/delete-commit.mjs <commit> [--branch <name>] [--dry-run]");
  exit(1);
}

// --- Resolve branch ---
const branch = branchArg || gitOut("branch --show-current");
if (!branch) {
  console.error("Detached HEAD. Check out a branch or pass --branch <name>.");
  exit(1);
}

// --- Preconditions ---
// NOTE: avoid `^` / `{}` revision syntax — under Windows `shell: true` runs via
// cmd.exe, which consumes `^` as an escape char. Use `cat-file -t` (no caret) to
// confirm the object is a commit, and `~1` (cmd-safe) to resolve the parent.
const objType = gitOk(`cat-file -t ${commit}`) ? gitOut(`cat-file -t ${commit}`) : "";
if (objType !== "commit") {
  console.error(`Commit '${commit}' does not exist or is not a commit.`);
  exit(1);
}

const tip = gitOut(`rev-parse ${branch}`);
if (!gitOk(`merge-base --is-ancestor ${commit} ${tip}`)) {
  console.error(`Commit '${commit}' is not an ancestor of branch '${branch}'.`);
  exit(1);
}

const parent = gitOk(`rev-parse --verify --quiet ${commit}~1`)
  ? gitOut(`rev-parse --verify --quiet ${commit}~1`)
  : "";
if (!parent) {
  console.error(`Commit '${commit}' is the root commit and cannot be removed this way.`);
  exit(1);
}

if (!gitOk("diff --quiet") || !gitOk("diff --cached --quiet")) {
  console.error("Working tree is not clean. Commit or stash changes first.");
  exit(1);
}

// --- Report what will be replayed ---
const short = commit.slice(0, 12);
const replayRange = gitOut(`rev-list ${commit}..${tip}`).split(/\r?\n/).filter(Boolean);

const DATE_RE = /^(?:author|committer) .*? (\d+) [+\-]\d{4}/gm;
let mismatch = 0;
for (const sha of replayRange) {
  const body = gitOut(`cat-file commit ${sha}`);
  const ts = [...body.matchAll(DATE_RE)].map((m) => m[1]);
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

if (replayRange.length === 0) {
  console.log(`  '${commit}' is the tip — it will simply be removed.`);
} else {
  console.log(`  ${replayRange.length} commit(s) after it will be replayed (new SHAs):`);
  for (const sha of replayRange)
    console.log(`    ${sha.slice(0, 12)}  ${gitOut(`log --format=%s -1 ${sha}`)}`);
}

if (isDryRun) {
  console.log("\n[dry-run] No changes made. Re-run without --dry-run to apply.");
  exit(0);
}

// --- Back up and rewrite ---
const backupRef = `${BACKUP_PREFIX}-${short}`;
git(`update-ref ${backupRef} ${tip}`);
console.log(`\n  Backup of '${branch}' saved at ${backupRef} (old tip ${tip.slice(0, 12)})`);

console.log(
  `  Dropping ${short} via rebase --committer-date-is-author-date (preserving timestamps)...`,
);
try {
  git(`rebase --committer-date-is-author-date --onto ${parent} ${commit} ${branch}`);
} catch {
  try {
    git("rebase --abort");
  } catch {
    // no rebase in progress
  }
  git(`update-ref refs/heads/${branch} ${tip}`);
  console.error(
    `\n  ERROR: rebase failed (likely conflicts); '${branch}' restored to its previous tip.`,
  );
  console.error(`  Restore if needed: git update-ref refs/heads/${branch} ${backupRef}`);
  exit(1);
}

const newTip = gitOut(`rev-parse ${branch}`);
console.log(`\n✓ Dropped ${short}: '${branch}' ${tip.slice(0, 12)} → ${newTip.slice(0, 12)}`);
console.log(`  Restore if needed: git update-ref refs/heads/${branch} ${backupRef}`);
