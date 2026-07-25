# Clipboard Maintenance Workflow

## 1. Establish the baseline

1. Read this skill and `TODO.md` as UTF-8.
2. Inspect `git status`, recent commits, relevant source, tests, and generated/runtime artifacts.
3. Preserve unrelated user changes. Never reset or overwrite them to simplify the task.

## 2. Audit TODOs from evidence

Classify every audited item as one of:

- `complete`: direct implementation evidence plus verification covering the full wording.
- `partial`: some named behavior exists, but at least one requirement or platform is missing.
- `unverified`: implementation may exist, but required runtime, visual, cross-platform, performance, or release evidence is absent.
- `not started`: no meaningful implementation evidence exists.

Only change `[ ]` to `[x]` for `complete`. Tests prove only what they exercise. A placeholder, mock, configuration field, UI-only optimistic update, or platform stub is not completion.

When an item is too broad, retain it as a parent and add independently verifiable child checkboxes. Merge duplicate items by keeping the clearest canonical wording and moving unique acceptance criteria underneath it. If completed work matches no TODO, add a concise checked item with evidence-friendly wording.

## 3. Prioritize remaining work

Prefer this order unless evidence justifies a change:

1. Data loss, privacy, corruption, lifecycle leaks, and broken core capture/paste flows.
2. Incorrect persistence or frontend/backend contract mismatches.
3. Cross-platform blockers and features exposed in UI but not actually functional.
4. Settings consistency, accessibility, and interaction reliability.
5. Performance, packaging, documentation, and optional enhancements.

Choose the smallest end-to-end behavior that makes one TODO child objectively complete.

## 4. Isolate parallel agents

- Assign agents non-overlapping subsystems or read-only audits.
- State explicit file ownership when agents may edit.
- Do not let two agents modify the same shared file such as `TODO.md`, `src-tauri/src/lib.rs`, translation files, or a settings component concurrently.
- The primary agent integrates shared files and runs final verification.

## 5. Apply the settings style gate

Before changing settings styles:

1. Inspect the main page to understand the approved visual language without redesigning it.
2. Compare all settings panels and inventory typography, spacing, cards, controls, and feedback states.
3. Consolidate repeated values into existing semantic CSS variables or shared patterns.
4. Make one narrow consistency pass at a time and visually or structurally verify it.

Avoid broad main-page restyling, new arbitrary color shades, isolated font sizes, and component-specific control variants when an established settings pattern exists.

## 6. Commit minimal verified units

1. Keep each commit to one independently explainable feature, fix, style pass, audit update, or skill update.
2. Stage selected files or hunks when unrelated work shares a file.
3. Run the narrowest relevant checks before committing; run the full verification suite at integration milestones.
4. Update matching TODO checkboxes in the same feature commit when practical. Use a separate audit commit when many historical items are being corrected from evidence.
5. Do not postpone safe commits merely because the full roadmap remains unfinished.

## 7. Evolve the skill

When a rule is repeatedly needed, prevents a real regression, or expresses a stable project preference, add it to this skill or a directly linked reference. Keep transient task details and one-off implementation notes out of the skill.
