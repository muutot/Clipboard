// Resolve and validate the exact release tag before verification/building.
// --resolve inspects the tagged Git tree and exports its immutable SHA; normal
// mode also requires the checked-out HEAD to equal that previously resolved tag.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";

const resolving = process.argv.includes("--resolve");
const git = (...args) =>
  execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
const failures = [];
const dispatch =
  process.env.GITHUB_EVENT_NAME === "workflow_dispatch" ||
  (!process.env.GITHUB_EVENT_NAME && process.env.GITHUB_REF_TYPE === "branch");
const input = dispatch
  ? process.env.GITHUB_EVENT_INPUTS_VERSION
  : process.env.GITHUB_REF_TYPE === "tag"
    ? process.env.GITHUB_REF_NAME
    : undefined;
if (!input || !/^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(input)) {
  console.error("Release identity requires a version or tag in the form [v]x.y.z[-prerelease]");
  process.exit(1);
}
const version = input.replace(/^v/, "");
const tag = `v${version}`;
let sha;
try {
  sha = git("rev-parse", "--verify", `refs/tags/${tag}^{commit}`);
} catch {
  console.error(`Release tag ${tag} does not resolve to an existing commit`);
  process.exit(1);
}
if (!dispatch && process.env.GITHUB_SHA) {
  try {
    if (git("rev-parse", "--verify", `${process.env.GITHUB_SHA}^{commit}`) !== sha) {
      failures.push("release tag moved after the triggering push");
    }
  } catch {
    failures.push("could not resolve the triggering push commit");
  }
}
if (!resolving && git("rev-parse", "HEAD") !== sha) {
  failures.push("checked-out HEAD does not equal the requested release tag");
}
if (process.env.RELEASE_SHA && process.env.RELEASE_SHA !== sha) {
  failures.push("release tag no longer equals the resolved release SHA");
}
if (process.env.RELEASE_TAG && process.env.RELEASE_TAG !== tag) {
  failures.push("release tag does not equal the resolved release tag");
}

const readVersionFile = (file) =>
  resolving ? git("show", `${sha}:${file}`) : readFileSync(file, "utf8");
try {
  const declared = {
    "package.json": JSON.parse(readVersionFile("package.json")).version,
    "src-tauri/tauri.conf.json": JSON.parse(readVersionFile("src-tauri/tauri.conf.json")).version,
    "src-tauri/Cargo.toml":
      readVersionFile("src-tauri/Cargo.toml").match(/^version\s*=\s*"([^"]+)"/m)?.[1],
  };
  for (const [file, declaredVersion] of Object.entries(declared)) {
    if (declaredVersion !== version)
      failures.push(`${file}: declared version does not match ${tag}`);
  }
} catch {
  failures.push("could not parse release version files");
}

// Dispatch and push must satisfy the same release-commit rules.
const expectedSubject = `\u{1F516} chore[release]: bump version to ${version}`;
if (git("log", "-1", "--pretty=%s", sha) !== expectedSubject) {
  failures.push("tag does not point at the release commit with the expected subject");
}
const releaseFiles = new Set([
  "package.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
  "CHANGELOG.md",
  "RELEASE.md",
]);
const headFiles = git("diff-tree", "--root", "--no-commit-id", "--name-only", "-r", sha)
  .split("\n")
  .filter(Boolean);
const unexpected = headFiles.filter((file) => !releaseFiles.has(file));
if (unexpected.length > 0)
  failures.push(`release commit touches non-release files: ${unexpected.join(", ")}`);
if (git("rev-list", "--parents", "-n", "1", sha).split(" ").length > 2) {
  failures.push("release commit must not be a merge commit");
}

if (failures.length > 0) {
  console.error("Release identity validation failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
if (resolving && process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `release_tag=${tag}\nrelease_sha=${sha}\n`);
}
console.log(`Release identity ok: ${tag} (${sha})`);
