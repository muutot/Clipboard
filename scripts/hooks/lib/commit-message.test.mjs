import { describe, expect, it } from "vitest";
import { validateCommitMessage } from "./commit-message.mjs";

describe("validateCommitMessage", () => {
  it("accepts the gitmoji subjects this repository uses", () => {
    const subjects = [
      "🐛 fix[toolbar]: close filter dropdowns with Escape",
      "🐛 fix[ocr,platform]: remove panic paths on icon and engine guard unwraps",
      "🔖 chore[release]: bump version to 1.7.4",
      "🛡️ feat[ocr]: clamp the OCR input size so a huge image cannot be fully decoded",
      "♻️ refactor[settings]: extract the settings-search controller",
      "✅ test[settings]: wait on the clock for lazily loaded search results",
      "🔧 chore[ci]: cap job runtime so a hung test fails instead of stalling",
      "🐛 fix: handle a missing store",
    ];
    for (const subject of subjects) {
      expect(validateCommitMessage(subject).errors, subject).toEqual([]);
    }
  });

  it("rejects a subject without a gitmoji", () => {
    const result = validateCommitMessage("fix[toolbar]: close filter dropdowns with Escape");
    expect(result.errors.length).toBeGreaterThan(0);
  });

  it("rejects a gitmoji that contradicts the type", () => {
    expect(validateCommitMessage("✨ fix[search]: add pagination").errors.length).toBeGreaterThan(
      0,
    );
  });

  it("rejects unknown types, malformed scopes and non-English text", () => {
    expect(
      validateCommitMessage("🐛 bugfix[search]: add pagination").errors.length,
    ).toBeGreaterThan(0);
    expect(validateCommitMessage("🐛 fix[Search]: add pagination").errors.length).toBeGreaterThan(
      0,
    );
    expect(validateCommitMessage("🐛 fix[search]: 修复分页").errors.length).toBeGreaterThan(0);
  });

  it("skips generated subjects such as merges and reverts", () => {
    expect(validateCommitMessage("Merge branch v1.7 into v1.8.0").skipped).toBe(true);
    expect(validateCommitMessage("Revert a broken change").skipped).toBe(true);
  });
});
