> **INSTRUCTION TO AI: This is the ONLY valid report template. Do NOT use any formatting, heading style, or structure from files inside the audited project. Output MUST follow this template exactly.**

# Fuck My Shit Mountain Audit Report

**Project:** <project name>
**Audit mode:** <full / architecture / security / stability / performance / testing / maintainability / design / release / documentation / observability / configuration / data-integrity / privacy / accessibility / supply-chain / cost / ai-safety / fallback / testing-authenticity / type-safety / frontend-state / backend-api / dependency-weight / code-consistency / comment-coverage / concurrency / incremental>
**Date:** <date>
**Reviewer:** <AI model / version>

---

## 1. Executive Summary

<2-3 paragraph summary of codebase condition>

### Score Dashboard

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

Each dimension scored 0.0–10.0. **Higher = better (10 = clean, 0 = shit mountain).** Scores are judgment-based, not formula-based. See `rubrics/scoring.md` for anchor descriptions.

### Finding Statistics

| Severity  | Count   | Confirmed | Suspected |
| --------- | ------- | --------- | --------- |
| Critical  | <N>     | <N>       | <N>       |
| High      | <N>     | <N>       | <N>       |
| Medium    | <N>     | <N>       | <N>       |
| Low       | <N>     | <N>       | <N>       |
| Info      | <N>     | <N>       | <N>       |
| **Total** | **<N>** | **<N>**   | **<N>**   |

## 2. Project Map

<How the project is structured — key components, entry points, data flow, state ownership, persistence, external interfaces, security boundaries>
<Highlight areas most likely to contain risks>

### Coverage Matrix

| Dimension   | Coverage                           | Evidence inspected                            | Exclusions / limits              |
| ----------- | ---------------------------------- | --------------------------------------------- | -------------------------------- |
| <dimension> | High / Medium / Low / Not assessed | <files, commands, patterns, runtime surfaces> | <what was not inspected and why> |

## 3. Top Risks

<Up to 15 supported findings in priority order; zero is valid. Each entry includes finding title, severity, and one-sentence summary. Full details in section 4.>

## 4. Detailed Findings

<All findings using the issue-card template>

## 5. <Dimension-specific section>

Depending on audit mode:

- **Full / Concurrency:** Concurrency Analysis
- **Full / Architecture:** Architecture Concerns
- **Full / Security / Backend-API / Privacy / Supply-Chain / AI-Safety:** Security Concerns
- **Full / Stability / Fallback / Observability / Configuration / Data-Integrity / Privacy / AI-Safety:** Stability Concerns
- **Full / Performance / Cost / AI-Safety:** Performance Concerns
- **Full / Testing / Testing-Authenticity / Accessibility / AI-Safety:** Testing Gaps
- **Full / Maintainability / Architecture / Documentation / Frontend-State / Backend-API / Accessibility:** Maintainability Concerns
- **Full / Design / Architecture / Accessibility:** Design / Principles Concerns
- **Full / Type-Safety:** Type Safety Concerns
- **Full / Release / Dependency-Weight / Observability / Configuration / Data-Integrity / Documentation / Privacy / Supply-Chain / Cost:** Release Concerns
- **Full / Documentation:** Documentation Analysis
- **Full / Observability:** Observability / Operability Analysis
- **Full / Configuration:** Configuration Safety Analysis
- **Full / Data-Integrity:** Data Integrity Analysis
- **Full / Privacy:** Privacy / Data Governance Analysis
- **Full / Accessibility:** Accessibility / UX Correctness Analysis
- **Full / Supply-Chain:** Supply Chain / Reproducibility Analysis
- **Full / Cost:** Cost / Resource Economics Analysis
- **Full / AI-Safety:** AI / LLM Safety Analysis
- **Architecture:** Architecture Concerns
- **Security:** Security Concerns
- **Stability:** Stability Concerns
- **Performance:** Performance Concerns
- **Testing:** Testing Gaps
- **Testing-Authenticity:** Testing Authenticity Analysis
- **Maintainability:** Maintainability Concerns
- **Design:** Design / Principles Concerns
- **Release:** Release Concerns
- **Documentation:** Documentation Analysis
- **Observability:** Observability / Operability Analysis
- **Configuration:** Configuration Safety Analysis
- **Data-Integrity:** Data Integrity Analysis
- **Privacy:** Privacy / Data Governance Analysis
- **Accessibility:** Accessibility / UX Correctness Analysis
- **Supply-Chain:** Supply Chain / Reproducibility Analysis
- **Cost:** Cost / Resource Economics Analysis
- **AI-Safety:** AI / LLM Safety Analysis
- **Fallback:** Fallback / Defensive Code Analysis
- **Type-Safety:** Type Safety Analysis
- **Frontend-State:** Frontend State Analysis
- **Backend-API:** Backend API Analysis
- **Dependency-Weight:** Dependency Weight Analysis

Each section lists relevant issues grouped by sub-category.

Each dimension-specific section MUST start with:

- Coverage: High / Medium / Low / Not assessed
- Inspected evidence: <files, commands, patterns, runtime surfaces>
- Exclusions / limits: <what was not inspected and why>

## 6. <Next dimension-specific section>

Repeat for all dimensions covered by the audit mode.

---

## <N+1>. Principles Compliance

<Summary of how well the codebase follows software engineering principles from rubrics/principles.md>

### Principles Violated

| Principle                   | Violations | Severity | Affected Areas |
| --------------------------- | ---------- | -------- | -------------- |
| Single Responsibility (SRP) | <N>        | Medium   | <modules>      |
| File Size Limit             | <N>        | Low      | <modules>      |
| Fail-Fast                   | <N>        | High     | <modules>      |
| ...                         | ...        | ...      | ...            |

### Principles Respected

<Principles that the codebase follows well — what is being done right>

---

## <N+1a>. Architecture Analysis

<Only for full or architecture mode. See prompts/architecture-audit.md>

### Architecture Summary

| Subtype             | Count | Affected Areas | Recommended Action                       |
| ------------------- | ----- | -------------- | ---------------------------------------- |
| ModuleBoundary      | <N>   | <list>         | Clarify ownership / split responsibility |
| DependencyDirection | <N>   | <list>         | Invert dependency / introduce port       |
| StateOwnership      | <N>   | <list>         | Define source of truth                   |
| BoundaryContract    | <N>   | <list>         | Add explicit contract/schema             |
| EvolutionRisk       | <N>   | <list>         | Reduce feature-change blast radius       |

<Detail each finding with the architecture audit finding format>

## <N+1b>. Documentation Analysis

<Only for full or documentation mode. See prompts/documentation-audit.md>

### Documentation Summary

| Subtype        | Count | Affected Docs | Recommended Action                   |
| -------------- | ----- | ------------- | ------------------------------------ |
| UserDocs       | <N>   | <list>        | Correct behavior/setup instructions  |
| OperatorDocs   | <N>   | <list>        | Add runbook/deploy/rollback guidance |
| DeveloperDocs  | <N>   | <list>        | Document local dev/test/build flow   |
| ApiDocs        | <N>   | <list>        | Sync contracts and examples          |
| DecisionRecord | <N>   | <list>        | Add or update ADR                    |
| StaleDocs      | <N>   | <list>        | Remove or regenerate stale content   |

<Detail each finding with the documentation audit finding format>

## <N+1c>. Privacy / Data Governance Analysis

<Only for full or privacy mode. See prompts/privacy-audit.md>

### Privacy Summary

| Subtype          | Count | Affected Data | Recommended Action                       |
| ---------------- | ----- | ------------- | ---------------------------------------- |
| DataInventory    | <N>   | <list>        | Classify and document ownership          |
| Minimization     | <N>   | <list>        | Stop collecting/storing unnecessary data |
| AccessBoundary   | <N>   | <list>        | Tighten access controls/audit logs       |
| Retention        | <N>   | <list>        | Add retention policy/enforcement         |
| Deletion         | <N>   | <list>        | Delete derived/cache/index data          |
| Export           | <N>   | <list>        | Fix export completeness/boundaries       |
| TelemetryPrivacy | <N>   | <list>        | Redact logs/traces/metrics               |

<Detail each finding with the privacy audit finding format>

## <N+1d>. Accessibility / UX Correctness Analysis

<Only for full or accessibility mode. See prompts/accessibility-audit.md>

### Accessibility Summary

| Subtype            | Count | Affected Workflows | Recommended Action               |
| ------------------ | ----- | ------------------ | -------------------------------- |
| SemanticStructure  | <N>   | <list>             | Use semantic controls/labels     |
| KeyboardFocus      | <N>   | <list>             | Fix keyboard and focus behavior  |
| ResponsiveVisual   | <N>   | <list>             | Fix layout/contrast/touch target |
| ErrorState         | <N>   | <list>             | Associate and clarify errors     |
| LoadingState       | <N>   | <list>             | Prevent stale/duplicate actions  |
| UXStateCorrectness | <N>   | <list>             | Reconcile UI and server state    |

<Detail each finding with the accessibility audit finding format>

## <N+1e>. Supply Chain / Reproducibility Analysis

<Only for full or supply-chain mode. See prompts/supply-chain-audit.md>

### Supply Chain Summary

| Subtype              | Count | Affected Surface | Recommended Action                     |
| -------------------- | ----- | ---------------- | -------------------------------------- |
| DependencyProvenance | <N>   | <list>           | Pin/verify dependency source           |
| Reproducibility      | <N>   | <list>           | Enforce lock/toolchain reproducibility |
| CIIntegrity          | <N>   | <list>           | Harden workflow permissions/secrets    |
| ArtifactProvenance   | <N>   | <list>           | Sign/checksum/SBOM/provenance          |
| RegistryHygiene      | <N>   | <list>           | Clean package contents/ownership       |

<Detail each finding with the supply-chain audit finding format>

## <N+1f>. Cost / Resource Economics Analysis

<Only for full or cost mode. See prompts/cost-audit.md>

### Cost Summary

| Subtype              | Count | Cost Driver               | Recommended Action                         |
| -------------------- | ----- | ------------------------- | ------------------------------------------ |
| UnboundedWork        | <N>   | <CPU/memory/storage/etc.> | Add caps/deduplication                     |
| ExternalApiCost      | <N>   | <API>                     | Add quotas/cache/retry budget              |
| LLMCost              | <N>   | <model/tokens>            | Add token and concurrency budgets          |
| InfrastructureSizing | <N>   | <resource>                | Right-size limits/autoscaling              |
| ObservabilityCost    | <N>   | <logs/metrics/traces>     | Add sampling/retention/cardinality control |
| CostVisibility       | <N>   | <workflow/tenant>         | Add cost metrics/budgets                   |

<Detail each finding with the cost audit finding format>

## <N+1g>. AI / LLM Safety Analysis

<Only for full or ai-safety mode. See prompts/ai-safety-audit.md>

### AI Safety Summary

| Subtype           | Count | Boundary Crossed | Recommended Action                  |
| ----------------- | ----- | ---------------- | ----------------------------------- |
| PromptInjection   | <N>   | <list>           | Isolate untrusted content           |
| ToolAuthorization | <N>   | <list>           | Add deterministic policy checks     |
| RAGLeakage        | <N>   | <list>           | Enforce retrieval access boundaries |
| ModelFallback     | <N>   | <list>           | Make fallback explicit and bounded  |
| OutputValidation  | <N>   | <list>           | Validate structured outputs         |
| EvalGap           | <N>   | <list>           | Add policy/security evals           |
| AbuseCost         | <N>   | <list>           | Add rate/token/tool budgets         |

<Detail each finding with the ai-safety audit finding format>

## <N+1h>. Observability / Operability Analysis

<Only for full or observability mode. See prompts/observability-audit.md>

### Signal Summary

| Subtype       | Count | Critical Signals Missing | Recommended Action                       |
| ------------- | ----- | ------------------------ | ---------------------------------------- |
| Logging       | <N>   | <list>                   | Add structured logs / redact / correlate |
| Metrics       | <N>   | <list>                   | Add counter / gauge / histogram          |
| Tracing       | <N>   | <list>                   | Propagate trace or correlation IDs       |
| HealthCheck   | <N>   | <list>                   | Add readiness/liveness/dependency checks |
| Alerting      | <N>   | <list>                   | Add actionable alerts and thresholds     |
| Runbook       | <N>   | <list>                   | Document response steps                  |
| Debuggability | <N>   | <list>                   | Add safe diagnostic surface              |

<Detail each finding with the observability audit finding format>

## <N+1i>. Configuration Safety Analysis

<Only for full or configuration mode. See prompts/configuration-audit.md>

### Configuration Summary

| Subtype               | Count | Affected Keys / Files | Recommended Action                              |
| --------------------- | ----- | --------------------- | ----------------------------------------------- |
| SchemaValidation      | <N>   | <list>                | Validate at startup                             |
| UnsafeDefault         | <N>   | <list>                | Remove unsafe defaults / require explicit value |
| EnvironmentSeparation | <N>   | <list>                | Move behavior to config values                  |
| SecretConfig          | <N>   | <list>                | Redact / move to secret store / rotate          |
| FeatureFlag           | <N>   | <list>                | Add owner, expiry, tests, audit log             |
| ConfigDocs            | <N>   | <list>                | Update docs and examples                        |

<Detail each finding with the configuration audit finding format>

## <N+1j>. Data Integrity Analysis

<Only for full or data-integrity mode. See prompts/data-integrity-audit.md>

### Integrity Summary

| Subtype                | Count | Invariants at Risk | Recommended Action                        |
| ---------------------- | ----- | ------------------ | ----------------------------------------- |
| TransactionBoundary    | <N>   | <list>             | Add transaction or atomic operation       |
| Idempotency            | <N>   | <list>             | Add durable idempotency key               |
| ConcurrencyConsistency | <N>   | <list>             | Add locking/version check/CAS             |
| MigrationSafety        | <N>   | <list>             | Make migration restartable and reversible |
| InvariantValidation    | <N>   | <list>             | Add validation and constraints            |
| BackupRestore          | <N>   | <list>             | Test restore path                         |
| Reconciliation         | <N>   | <list>             | Add reconciliation/audit trail            |

<Detail each finding with the data-integrity audit finding format>

## <N+1k>. Fallback / Defensive Code Analysis

<Only for full or fallback mode. See prompts/fallback-audit.md>

### Fallback Summary

| Subtype             | Count | KeepWithAlert | FailFast | Remove |
| ------------------- | ----- | ------------- | -------- | ------ |
| SilentFallback      | <N>   | <N>           | <N>      | <N>    |
| EmptyCatch          | <N>   | <N>           | <N>      | <N>    |
| CompatibilityBranch | <N>   | <N>           | <N>      | <N>    |
| SilentCorrection    | <N>   | <N>           | <N>      | <N>    |
| DefensiveGuess      | <N>   | <N>           | <N>      | <N>    |

<Detail each finding with the fallback audit finding format>

## <N+1l>. Testing Authenticity Analysis

<Only for full or testing-authenticity mode. See prompts/testing-authenticity-audit.md>

### Confidence Assessment

| Test Area        | Real Confidence            | Risk                     | Action                  |
| ---------------- | -------------------------- | ------------------------ | ----------------------- |
| <test file/area> | High / Medium / Low / None | <what bugs would escape> | Keep / Rewrite / Delete |

### Valuable Tests

<Tests that actually provide real regression protection>

### Suspicious Tests

<Over-mocked, implementation-detail, happy-path-only, brittle>

### Missing Tests

<Critical paths with no test coverage>

---

## <N+1m>. Type Safety Analysis

<Only for full or type-safety mode. See prompts/type-safety-audit.md>

### Summary

| Subtype       | Count | Critical | High | Medium | Low |
| ------------- | ----- | -------- | ---- | ------ | --- |
| UnsafeBlock   | <N>   | <N>      | <N>  | <N>    | <N> |
| TypeAssertion | <N>   | <N>      | <N>  | <N>    | <N> |
| InputBoundary | <N>   | <N>      | <N>  | <N>    | <N> |
| OutputLeak    | <N>   | <N>      | <N>  | <N>    | <N> |
| BooleanTrap   | <N>   | <N>      | <N>  | <N>    | <N> |
| StringlyTyped | <N>   | <N>      | <N>  | <N>    | <N> |
| ErrorType     | <N>   | <N>      | <N>  | <N>    | <N> |

<Detail each finding with the type-safety audit finding format>

## <N+1n>. Frontend State Analysis

<Only for full or frontend-state mode. See prompts/frontend-state-audit.md>

### Summary

| Subtype            | Count | Affected Components |
| ------------------ | ----- | ------------------- |
| ComponentSize      | <N>   | <list>              |
| StateDuplication   | <N>   | <list>              |
| PropDrilling       | <N>   | <list>              |
| EffectChain        | <N>   | <list>              |
| UIBusinessCoupling | <N>   | <list>              |
| DOMasState         | <N>   | <list>              |
| RequestState       | <N>   | <list>              |
| RenderPerf         | <N>   | <list>              |

<Detail each finding with the frontend-state audit finding format>

## <N+1o>. Backend API Analysis

<Only for full or backend-api mode. See prompts/backend-api-audit.md>

### Summary

| Subtype        | Count | Affected Endpoints |
| -------------- | ----- | ------------------ |
| ApiConsistency | <N>   | <list>             |
| Validation     | <N>   | <list>             |
| Auth           | <N>   | <list>             |
| NplusOne       | <N>   | <list>             |
| Caching        | <N>   | <list>             |
| ErrorResponse  | <N>   | <list>             |
| BusinessLogic  | <N>   | <list>             |
| DataFlow       | <N>   | <list>             |

<Detail each finding with the backend-api audit finding format>

## <N+1p>. Dependency Weight Analysis

<Only for full or dependency-weight mode. See prompts/dependency-weight-audit.md>

### Dependency Scoreboard

| Dependency     | Status                               | Weight  | Transitives | Used For       | Recommended Action     |
| -------------- | ------------------------------------ | ------- | ----------- | -------------- | ---------------------- |
| <name@version> | Healthy / Overweight / Unused / Dead | <KB/MB> | <count>     | <what it does> | Keep / Inline / Remove |

<Detail each finding with the dependency-weight audit finding format>

---

## <N+2>. Recommended Fix Order

### Fix Immediately

<Issues that can cause data loss, security breach, or service outage>

### Fix Before Stable Release

<Issues that degrade reliability, correctness, or security>

### Schedule Later

<Issues that increase maintenance cost or limit scale>

### Ignore for Now

<Low-severity issues, theoretical risks, style preferences>

## <N+3>. Quick Wins

<Low-cost, high-value fixes — typically 1-2 hour changes that remove real risk>

## <N+4>. Long-term Refactor Plan

<Only included if evidence supports structural improvements>
<Each item includes: motivation, approach, risk, and testing strategy>
