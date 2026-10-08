---
name: fuck-my-shit-mountain
description: Perform evidence-based codebase health audits or focused and incremental reviews, with prioritized findings, coverage limits, and Markdown, HTML, or JSON reports. Use for requested repository audits, not routine feature implementation or editing this skill itself.
---

# Fuck My Shit Mountain

Audit real failure risks with source evidence and proportionate verification. Keep reports professional, actionable, and in the user's language.

## Resolve intent without repeated setup

- Honor modes, scope, language, output path, and format already supplied in the conversation.
- Infer the report language from the user. Default to `md` for a report request and `stdout` for a conversational review. State these assumptions briefly and proceed.
- Use `full` for a general codebase audit; infer focused modes from a specific concern. A request to review a PR or current changes selects `incremental`.
- Ask only when ambiguity materially changes what must be inspected, such as an unknown PR base with several plausible targets. Continue independent inventory while clarifying. Do not block on language or format defaults.
- Scope defaults to the current repository. Respect explicit path patterns or subsystem names. For incremental comparisons, follow [prompts/incremental-audit.md](prompts/incremental-audit.md); distinguish a merge-base comparison, two endpoint snapshots, and uncommitted changes.
- Copying, installing, or improving this skill is a skill-maintenance task; it does not start a repository audit.

## Workflow

1. Inspect repository instructions, Git status and relevant manifests. For this Clipboard repository, read [references/clipboard.md](references/clipboard.md). Preserve existing user changes.
2. Optionally run `python <skill-dir>/scripts/project_inventory.py <project-root> --format json`. Treat its results as navigation hints, not proof of implementation or complete coverage. Use project-aware `rg --files` to refine scope.
3. Load the selected prompt(s) from the table below. For `full`, use the full prompt as a checklist and load focused detail as each area is examined. Incremental alone applies all dimensions to the diff; incremental plus focused modes narrows the dimensions.
4. Load [report-format.md](references/report-format.md), the requested report template, and the severity, confidence, evidence, coverage, and scoring rubrics under `rubrics/`. Load `rubrics/principles.md` for architecture/design/maintainability or principle-related findings; [tooling.md](references/tooling.md) only when additional tooling will improve evidence.
5. Map entry points, contracts, ownership, persistence, privacy boundaries, platform code, tests and release configuration. Trace a realistic trigger through callers, validation, effects, error paths and cleanup before confirming a finding.
6. Record findings with [issue-card.md](templates/issue-card.md). Deduplicate one root cause appearing in multiple dimensions; retain cross-references. Separate confirmed defects from suspected risks and verification gaps.
7. Assemble the requested report and run `python <skill-dir>/scripts/report_lint.py --modes "<comma-separated-modes>" <report-file>`. Fix failures; disclose what the linter cannot verify. For `stdout`, apply the equivalent checks manually.
8. Deliver the report location, top actionable risks and material verification limits. If remediation was also requested, continue with the smallest verified fixes; use [remediation-plan.md](templates/remediation-plan.md) when a plan is useful.

## Output and side effects

- `md`, `html`, `json`, `both` (Markdown + HTML), and `stdout` are supported.
- Store all generated audit files under `<skill-dir>/result/`, resolved from this `SKILL.md` location rather than the current working directory. In Clipboard this is `skills/fuck-my-shit-mountain/result/`. Create it when needed; do not write audit files to the project root.
- Use `audit-report-<project>-<YYYY-MM-DD>.<ext>` inside `result/`. Keep related Markdown/HTML/JSON reports, metadata, remediation plans, evidence exports, screenshots and logs together there, optionally in a run-specific subdirectory. A supplied filename also resolves inside `result/`; use another directory only when the user explicitly requests that destination. Preserve existing reports by adding a descriptive suffix on collisions.
- JSON follows [templates/audit-report.json](templates/audit-report.json), a schema, not a report instance. HTML reuses the template's CSS and reusable section structure, with content escaped and selected dimensions added/removed as needed.
- Zero findings is valid. Leave risk lists empty and explain coverage; never invent findings to populate a template.
- `stdout` writes no files. Save history metadata only when the user requests historical tracking, beside the report inside `result/`. Include commit, dirty-tree state, actual comparison refs, modes, scope, scores and report paths. Do not create tool-specific configuration directories by default.
- Treat `result/` as local generated output: preserve its Git ignore policy and exclude its contents from skill distribution packages. Prior reports are historical context, not current implementation evidence.
- An audit alone does not authorize application fixes, dependency installs, TODO edits, commits, version bumps, releases, remote comments, or scheduled monitors. Continue already-authorized remediation without asking again.

## Evidence and coverage

- Include in-scope first-party source, tests, migrations, scripts, CI/config and behavior documentation. Exclude dependency/build/cache folders, nested worktrees, generated assets and binaries by default; inspect lockfiles when dependency/release evidence requires them.
- Inventory is not inspection. Report what was actually read or tested, the precise revision/diff, commands and results, and uninspected paths/platforms. Maintain a coverage matrix with High / Medium / Low / Not assessed for each selected dimension.
- Full mode covers every focused dimension in the table. Mark absent surfaces Not assessed with a reason. OCR, `storage` paths and audit prompt files alone do not establish an LLM runtime surface.
- Every finding needs severity, confidence, status, file and function/line evidence, realistic failure scenario, user impact, minimal fix, regression verification suggestion and estimated effort. A documentation-only issue may use a link/source check instead of a new unit test.
- Missing tests, large files, scanner hits or stylistic differences are investigation signals, not automatic defects. Severity follows demonstrated impact and likelihood; uncertainty belongs in confidence.
- For secrets, report only location/key/type and redacted evidence. Do not echo clipboard contents, credentials or private data in commands, logs or reports. Lint is heuristic and does not prove absence of secrets.

## Scoring

Use seven score categories from [rubrics/scoring.md](rubrics/scoring.md), distinct from the focused audit dimensions. Higher is better (0–10). Explain each score with evidence and coverage limits; exclude Not assessed categories from the overall mean. Use equal weights by default; use explicit, documented weights only when requested. Scores never substitute for findings or release verification.

## Modes

| Mode                   | Prompt                                  | Focus                                                        |
| ---------------------- | --------------------------------------- | ------------------------------------------------------------ |
| `full`                 | `prompts/full-audit.md`                 | All dimensions + principles                                  |
| `incremental`          | `prompts/incremental-audit.md`          | Diff-based audit of changed files since git reference        |
| `architecture`         | `prompts/architecture-audit.md`         | Module boundaries, dependency direction, state ownership     |
| `security`             | `prompts/security-audit.md`             | Security risks                                               |
| `stability`            | `prompts/stability-audit.md`            | Reliability & errors                                         |
| `performance`          | `prompts/performance-audit.md`          | Realistic bottlenecks                                        |
| `testing`              | `prompts/testing-audit.md`              | Test quality & gaps                                          |
| `maintainability`      | `prompts/maintainability-audit.md`      | Complexity, coupling, principles                             |
| `design`               | `prompts/design-audit.md`               | Engineering principles and design risk                       |
| `release`              | `prompts/release-audit.md`              | Release readiness                                            |
| `documentation`        | `prompts/documentation-audit.md`        | Docs accuracy, setup, operator/developer guidance            |
| `observability`        | `prompts/observability-audit.md`        | Logging, metrics, tracing, health checks, alerting           |
| `configuration`        | `prompts/configuration-audit.md`        | Config validation, defaults, feature flags, env separation   |
| `data-integrity`       | `prompts/data-integrity-audit.md`       | Transactions, idempotency, migrations, invariants            |
| `privacy`              | `prompts/privacy-audit.md`              | PII, minimization, retention, deletion, data governance      |
| `accessibility`        | `prompts/accessibility-audit.md`        | Keyboard, focus, semantics, responsive and UX states         |
| `supply-chain`         | `prompts/supply-chain-audit.md`         | Provenance, reproducibility, CI integrity, signing           |
| `cost`                 | `prompts/cost-audit.md`                 | Resource economics, budgets, external API and LLM costs      |
| `ai-safety`            | `prompts/ai-safety-audit.md`            | Prompt injection, tool auth, RAG leakage, evals, cost abuse  |
| `fallback`             | `prompts/fallback-audit.md`             | Silent fallback, catch, defensive guessing                   |
| `testing-authenticity` | `prompts/testing-authenticity-audit.md` | Real confidence vs green checkmarks                          |
| `type-safety`          | `prompts/type-safety-audit.md`          | Unsafe blocks, assertions, boundary types                    |
| `frontend-state`       | `prompts/frontend-state-audit.md`       | Component size, state, effects, coupling                     |
| `backend-api`          | `prompts/backend-api-audit.md`          | API design, validation, data access patterns                 |
| `dependency-weight`    | `prompts/dependency-weight-audit.md`    | Overweight deps, build toolchain                             |
| `code-consistency`     | `prompts/code-consistency-audit.md`     | Naming, imports, patterns, style uniformity                  |
| `comment-coverage`     | `prompts/comment-coverage-audit.md`     | Doc quality, stale comments, missing docs                    |
| `concurrency`          | `prompts/concurrency-audit.md`          | Race conditions, deadlocks, atomicity, shared state, locking |

## Maintaining this copy

The repository owns this copy; updating it must not overwrite a global installation. Keep prompts, schemas, templates and lint mode coverage aligned. Run:

```powershell
python -X utf8 -B -m unittest discover -s skills/fuck-my-shit-mountain/tests -v
python -X utf8 -B skills/fuck-my-shit-mountain/scripts/project_inventory.py . --format text --language zh
python -X utf8 -B skills/fuck-my-shit-mountain/scripts/package_skill.py --dry-run
```

Use the skill-creator validator when available. The Python helpers use only the standard library. See [references/report-format.md](references/report-format.md) for lint coverage and limitations.
