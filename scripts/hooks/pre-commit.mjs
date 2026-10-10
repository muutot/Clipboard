// Clipboard pre-commit gate. Rules: skills/clipboard-dev/SKILL.md.
// Installed through core.hooksPath by scripts/install-git-hooks.mjs.
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { evaluateDocGate } from "./lib/doc-currency.mjs";

const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();

function git(args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" });
}

function attempt(run) {
  try {
    return { ok: true, output: run(), code: null };
  } catch (error) {
    return {
      ok: false,
      output: String(error.stdout ?? "") + String(error.stderr ?? ""),
      code: error.code ?? null,
    };
  }
}

function stagedPaths(args) {
  return git(args)
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

const staged = stagedPaths(["diff", "--cached", "--name-only"]);
const addedOrRemoved = stagedPaths(["diff", "--cached", "--diff-filter=AD", "--name-only"]);
const results = [];
const failures = [];
const notes = [];

// 1. Whitespace errors and unresolved conflict markers in the staged diff.
const whitespace = attempt(() => git(["diff", "--cached", "--check"]));
if (whitespace.ok) {
  results.push(["ok", "staged diff: no whitespace errors or conflict markers"]);
} else {
  results.push(["fail", "staged diff"]);
  failures.push(whitespace.output.trim());
}

// 2. Prettier formatting of the staged files it owns.
const PRETTIER_FILE = /\.(m?[jt]s|svelte|css|html|json|md|ya?ml)$/;
const prettierFiles = staged.filter(
  (path) => PRETTIER_FILE.test(path) && existsSync(join(root, path)),
);
const prettierBin = join(root, "node_modules", "prettier", "bin", "prettier.cjs");
if (prettierFiles.length === 0) {
  results.push(["skip", "prettier: no staged file it formats"]);
} else if (!existsSync(prettierBin)) {
  results.push(["note", "prettier: not installed, run npm install to enable this check"]);
} else {
  const formatted = attempt(() =>
    execFileSync(process.execPath, [prettierBin, "--check", ...prettierFiles], {
      cwd: root,
      encoding: "utf8",
    }),
  );
  if (formatted.ok)
    results.push(["ok", "prettier: " + prettierFiles.length + " staged file(s) formatted"]);
  else {
    results.push(["fail", "prettier"]);
    failures.push(
      formatted.output.trim() + "\n  run: npx prettier --write " + prettierFiles.join(" "),
    );
  }
}

// 3. Rust formatting when Rust sources are staged.
const rustFiles = staged.filter((path) => path.endsWith(".rs") && existsSync(join(root, path)));
if (rustFiles.length === 0) {
  results.push(["skip", "rustfmt: no staged Rust file"]);
} else {
  const formatted = attempt(() =>
    execFileSync(
      "cargo",
      ["fmt", "--manifest-path", "src-tauri/Cargo.toml", "--all", "--", "--check"],
      {
        cwd: root,
        encoding: "utf8",
      },
    ),
  );
  if (formatted.ok) results.push(["ok", "rustfmt: cargo fmt --check"]);
  else if (formatted.code === "ENOENT")
    results.push(["note", "rustfmt: cargo not found, check skipped"]);
  else {
    results.push(["fail", "rustfmt"]);
    failures.push(formatted.output.trim() + "\n  run: npm run format:rust");
  }
}

// 4. A release commit must only carry the version artifacts (version-release skill).
const VERSION_FILES = ["package.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml"];
const RELEASE_ALLOWED = [...VERSION_FILES, "src-tauri/Cargo.lock", "CHANGELOG.md", "RELEASE.md"];

function manifestVersion(path, text) {
  if (path.endsWith(".json")) {
    try {
      return JSON.parse(text).version ?? null;
    } catch {
      return null;
    }
  }
  const match = text.match(/^version\s*=\s*"([^"]+)"/m);
  return match ? match[1] : null;
}

const bumped = VERSION_FILES.filter((path) => {
  if (!staged.includes(path)) return false;
  const before = attempt(() => git(["show", "HEAD:" + path]));
  const after = attempt(() => git(["show", ":" + path]));
  return manifestVersion(path, before.output) !== manifestVersion(path, after.output);
});
if (bumped.length === 0) {
  results.push(["skip", "release commit: no version bump staged"]);
} else {
  const stray = staged.filter((path) => !RELEASE_ALLOWED.includes(path));
  if (stray.length === 0) results.push(["ok", "release commit: only version artifacts staged"]);
  else {
    results.push(["fail", "release commit"]);
    failures.push(
      "a version bump may only be committed with: " +
        RELEASE_ALLOWED.join(", ") +
        "\n  also staged: " +
        stray.join(", ") +
        "\n  commit the other changes first (see skills/version-release/SKILL.md)",
    );
  }
  notes.push(
    "pre-release gate: run npm run format:check, npm run check and npm run lint:rust on the final tree," +
      " and keep formatting-only edits in a separate 🎨 style commit",
  );
}

// 5. Documentation currency gate.
const gate = evaluateDocGate({ stagedPaths: staged, addedOrRemovedPaths: addedOrRemoved });
if (gate.status === "not-applicable") {
  results.push(["skip", "documentation currency: no documented source staged"]);
} else if (gate.status === "satisfied") {
  results.push(["ok", "documentation currency: " + gate.referencesTouched.join(", ")]);
} else if (gate.status === "reminder") {
  notes.push(
    "documentation currency: " +
      gate.refs.map((ref) => "skills/clipboard-dev/references/" + ref).join(", ") +
      " may need an update for the staged source files",
  );
} else if (process.env.CLIPBOARD_DOC_GATE === "none") {
  notes.push("documentation currency: waived by CLIPBOARD_DOC_GATE=none");
} else {
  results.push(["fail", "documentation currency"]);
  failures.push(
    "new or removed source files need a skills/clipboard-dev update:\n  " +
      gate.structural.join("\n  ") +
      "\n  expected reference: " +
      gate.refs.join(", ") +
      "\n  stage that reference, or set CLIPBOARD_DOC_GATE=none when none of them needs a change",
  );
}

console.log("");
console.log("pre-commit gate (skills/clipboard-dev/SKILL.md)");
for (const [state, label] of results) console.log("  " + state.padEnd(4) + " " + label);
for (const note of notes) console.log("  note " + note);
console.log("");
if (failures.length === 0) process.exit(0);
for (const failure of failures) console.log("  " + failure.split("\n").join("\n  "));
console.log("");
console.log("  commit rejected; fix the items above or use --no-verify deliberately.");
process.exit(1);
