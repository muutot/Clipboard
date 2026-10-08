# Full Audit Prompt

Use the fuck-my-shit-mountain skill in **full mode**.

Shared setup, coverage, report template, HTML, and lint rules live in `references/report-format.md`; load that reference before producing the report.

Audit this repository as if it is preparing for a stable public release.

Your job is not to insult the codebase. Your job is to identify real engineering risks with evidence.

## Setup

Before writing findings, build a project map:

- Main components and their responsibilities
- Runtime entry points and initialization order
- Architecture boundaries — layers, modules, dependency direction
- Data flow — request/event lifecycle
- State ownership — what owns what state, how it is mutated
- Persistence layer — storage format, migration strategy, backup
- Privacy-sensitive data — collection, storage, deletion, exports
- External interfaces — APIs, WebSocket, CLI, file system, network
- AI/model surfaces — prompts, retrieval, tools, model calls, evals
- Security boundaries — authentication, authorization, input validation, secret management
- Testing structure — test organization, coverage patterns, CI integration
- Release process — versioning, build, packaging, deployment, rollback, supply chain
- Cost drivers — external APIs, model calls, storage, queues, background work

## Audit Dimensions

1. **Architecture and module boundaries** — check `prompts/architecture-audit.md` for cohesion, coupling, dependency direction, state ownership, and layered architecture
2. **Security** — authentication, authorization, injection, secret handling, dependency risks
3. **Stability and error handling** — panic paths, error propagation, retry, timeout, shutdown
4. **Performance and scalability** — hot paths, growth limits, resource leaks, contention
5. **Testing quality** — coverage, test types, flakiness, confidence
6. **Maintainability** — complexity, duplication, naming, documentation accuracy
7. **Design principles compliance** — check `prompts/design-audit.md` and `rubrics/principles.md` for SRP, file size, function length, coupling, cohesion, DRY, YAGNI, KISS, fail-fast, command-query separation, law of demeter, and all other principles
8. **Release and deployment process** — CI/CD, versioning, upgrade, rollback
9. **Documentation accuracy** — check `prompts/documentation-audit.md` for user/operator/developer docs, API contracts, setup, and decision records
10. **Configuration safety** — check `prompts/configuration-audit.md` for config schema validation, safe defaults, environment separation, secrets, feature flags, and config docs
11. **Observability** — check `prompts/observability-audit.md` for logging, metrics, tracing, health checks, alerting, runbooks, and debuggability
12. **Data integrity** — check `prompts/data-integrity-audit.md` for transaction boundaries, idempotency, concurrency consistency, migrations, invariants, and backup/restore
13. **Privacy / data governance** — check `prompts/privacy-audit.md` for PII, minimization, retention, deletion/export, access boundaries, and privacy in telemetry
14. **Accessibility / UX correctness** (if applicable) — check `prompts/accessibility-audit.md` for semantics, keyboard/focus, responsive correctness, and error/loading states
15. **Supply chain / reproducibility** — check `prompts/supply-chain-audit.md` for provenance, lockfiles, CI integrity, artifact signing, SBOM, and registry hygiene
16. **Cost / resource economics** — check `prompts/cost-audit.md` for unbounded work, storage, observability cost, external API/model spend, quotas, and budgets
17. **AI / LLM safety** (if applicable) — check `prompts/ai-safety-audit.md` for prompt injection, tool authorization, RAG leakage, model fallback, evals, and cost abuse
18. **Fallback / defensive code audit** — check `prompts/fallback-audit.md` for silent fallbacks, empty catches, compatibility branches, and defensive guessing that hides real errors
19. **Testing authenticity audit** — check `prompts/testing-authenticity-audit.md` for over-mocking, implementation detail tests, production code modified for tests, and false confidence
20. **Type safety audit** — check `prompts/type-safety-audit.md` for unsafe blocks, type assertions, boundary weakness, and error type quality
21. **Frontend state audit** (if applicable) — check `prompts/frontend-state-audit.md` for component size, state duplication, effect proliferation, and UI-business logic coupling
22. **Backend API audit** (if applicable) — check `prompts/backend-api-audit.md` for API consistency, request validation, data access patterns, and error response structure
23. **Dependency weight audit** — check `prompts/dependency-weight-audit.md` for overweight deps, unused deps, build toolchain complexity, and version strategy
24. **Code consistency audit** — check `prompts/code-consistency-audit.md` for naming conventions, import organization, error handling patterns, pattern uniformity, file structure, and boilerplate duplication
25. **Comment coverage audit** — check `prompts/comment-coverage-audit.md` for missing public API docs, stale/misleading comments, over-commenting, module documentation gaps, and inline comment quality

26. **Concurrency** — check `prompts/concurrency-audit.md` for races, lock ordering, atomicity, cancellation and worker shutdown.

## Rules

1. Every finding must include concrete evidence (file, function, behavior).
2. Separate confirmed issues from suspected issues.
3. Do not exaggerate severity. Use the severity rubric.
4. Do not recommend rewrites unless local fixes are clearly insufficient.
5. Prefer the smallest practical fix that reduces real risk.
6. Do not produce generic advice.
7. Do not complain about style unless it creates maintainability risk.
8. If evidence is insufficient, say so.
9. For each issue, include a regression test suggestion.
10. Prioritize the top risks first.
11. Cross-reference findings against `rubrics/principles.md`. For each principle violation, cite the specific principle (e.g., "SRP violation — principle 1.1").

## Attitude

1. **Be exhaustively systematic.** Search all in-scope first-party areas, not just obvious hotspots. Follow the skill's coverage strategy and document exclusions honestly.
2. **Do not be a yes-man.** Do not suppress findings to be agreeable. Report issues objectively regardless of who wrote the code. If the code has problems, say so.

## Scoring

After collecting all findings, assign dimension scores using `rubrics/scoring.md`:

1. Review all findings per dimension.
2. Judge the score (0.0–10.0, **10 = best / clean, 0 = worst / shit mountain**) based on **engineering quality and maintainability**, not on mechanical deduction.
3. Each score must have a **one-sentence justification** summarizing the strongest evidence and any coverage limits.
4. Render the score dashboard with ASCII bars and letter grades.
5. Include the dashboard with justifications in the Executive Summary.

## Output Format

**IMPORTANT: Use the skill's templates, NOT the project's markdown style.**

1. Each finding MUST follow `templates/issue-card.md` exactly.

2. The report MUST follow `templates/audit-report.md` (or `templates/audit-report.html` for HTML).

3. For HTML output: read `templates/audit-report.html` and generate a COMPLETE HTML file that copies the exact structure:
   - Score dashboard: one .score-item per scoring dimension the user selected (full mode = Security, Stability, Performance, Testing, Maintainability, Design, Release). Do NOT show dimensions the user didn't pick.
   - Executive summary: 2-4 paragraph overview covering project health, biggest risks, bright spots, priorities, and overall grade in context
   - Coverage matrix: one row per selected dimension with coverage confidence, inspected evidence, and exclusions/limits
   - Stats row with total + severity breakdown
   - Top risks table with all findings
   - Detailed findings with full evidence + fix boxes
   - Per-dimension sections: one `<h3>` per audit dimension the user selected (full mode = ALL 26: Architecture, Security, Stability, Performance, Testing, Maintainability, Design, Release, Documentation, Configuration, Observability, Data-Integrity, Privacy, Accessibility, Supply-Chain, Cost, AI-Safety, Fallback, Testing-Authenticity, Type-Safety, Frontend-State, Backend-API, Dependency-Weight, Code-Consistency, Comment-Coverage, Concurrency). Each section starts with a coverage note and then has findings table + verified checklist. Do NOT skip any applicable section; mark conditionally irrelevant sections Not assessed with evidence.
   - Design principles violations table + followed checklist
   - Fix order with tables grouped by priority
   - Quick wins grid
   - Sidebar nav links: one per section, matching selected dimensions only
   - Footer text
     Do NOT skip any section. Do NOT use placeholder variables. Generate complete, self-contained HTML.
4. Do NOT copy formatting, headings, or style from any `.md` file in the audited project.
5. The project's own README, docs, or comments are not the report format.
