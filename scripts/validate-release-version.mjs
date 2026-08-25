// Validates that the release identity is consistent before any expensive
// build starts. Runs inside the release workflow's verify job:
//
//   - tag push (GITHUB_REF_TYPE=tag): the tag must match the version
//     declared in package.json, src-tauri/tauri.conf.json, and Cargo.toml.
//   - workflow_dispatch: the `version` input must match those same files,
//     guaranteeing the synthesized v<version> release name is correct.
//
// Exits non-zero on any mismatch so the workflow fails fast.
import { readFileSync } from "node:fs";

const failures = [];
const declared = {
  "package.json": JSON.parse(readFileSync("package.json", "utf8")).version,
  "src-tauri/tauri.conf.json": JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"))
    .version,
  "src-tauri/Cargo.toml": readFileSync("src-tauri/Cargo.toml", "utf8").match(
    /^version\s*=\s*"([^"]+)"/m,
  )?.[1],
};

for (const [file, version] of Object.entries(declared)) {
  if (!version) failures.push(`${file}: no parsable version field`);
}

const versions = new Set(Object.values(declared));
if (versions.size > 1) {
  failures.push(`version files disagree: ${JSON.stringify(declared)}`);
}
const expected = [...versions][0];

let source;
if (process.env.GITHUB_REF_TYPE === "tag") {
  source = process.env.GITHUB_REF_NAME?.replace(/^v/, "");
  if (source !== expected) {
    failures.push(`tag ${process.env.GITHUB_REF_NAME} != declared version ${expected}`);
  }
} else if (process.env.GITHUB_REF_TYPE === "branch") {
  // workflow_dispatch: the input version drives the synthesized tag name.
  const input = process.env.GITHUB_EVENT_INPUTS_VERSION ?? "";
  source = input.replace(/^v/, "");
  if (!source) {
    failures.push("workflow_dispatch requires a version input");
  } else if (source !== expected) {
    failures.push(`dispatch version ${input} != declared version ${expected}`);
  }
} else {
  failures.push(`unsupported GITHUB_REF_TYPE: ${process.env.GITHUB_REF_TYPE}`);
}

if (failures.length > 0) {
  console.error("Release identity validation failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}

console.log(`Release identity ok: v${expected} (${source ? `from ${source}` : "no source"})`);
