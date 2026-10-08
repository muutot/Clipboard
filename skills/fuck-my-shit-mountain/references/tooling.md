# Optional Audit Tooling

Use this reference when an audit can benefit from existing local scanners, linters, type checkers, or dependency tools. These tools are optional evidence sources, not hard requirements.

## Rules

- Prefer tools already configured in the repository over globally invented commands.
- Check availability before running a tool when it is not obviously part of the project.
- Do not install dependencies or mutate lockfiles unless the user explicitly approves.
- If a tool is missing, record that it was unavailable and continue with source inspection.
- Treat tool output as evidence to verify, not as final judgment.
- Redact secrets from terminal excerpts and reports.
- Include commands run in the report coverage evidence when they materially affect conclusions.

## Cross-Ecosystem Tools

| Area                       | Tools                                                 | Useful for                                                  |
| -------------------------- | ----------------------------------------------------- | ----------------------------------------------------------- |
| Secret scanning            | `gitleaks`, `trufflehog`, `detect-secrets`            | Hardcoded credentials, private keys, token leaks            |
| Semgrep rules              | `semgrep`                                             | Security patterns, framework misuse, injection paths        |
| Dependency vulnerabilities | `osv-scanner`, `trivy fs`, `grype`, `snyk`            | Known CVEs, vulnerable transitive dependencies              |
| Containers                 | `trivy image`, `trivy fs`, `hadolint`, `dockle`       | Dockerfile/image risk, package CVEs, unsafe Docker defaults |
| Infrastructure as code     | `checkov`, `tfsec`, `terrascan`, `kics`               | Terraform/Kubernetes/cloud misconfiguration                 |
| SBOM/provenance            | `syft`, `cosign`, `slsa-verifier`                     | Artifact inventory, signatures, provenance                  |
| Architecture/dependencies  | `cloc`, `scc`, `tokei`, `dependency-cruiser`, `madge` | Size, dependency direction, cycles                          |
| Documentation links        | `markdownlint`, `lychee`, `markdown-link-check`       | Broken links, inconsistent docs                             |

## Language and Framework Tools

| Ecosystem               | Prefer repository commands first                                                                                    | Tooling hints                                                                                                      |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| JavaScript / TypeScript | `npm test`, `npm run test`, `npm run lint`, `npm run typecheck`, `npm run build`, `pnpm ...`, `yarn ...`, `bun ...` | `tsc --noEmit`, `eslint`, `vitest`, `jest`, `playwright`, `npm audit`, `pnpm audit`, `dependency-cruiser`, `madge` |
| Python                  | `pytest`, `tox`, `nox`, `ruff check`, `mypy`, `pyright`                                                             | `pip-audit`, `safety`, `bandit`, `coverage`, `pytest --collect-only`                                               |
| Rust                    | `cargo test`, `cargo clippy`, `cargo fmt --check`, `cargo audit`, `cargo deny`                                      | Look for `unsafe`, missing safety comments, feature flags, panic paths                                             |
| Go                      | `go test ./...`, `go vet ./...`, `staticcheck ./...`, `govulncheck ./...`                                           | Check `context` use, goroutine leaks, error wrapping, race-sensitive paths                                         |
| Java / Kotlin / Scala   | `mvn test`, `mvn verify`, `gradle test`, `gradle check`                                                             | `spotbugs`, `checkstyle`, `pmd`, dependency checkers, Spring config checks                                         |
| .NET                    | `dotnet test`, `dotnet build`, `dotnet format --verify-no-changes`                                                  | `dotnet list package --vulnerable`, nullable reference warnings                                                    |
| PHP                     | `composer test`, `phpunit`, `phpstan`, `psalm`, `composer audit`                                                    | Laravel/Symfony route, auth, validation, serialization checks                                                      |
| Ruby                    | `bundle exec rspec`, `bundle exec rubocop`, `bundle audit`                                                          | Rails route/auth/strong parameter checks                                                                           |
| Elixir / Erlang         | `mix test`, `mix credo`, `mix dialyzer`, `mix hex.audit`                                                            | Supervision trees, process crashes, Ecto migrations                                                                |
| Swift / Objective-C     | `swift test`, `xcodebuild test`                                                                                     | Signing, entitlements, async/lifecycle correctness                                                                 |
| Dart / Flutter          | `flutter test`, `flutter analyze`, `dart analyze`                                                                   | Platform permissions, state management, accessibility                                                              |
| C / C++                 | project build/test command, `cmake --build`, `ctest`                                                                | `clang-tidy`, `cppcheck`, sanitizers, lifetime/threading risks                                                     |

## Frontend and UX Verification

When a browser UI exists and the audit scope includes frontend, accessibility, performance, or release readiness:

- Prefer configured tests first: Playwright, Cypress, Vitest, Jest, Storybook tests.
- For accessibility, use existing `axe`/`jest-axe`/Playwright accessibility checks when present.
- For manual evidence, inspect keyboard navigation, focus order, loading/error/empty states, responsive breakpoints, and contrast-sensitive UI.
- Do not claim browser behavior was verified unless a real browser, screenshot, test, or DOM evidence was used.

## AI / LLM Applications

When AI or LLM surfaces are detected:

- Look for prompts, tool-call permission boundaries, RAG retrieval filters, vector-store access, model fallback, evals, and token/cost limits.
- Useful configured tools may include project-specific eval runners, promptfoo, deepeval, ragas, LangSmith evals, or custom benchmark scripts.
- Treat missing evals or missing budget controls as coverage evidence, not automatic high severity unless tied to realistic failure.

## Reporting Guidance

In the report, mention:

- Commands run and whether they passed, failed, or were unavailable.
- Important scanner findings that were verified against source evidence.
- Any major audit dimension where tooling was unavailable and source inspection was partial.
- Why a tool was skipped when running it would be expensive, destructive, interactive, network-heavy, or outside user permission.
