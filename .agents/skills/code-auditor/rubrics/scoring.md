# Scoring Rubric

## Principle

Scores are **judgment-based, not formula-based.** The AI evaluates each dimension holistically based on the evidence collected, then assigns a score. There is no mechanical "Critical = -2.0" deduction — that produces inflated scores and false precision.

## Scale

Each dimension is scored **0.0 – 10.0** — **higher = better**.

| Score                                                                                      | Meaning                                                                  |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| **10.0**                                                                                   | **Perfect.** Clean, production-ready. No issues found.                   |
| **0.0**                                                                                    | **Maximum shit mountain.** Completely unmaintainable. Unacceptable risk. |
| The score reflects **engineering quality and maintainability**, not code style preference. |

**CRITICAL — Do not reverse this. 10 is the best score. 0 is the worst. Higher = better, always.**

## Dimensions

| Dimension       | What It Measures                                                                                                                                | Mapped From Modes                                                                                                            |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| Security        | How resistant to attack. Auth, injection, secrets, dependency risk, privacy boundaries, supply-chain risk, AI/tool abuse.                       | security, type-safety, configuration, privacy, supply-chain, ai-safety                                                       |
| Stability       | How reliable under failure. Panic paths, error handling, retry, timeout, state consistency, fallback quality, data correctness, operability.    | stability, fallback, observability, configuration, data-integrity, privacy, ai-safety, concurrency                           |
| Performance     | How efficient under realistic load and cost. Hot paths, memory, I/O, contention, external API/model spend.                                      | performance, dependency-weight, cost, ai-safety                                                                              |
| Testing         | How much real confidence tests provide. Coverage quality, test types, authenticity, accessibility/AI safety evals.                              | testing, testing-authenticity, accessibility, ai-safety                                                                      |
| Maintainability | How easy to change. Architecture, complexity, coupling, duplication, naming, docs, state management, API design.                                | architecture, maintainability, documentation, frontend-state, backend-api, code-consistency, comment-coverage, accessibility |
| Design          | How well it follows engineering principles. SRP, DRY, KISS, fail-fast, type safety, boundary design.                                            | architecture, design, type-safety, accessibility                                                                             |
| Release         | How ready to ship. CI/CD, versioning, upgrade, rollback, dependency weight, configuration, observability, migration safety, supply chain, cost. | release, dependency-weight, observability, configuration, data-integrity, documentation, supply-chain, cost, privacy         |

## Score Anchors

Use these descriptions as **guidance**, not rules. The final score is your judgment.

### 9.0 – 10.0 (Excellent / Clean)

- The dimension is in good shape or has only minor issues.
- Issues found are isolated, low-severity, and easy to fix.
- No structural debt. No systemic risk.
- **One-sentence pattern:** "Solid. A few minor issues but nothing that blocks release."

### 7.0 – 8.9 (Good / Needs attention)

- Some real issues exist, but they are contained.
- Moderate risk in specific areas. No systemic failure.
- Fixing requires local changes, not rewrites.
- **One-sentence pattern:** "Some real issues, but contained. Worth fixing before next release."

### 5.0 – 6.9 (Fair / Significant risk)

- Systemic issues in this dimension.
- Multiple medium-or-higher severity findings.
- The dimension needs deliberate investment, not just quick fixes.
- **One-sentence pattern:** "Systemic problems. Needs deliberate investment, not quick patches."

### 3.0 – 4.9 (Poor / Critical debt)

- Serious failures in this dimension.
- High-severity issues that are not isolated — they indicate a pattern.
- Fixing requires structural changes.
- **One-sentence pattern:** "Structural failures. Fixing requires meaningful rework, not spot fixes."

### 0.0 – 2.9 (Shit Mountain / Unacceptable)

- This dimension is a disaster.
- Critical-severity issues that are pervasive.
- The approach in this dimension is fundamentally wrong.
- **One-sentence pattern:** "Fundamentally broken. Needs to be redone."

## Overall Score

Use the equal-weight mean of assessed score categories, rounded to 1 decimal place. Exclude Not assessed categories; if none are assessed, use null / Not assessed. Only use alternative weights when requested, document them and normalize them over assessed categories.

For focused audit modes (e.g., security-only), only report the relevant dimension score and note that other dimensions were not assessed.

## Grade Map

**Higher = better.** S is best, F is worst.

| Score      | Grade | Label         | Meaning                                                     |
| ---------- | ----- | ------------- | ----------------------------------------------------------- |
| 9.0 – 10.0 | S     | Excellent     | Production-ready. Minor nitpicks only.                      |
| 7.0 – 8.9  | A     | Good          | Solid. Some issues but low urgency.                         |
| 5.0 – 6.9  | B     | Fair          | Needs work. Medium risks present.                           |
| 3.0 – 4.9  | C     | Poor          | Significant risks. Should address before release.           |
| 1.0 – 2.9  | D     | Bad           | Critical issues. Do not ship as-is.                         |
| 0.0 – 0.9  | F     | Shit Mountain | High-severity issues across the board. Major rework needed. |

## Score Visualization

Render each dimension score as a 10-character ASCII bar. **Higher = more filled = better.**

```
Score bar: ████████░░
8.0 = ████████░░  (8 filled, 2 empty)
3.5 = ███░░░░░░░  (3.5 filled, 6.5 empty)
```

Filled blocks: `█` (use `\u2588`)
Empty blocks: `░` (use `\u2591`)

Number of filled blocks = floor(score). Remainder rounds up at ≥ 0.5.

### Example Dashboard

```
Security        ████████░░  8.0  A   No auth on WS, hardcoded secret in config
Stability       ██████░░░░  6.0  B   3 unwrap on hot path, no retry on DB
Performance     ██████████  10.0 S   No issues found
Testing         ████░░░░░░  4.0  C   9 integration tests real, but unit is weak
Maintainability ███████░░░  7.0  A   3 files over 800 lines, SRP violated in 2 modules
Design          █████░░░░░  5.0  B   DRY violated 5x, fail-fast missing at API boundary
Release         ██████░░░░  6.0  B   No CI on Windows, no rollback plan
─────────────────────────────────────
Overall         ██████░░░░  6.6  B
```

## Rules

1. **Each score must have a one-sentence justification** in the score dashboard. The justification summarizes the strongest evidence and mentions coverage confidence when it limits the conclusion.
2. **Do not average finding severities.** If there is 1 Critical issue and nothing else, judge based on whether that one issue is systemic or isolated.
3. **Consider intensity and density.** 10 low-severity issues in one file may deserve a lower score than 1 critical issue with a trivial fix.
4. **Consider context.** A 600-line file in a CLI tool is different from a 600-line file in a security-critical library. Adjust for project type and scale.
5. **If a dimension has zero findings, score 10.0 (Excellent) only when coverage is High.** With Medium or Low coverage, explain the limitation and avoid presenting the dimension as fully clean.
6. **Do not round to game the grade.** If the score is 5.9, show 5.9, not 6.0. If it's 6.0, show 6.0.
7. **Include the score dashboard in the Executive Summary** with one-sentence justifications per dimension.
8. **Focused audit modes** (security-only, etc.): only score the relevant dimension. State "not assessed" for others.
9. **IMPORTANT — Direction:** 10.0 = best (clean). 0.0 = worst (shit mountain). Do not reverse this.
10. **Coverage interaction:** Use `rubrics/coverage.md`. Not assessed dimensions are excluded from the overall score, and Low coverage should not support a high-confidence clean conclusion.
