// Points core.hooksPath at the committed .githooks directory.
// Wired to npm prepare so a fresh clone gets the gates after npm install.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const hooksDir = join(root, ".githooks");

function git(args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
}

try {
  // A packaged copy or an extracted tarball has no repository to configure.
  if (!existsSync(join(root, ".git"))) process.exit(0);
  for (const name of ["pre-commit", "commit-msg"]) {
    if (process.platform !== "win32") chmodSync(join(hooksDir, name), 0o755);
  }
  let current = "";
  try {
    current = git(["config", "--get", "core.hooksPath"]);
  } catch {
    current = "";
  }
  if (current === hooksDir) {
    console.log("  git hooks already installed: " + hooksDir);
    process.exit(0);
  }
  if (current) console.warn("  replacing core.hooksPath " + current + " with " + hooksDir);
  git(["config", "core.hooksPath", hooksDir]);
  console.log("  git hooks installed: " + hooksDir);
} catch (error) {
  console.warn("  git hooks not installed: " + (error.message ?? error));
  process.exit(0);
}
