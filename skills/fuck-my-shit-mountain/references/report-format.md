# Shared Report Format Rules

Load this reference when a prompt says to use shared setup/report rules.

## Required Context Before Auditing

Resolve modes, language, format and scope using the defaults in `SKILL.md`. Ask only about ambiguity that materially changes scope; do not repeat questions answered by the user or invoking skill.

## Report Template Constraint

All generated audit files belong under `<skill-dir>/result/`, including reports in every format, metadata, remediation plans and evidence attachments. Resolve this directory from the skill location, not the shell working directory. Create it as needed and preserve existing files on name collisions. In Clipboard, use `skills/fuck-my-shit-mountain/result/`; do not place audit output in the project root. `stdout` creates no files. See `SKILL.md` for explicit destination overrides and local-output handling.

The report MUST follow the skill templates:

- Findings use `templates/issue-card.md`.
- Markdown reports use `templates/audit-report.md`.
- JSON reports follow the schema in `templates/audit-report.json`.
- HTML reports use `templates/audit-report.html`.
- Do NOT copy formatting, heading style, or structure from markdown files inside the audited project.
- The audited project's own README, docs, or comments are evidence, not the report template.
- Keep Markdown machine labels (headings, `Finding:`, field names and severity/status enums) in English, optionally followed by a localized title; write narrative content in the requested language. HTML labels can be localized while section IDs stay stable. JSON keys/enums stay stable and values use the requested language.
- A zero-finding report is valid: use empty risk/finding/fix arrays or explicit no-findings text. Do not copy example scores or invent a minimum number of issues.

## HTML Output Rules

For HTML output:

- Read `templates/audit-report.html`.
- Generate complete, self-contained HTML.
- Copy the exact CSS, section structure, classes, and ordering from the template.
- Repeat the template's dimension block for selected dimensions, including concurrency. Escape source excerpts and file names as text; do not execute audited code in a report.
- Include only score items and dimension sections relevant to the selected mode(s), except `full`, which covers all dimensions and marks inapplicable dimensions Not assessed.
- Every dimension section must include a coverage note, findings table or no-findings card, and verified checklist.
- Include sidebar nav links for every generated section.
- Do not leave placeholder variables or example data.

## Coverage Rules

Every report must include:

- A coverage matrix with one row per selected dimension.
- Per-dimension coverage: High / Medium / Low / Not assessed.
- Inspected evidence: files, commands, searches, runtime surfaces, or patterns checked.
- Exclusions / limits: what was not checked and why.

Use `rubrics/coverage.md` to assign coverage confidence.

## Lint Rules

For generated file output, run:

```bash
python3 <skill-dir>/scripts/report_lint.py --modes <selected-modes> <skill-dir>/result/<report-file>
```

Fix lint failures before delivering the report. For `stdout`, apply the same checks manually:

- No unreplaced placeholders.
- Required sections exist.
- Selected dimension sections exist.
- Markdown finding fields are complete.
- Severity statistics match detailed findings.
- No unredacted secrets or private keys appear.

## What the linter proves

The scripts use Python's standard library. Markdown lint checks English section/field labels, selected dimensions, and severity totals; HTML lint checks section IDs; both scan for common placeholders and secret patterns. JSON lint checks the bundled schema's supported constraints, selected dimension coverage/sections, finding IDs/references, and severity/status statistics. `incremental` alone selects all dimensions; combined with focused modes it selects those dimensions.

`schema_check.py` implements only the schema keywords currently used by the bundled schema, not arbitrary JSON Schema. Unknown visited constraints fail validation; extend and test the validator when changing the schema. JSON permits an empty `topRisks` list and null scores with `Not assessed` grades where evidence is absent.

Lint cannot prove source evidence, complete secret detection, rendered HTML appearance, HTML finding/statistic consistency, or scoring quality. Manually verify these before delivery. Exit codes: 0 passes implemented checks, 1 reports validation issues, 2 means missing/unreadable input or a validation execution error. Diagnostics omit matched secret values.
