# Incremental Audit Prompt

Use the fuck-my-shit-mountain skill in **incremental mode**.

Shared setup, coverage, report template, HTML, and lint rules live in `references/report-format.md`; load that reference before producing the report.

Focus on auditing only the files that changed since a specified git reference (commit, branch, or tag). This mode is designed for PR reviews, pre-merge checks, and continuous auditing workflows.

## Audit Scope

Record the actual comparison before inspecting changes:

- `main...HEAD` compares the merge base with HEAD, appropriate for a branch/PR review.
- `v1.2.0..HEAD` or `abc123..HEAD` compares two endpoint trees; two dots do not mean merge-base comparison in `git diff`.
- For current uncommitted changes, use `git diff HEAD --` for tracked staged + unstaged changes and `git ls-files --others --exclude-standard` for untracked paths. Read untracked files separately; do not stage them.
- For staged-only review, use `git diff --cached --`.
- If the base cannot be inferred reliably, ask for it while inspecting independent repository context. Do not invent `main` or silently replace a supplied two-dot range with three dots.

Use `git diff --name-status -z -M <comparison> --` and `git diff <comparison> --` to identify changed/deleted/renamed paths and hunks. Handle names with spaces and parse NUL-delimited output in scripts. Do not interpolate untrusted references into shell code. In an unborn repository, inspect staged and untracked files directly and record that HEAD does not exist.

## Audit Areas

Incremental alone applies all dimensions (including concurrency) to the changed files. When combined with focused modes, apply only those dimensions. Focus on:

1. **New risks introduced** — vulnerabilities, crash paths, race conditions, data integrity issues
2. **Deleted safety mechanisms** — removed validation, error handling, tests, or security checks
3. **Behavioral changes** — modified logic that could break existing assumptions or contracts
4. **Dependency changes** — new dependencies, version upgrades, or removals
5. **Test coverage for changes** — whether new/modified code has corresponding tests
6. **Configuration changes** — new config keys, changed defaults, environment variables
7. **Migration or schema changes** — database migrations, API contract changes
8. **Documentation updates** — whether docs reflect the changes

## Context Awareness

When auditing changed files, consider:

- **Unchanged callers** — if a function signature or behavior changes, check if callers are still correct
- **Cross-file impacts** — if a shared module changes, consider downstream effects
- **Deleted code** — what depended on it, and is the deletion safe?
- **Renamed/moved files** — preserve rename context and check references and packaging paths

Use `git diff <ref>` to see the actual line-level changes, not just file names.

## Rules

1. **Focus on the diff.** Do not audit the entire codebase unless a change touches a critical shared component.
2. **Report only new risks.** Do not report pre-existing issues unless they are made worse by the changes.
3. **Check test coverage.** Report a missing test only when a meaningful behavior or failure path lacks protection; do not demand tests for reversible copy/formatting changes.
4. **Consider blast radius.** Trace affected callers and assess demonstrated impact; shared use alone does not automatically raise severity.
5. **Flag breaking changes.** API contract changes, schema migrations, and config key renames should be explicitly noted.
6. **Be proportional.** A 5-line bug fix doesn't need a 50-finding audit. Scale findings to the change size.

## Attitude

1. **Be exhaustively systematic.** Check every changed file, every modified function, every deleted safety check. Follow the skill's coverage strategy and document exclusions honestly.
2. **Do not be a yes-man.** Do not approve changes just because they are small or seem safe. If a one-line change introduces a race condition, say so.
3. **Context matters.** A change that would be fine in a new project may be risky in a mature codebase. Consider the project's maturity and stability requirements.

## Finding Format

Use `templates/issue-card.md` for the canonical fields, adding the diff-specific evidence below rather than replacing required fields.

### Finding: <short title>

- Severity: Critical / High / Medium / Low / Info
- Confidence: High / Medium / Low
- Category: <dimension>
- Status: Confirmed / Suspected
- Affected area:
- Evidence:
  - Changed files:
  - Changed functions:
  - Diff context: (git diff snippet if relevant)
  - Related unchanged code: (callers, dependencies)
- Change type: Added / Modified / Deleted / Renamed
- Risk introduced:
- Failure scenario:
- User-visible impact:
- Minimal fix:
- Better long-term fix:
- Regression test suggestion:
- Estimated effort:

## Report Additions for Incremental Mode

In addition to standard report sections, include:

### Change Summary

- Total files changed: <N>
- Lines added: <N>
- Lines deleted: <N>
- Commits in range: <N>
- Authors: <list>

### Change Categories

- New features: <N files>
- Bug fixes: <N files>
- Refactoring: <N files>
- Dependency updates: <N files>
- Configuration: <N files>
- Tests: <N files>
- Documentation: <N files>

### Risk Delta

- New risks introduced: <N>
- Existing risks fixed: <N>
- Existing risks made worse: <N>

### Test Coverage Delta

- New code with tests: <N files>
- New code without tests: <N files>
- Deleted tests: <N files>

### Approval Recommendation

Based on findings:

- **Approve** — inspected changes have no blocking findings; state coverage limits rather than guaranteeing safety
- **Approve with comments** — minor issues that can be addressed post-merge
- **Request changes** — issues that must be fixed before merge
- **Block** — critical issues that make the changes unsafe
