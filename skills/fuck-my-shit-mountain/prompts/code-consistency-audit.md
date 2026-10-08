# Code Consistency Audit Prompt

Use the fuck-my-shit-mountain skill in **code-consistency mode**.

Shared setup, coverage, report template, HTML, and lint rules live in `references/report-format.md`; load that reference before producing the report.

Focus on code style consistency, naming conventions, pattern uniformity, and adherence to the project's own stated conventions.

## Audit Areas

### Naming Conventions

- Inconsistent casing (camelCase vs snake_case vs kebab-case) within the same language.
- Abbreviations used inconsistently (e.g., `idx` vs `index`, `cfg` vs `config`, `msg` vs `message`).
- Names that obscure meaning (single-letter variables beyond loop counters, overly generic names like `data`, `info`, `temp`).
- Function/method names that don't describe what they do (side effects not reflected in name).
- Boolean parameters that are meaningless at call site (e.g., `process(true, false)` without named params/struct).

### Import / Module Organization

- Import groups not following project convention (stdlib → third-party → internal).
- Wildcard imports that pollute namespace (`use module::*`, `from module import *`).
- Circular dependencies between modules.
- Files/modules that import from deep/generic paths instead of the public API.

### Error Handling Consistency

- Mix of error return patterns (e.g., exceptions in some paths, error codes in others).
- Inconsistent error type usage (string errors vs typed errors vs Result/Option).
- Some functions return `null`/`nil`/`None` for errors while others throw.
- Different parts of the codebase use different logging patterns for similar situations.

### Pattern Uniformity

- Similar operations done different ways across the codebase (e.g., HTTP clients constructed inline vs injected).
- Configuration access patterns vary (env vars read directly vs through a config object).
- Same data transformation duplicated with slightly different implementations.
- Inconsistent use of language features (e.g., async/await mixed with raw promises/callbacks).

### File / Directory Structure

- Files that don't follow the project's naming convention.
- Misplaced files (utility function in a domain-specific module, or vice versa).
- Files in the wrong directory layer (e.g., infrastructure code in domain layer).
- N+1 files doing essentially the same thing in slightly different ways.

### Boilerplate / Verbosity

- Repeated patterns that could be extracted (same validation appearing in N handlers).
- Manual serialization/deserialization where a library/framework handles it elsewhere.
- Copy-paste code that differs only in variable names.

## Rules

1. Focus on inconsistencies that create **real maintenance cost**, not aesthetic preferences.
2. A single inconsistent file is noise; a pattern of inconsistency across 5+ locations is a finding.
3. Do NOT suggest a full codebase reformat — suggest targeted extraction or lint rule additions.
4. If the project has an existing style guide or linter config, check compliance against it.
5. Consider whether a `clippy`/`eslint`/`ruff` rule could catch the inconsistency automatically.

## Finding Format

### Finding: <short title>

- Severity: Critical / High / Medium / Low / Info
- Confidence: High / Medium / Low
- Category: Maintainability
- Status: Confirmed / Suspected
- Subtype: NamingConvention / ImportOrganization / ErrorHandlingConsistency / PatternUniformity / FileStructure / Boilerplate
- Evidence:
  - File(s):
  - Pattern observed:
  - Expected convention:
- Number of occurrences:
- Why this creates maintenance cost:
- Minimal fix (extract + unify):
- Better long-term fix:
- Regression test suggestion:
- Estimated effort:
