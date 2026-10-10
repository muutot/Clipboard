// Pure validation of the Clipboard gitmoji commit-message contract.
// Rules live in skills/clipboard-dev/SKILL.md (Commit message format).
// No git or filesystem access here so the rules stay unit-testable.

export const TYPES = ["feat", "fix", "docs", "refactor", "style", "perf", "test", "chore"];

// The SKILL.md table, plus the two extra glyphs this repository already uses.
// 🔖 is the release-flow emoji for chore[release]; 🛡️ was used for a feat[ocr] fix.
export const GITMOJI_TYPES = new Map([
  ["✨", ["feat"]],
  ["🐛", ["fix"]],
  ["📝", ["docs"]],
  ["♻️", ["refactor"]],
  ["🎨", ["style"]],
  ["🚀", ["perf"]],
  ["✅", ["test"]],
  ["🔧", ["chore"]],
  ["🎉", ["chore"]],
  ["🗃️", ["feat"]],
  ["🔒", ["feat"]],
  ["🔍", ["feat"]],
  ["🔌", ["feat"]],
  ["🔄", ["feat"]],
  ["⌨️", ["feat"]],
  ["⚙️", ["feat"]],
  ["💾", ["feat"]],
  ["📦", ["feat"]],
  ["🔎", ["feat"]],
  ["🏷️", ["feat"]],
  ["💬", ["fix"]],
  ["🙈", ["fix"]],
  ["📁", ["feat"]],
  ["📂", ["feat"]],
  ["🔡", ["fix"]],
  ["✏️", ["fix"]],
  ["⚡", ["chore"]],
  ["🛠️", ["feat"]],
  ["🔖", ["chore"]],
  ["🛡️", ["feat"]],
]);

const SUBJECT_PATTERN =
  /^(\p{Extended_Pictographic}\uFE0F?(?:\u200D\p{Extended_Pictographic}\uFE0F?)*)\s+([a-z]+)(?:\[([^\]]*)\])?:\s+(.*)$/u;
const SCOPE_PATTERN = /^[a-z][a-z0-9,-]*$/;
const CJK_PATTERN = /[\u3000-\u303F\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF\uFF00-\uFFEF]/;
const SKIPPED_PREFIXES = ["Merge ", "Revert ", "fixup! ", "squash! ", "amend! "];

/**
 * Validates one commit message against the repository gitmoji contract.
 * Returns blocking errors and advisory warnings so the hook can report both.
 */
export function validateCommitMessage(message) {
  const errors = [];
  const warnings = [];
  const text = String(message ?? "").replace(/\r\n/g, "\n");
  const subject = (text.split("\n")[0] ?? "").trim();
  const skipped = SKIPPED_PREFIXES.some((prefix) => subject.startsWith(prefix));
  if (skipped) return { errors, warnings, skipped: true };
  const match = subject.match(SUBJECT_PATTERN);
  if (!match) {
    errors.push("subject must follow <gitmoji> <type>[<scope>]: <message>");
    errors.push('example: "✨ feat[search]: add pagination"');
    return { errors, warnings, skipped: false };
  }
  const emoji = match[1];
  const type = match[2];
  const scope = match[3];
  const summary = match[4];
  if (!TYPES.includes(type)) errors.push('type "' + type + '" is not one of: ' + TYPES.join(", "));
  const allowed = GITMOJI_TYPES.get(emoji);
  if (!allowed) {
    warnings.push("gitmoji " + emoji + " is not in the SKILL.md table; confirm the mapping");
  } else if (!allowed.includes(type)) {
    errors.push("gitmoji " + emoji + " maps to " + allowed.join(", ") + ", not " + type);
  }
  if (scope !== undefined && !SCOPE_PATTERN.test(scope)) {
    errors.push('scope "' + scope + '" must be lowercase, e.g. [storage] or [ocr,platform]');
  }
  if (!summary.trim()) errors.push("the subject needs a message after the colon");
  else if (/^[A-Z]/.test(summary)) {
    warnings.push("start the subject message with a lowercase imperative word");
  }
  if (CJK_PATTERN.test(text))
    errors.push("commit messages must be English only (CJK characters found)");
  return { errors, warnings, skipped: false, emoji, type, scope, summary };
}
