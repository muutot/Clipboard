# Stability Audit Prompt

Use the fuck-my-shit-mountain skill in **stability mode**.

Shared setup, coverage, report template, HTML, and lint rules live in `references/report-format.md`; load that reference before producing the report.

Focus on reliability, runtime safety, state consistency, and failure recovery.

## Audit Areas (see principles 4.4, 6.1–6.4, 10.1–10.4)

### Crash & Panic Paths

- Direct calls to `panic!`, `unwrap()`, `expect()`, `assert!`
- Indexing without bounds checking
- Division by zero
- Null pointer / nil dereference risks
- Downcasting without type check
- Integer overflow / underflow

### Error Handling

- Errors that are silently swallowed
- Errors that are logged but not handled
- Catch-all error handlers that mask failures
- Error types that lose context (e.g., `Box<dyn Error>`, `String` errors)
- Missing error propagation in async contexts
- Error recovery that leaves state inconsistent

### Concurrency & State

- Race conditions in shared state access
- Lock ordering and deadlock risks
- Missing synchronization on shared mutable state
- Channel / queue overflow
- Task / goroutine / thread leaks
- Async task cancellation safety

### External Dependencies

- Network calls without timeout
- Retry without backoff or jitter
- Circuit breaker or bulkhead missing
- Database connection pool exhaustion
- File handle leaks
- Resource cleanup on error paths

### Lifecycle

- Graceful shutdown — is cleanup guaranteed?
- Signal handling (SIGTERM, SIGINT)
- State persistence and recovery
- Snapshot / checkpoint corruption handling
- Startup dependency ordering

### Resource Management

- Unbounded memory growth (collections, caches, buffers)
- Goroutine / task / thread leaks
- Connection pool sizing
- Backpressure implementation
- Streaming backpressure

## Rules

1. Focus on realistic failure scenarios, not theoretical ones.
2. For each issue, describe the trigger, the failure scenario, and the user-visible impact.
3. Prefer the minimal fix that removes the crash or inconsistency risk.

## Attitude

1. **Be exhaustively systematic.** Check in-scope error paths, panic/unwrap paths, timeout behavior, lifecycle edges, and recovery paths. Follow the skill's coverage strategy and document exclusions honestly.
2. **Do not be a yes-man.** Do not skip issues because "it works in practice." Report every realistic crash path.

## Finding Format

### Finding: <short title>

- Severity: Critical / High / Medium / Low / Info
- Confidence: High / Medium / Low
- Category: Stability
- Status: Confirmed / Suspected
- Affected area:
- Evidence:
  - File:
  - Function / Module:
  - Relevant behavior:
- Failure trigger:
- Failure scenario:
- User-visible impact:
- Minimal fix:
- Better long-term fix:
- Regression test suggestion:
- Estimated effort:
