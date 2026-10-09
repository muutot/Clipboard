import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const scripts = dirname(fileURLToPath(import.meta.url));
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "clipboard-release-identity-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const git = (...args) =>
    execFileSync(
      "git",
      [
        "-c",
        "user.name=Release Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=.unused-hooks",
        ...args,
      ],
      { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    ).trim();
  git("init", "-q");
  writeFileSync(join(root, "app.txt"), "original code\n");
  git("add", ".");
  git("commit", "-qm", "initial fixture");
  mkdirSync(join(root, "src-tauri"));
  for (const path of ["package.json", "src-tauri/tauri.conf.json"]) {
    writeFileSync(join(root, path), '{"version":"1.2.3"}\n');
  }
  writeFileSync(join(root, "src-tauri/Cargo.toml"), '[package]\nversion = "1.2.3"\n');
  for (const path of ["src-tauri/Cargo.lock", "CHANGELOG.md", "RELEASE.md"]) {
    writeFileSync(join(root, path), "release fixture\n");
  }
  git("add", ".");
  git("commit", "-qm", "🔖 chore[release]: bump version to 1.2.3");
  const sha = git("rev-parse", "HEAD");
  git("tag", "-a", "v1.2.3", "-m", "fixture tag");
  const run = (extra = {}, args = []) => {
    const env = {
      ...process.env,
      GITHUB_EVENT_NAME: "workflow_dispatch",
      GITHUB_REF_TYPE: "branch",
      GITHUB_REF_NAME: "fixture",
      GITHUB_EVENT_INPUTS_VERSION: "1.2.3",
    };
    delete env.GITHUB_OUTPUT;
    delete env.RELEASE_SHA;
    delete env.RELEASE_TAG;
    Object.assign(env, extra);
    return spawnSync(process.execPath, [join(scripts, "validate-release-version.mjs"), ...args], {
      cwd: root,
      env,
      encoding: "utf8",
    });
  };
  return { root, git, sha, run };
}

test("dispatch rejects newer same-version code instead of publishing it under an old tag", (t) => {
  const { root, git, run } = fixture(t);
  writeFileSync(join(root, "app.txt"), "unreleased code\n");
  git("add", ".");
  git("commit", "-qm", "unreleased fix");
  const result = run();
  assert.notEqual(result.status, 0, result.stdout + result.stderr);
});

test("dispatch accepts both input prefixes and resolves the tag tree from a newer branch", (t) => {
  const { root, git, sha, run } = fixture(t);
  for (const input of ["1.2.3", "v1.2.3"]) {
    const result = run({ GITHUB_EVENT_INPUTS_VERSION: input });
    assert.equal(result.status, 0, result.stdout + result.stderr);
  }
  writeFileSync(join(root, "package.json"), '{"version":"9.9.9"}\n');
  git("add", ".");
  git("commit", "-qm", "unreleased branch");
  const output = join(root, "job-output");
  const result = run({ GITHUB_EVENT_INPUTS_VERSION: "v1.2.3", GITHUB_OUTPUT: output }, [
    "--resolve",
  ]);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(readFileSync(output, "utf8"), `release_tag=v1.2.3\nrelease_sha=${sha}\n`);
});

test("dispatch rejects missing tags and unsafe input before exporting job outputs", (t) => {
  const { run } = fixture(t);
  for (const input of ["1.2.4", "", "v1.2.3\nrelease_sha=bad", "../1.2.3"]) {
    const result = run({ GITHUB_EVENT_INPUTS_VERSION: input }, ["--resolve"]);
    assert.notEqual(result.status, 0, input);
  }
});

test("dispatch and push reject a tag on a non-release or mixed release commit", (t) => {
  const { root, git, run } = fixture(t);
  writeFileSync(join(root, "app.txt"), "changed code\n");
  git("add", ".");
  git("commit", "-qm", "ordinary fix");
  for (const subject of ["ordinary fix", "🔖 chore[release]: bump version to 1.2.3"]) {
    git("commit", "--amend", "-qm", subject);
    git("tag", "-f", "v1.2.3");
    for (const env of [
      {},
      { GITHUB_EVENT_NAME: "push", GITHUB_REF_TYPE: "tag", GITHUB_REF_NAME: "v1.2.3" },
    ]) {
      const result = run(env, ["--resolve"]);
      assert.notEqual(result.status, 0, result.stdout + result.stderr);
      assert.match(result.stderr, /non-release files|expected subject/);
    }
  }
});

test("push binds the triggering commit and consumers reject changed resolved identity", (t) => {
  const { sha, run } = fixture(t);
  const push = {
    GITHUB_EVENT_NAME: "push",
    GITHUB_REF_TYPE: "tag",
    GITHUB_REF_NAME: "v1.2.3",
    GITHUB_SHA: sha,
  };
  assert.equal(run(push, ["--resolve"]).status, 0);
  assert.notEqual(run({ ...push, GITHUB_SHA: "0".repeat(40) }, ["--resolve"]).status, 0);
  assert.notEqual(run({ RELEASE_SHA: "0".repeat(40) }).status, 0);
  assert.notEqual(run({ RELEASE_TAG: "v9.9.9" }).status, 0);
});

test("manual dispatch from a tag still uses its explicit version input", (t) => {
  const { run } = fixture(t);
  const result = run({
    GITHUB_REF_TYPE: "tag",
    GITHUB_REF_NAME: "v9.9.9",
    GITHUB_EVENT_INPUTS_VERSION: "v1.2.3",
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
