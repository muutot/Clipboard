// Git commit-msg hook for the Clipboard repository.
// Enforces the gitmoji contract documented in skills/clipboard-dev/SKILL.md.
import { readFileSync } from "node:fs";
import { validateCommitMessage } from "./lib/commit-message.mjs";

const messageFile = process.argv[2];
if (!messageFile) {
  console.error("commit-msg gate: no message file argument");
  process.exit(2);
}

const result = validateCommitMessage(readFileSync(messageFile, "utf8"));
if (result.skipped) process.exit(0);
for (const warning of result.warnings) console.warn("  commit-msg warning: " + warning);
if (result.errors.length === 0) process.exit(0);

console.error("");
console.error("commit-msg gate (skills/clipboard-dev/SKILL.md): message rejected");
for (const error of result.errors) console.error("  - " + error);
console.error("");
console.error("  format: <gitmoji> <type>[<scope>]: <message>");
console.error("  example: 🐛 fix[viewer]: correct fullscreen crash");
console.error("  fix the message and retry; --no-verify stays a deliberate exception.");
process.exit(1);
