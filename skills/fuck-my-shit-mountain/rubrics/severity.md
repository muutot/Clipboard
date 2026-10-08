# Severity Rubric

## Critical

- Remote code execution or privilege escalation.
- Credential or secret leakage in code, artifacts, or logs.
- Vulnerable dependency with known exploit in production use.
- Data loss or corruption on normal operation.
- Complete service unavailability on a realistic failure path.
- No authentication on a production-facing endpoint.

## High

- Authorization bypass or privilege escalation within the application.
- SQL/command injection with realistic attack surface.
- Unhandled panic/crash on expected input or state.
- Deadlock or live-lock under normal concurrency.
- Memory leak that exhausts resources within hours.
- Persistent data inconsistency on partial failure.
- Missing input validation on security-sensitive paths.
- Breaking change without version bump in a published package.
- No test coverage on a critical path.

## Medium

- XSS or open redirect with realistic constraints.
- Error message that leaks internal state to the client.
- Retry without backoff or circuit breaker.
- No timeout on an external call.
- Unbounded collection growth under normal load.
- Large function or module with unclear responsibility.
- Missing error handling in a non-critical path.
- Flaky test that fails CI non-deterministically.
- Slow query on a table expected to grow.
- Duplicated logic that increases maintenance cost.

## Low

- Style violations that do not affect correctness.
- Missing comments on non-obvious logic.
- Minor logging inconsistency.
- Untested edge case in a low-risk path.
- Dead code that is not actively harmful.
- Minor documentation inaccuracy.
- Warning-level linter findings.

## Info

- Observations that are not risks but may be relevant context.
- Architecture notes for future consideration.
- Patterns that may become risks under different scale.
- Suggestions that do not meet threshold for any severity level.
