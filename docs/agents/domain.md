# Domain Docs

How the vendored engineering skills should consume Clipboard's domain
documentation when exploring the codebase.

## Before exploring, read these

The upstream skills look for a root `GLOSSARY.md` and `docs/adr/`. Neither exists
in this repo, and the upstream default is to proceed silently when they are
absent. Clipboard keeps equivalent, maintained knowledge elsewhere:

- **`.agents/skills/clipboard-dev/SKILL.md`** and
  **`.agents/skills/clipboard-dev/references/`**: the canonical development references
  (project structure, components, services, data contracts, backend
  architecture, CSS/theming, settings panels, search cache, hotkeys).
- **`docs/PITFALLS.md`**, **`docs/SEARCH.md`**, **`docs/OCR.md`**,
  **`docs/DEFAULTS_AND_PRIVACY.md`**: focused live documents for known
  pitfalls and subsystem behavior.
- **`TODO.md`**: the current roadmap and its acceptance criteria.
- **`AGENTS.md`**: repository-level instructions that override any skill.

Read `.agents/skills/clipboard-dev/SKILL.md` first, then follow its routing table to the
reference for the subsystem you are touching. Treat `domain-modeling`'s
`GLOSSARY-FORMAT.md` / `ADR-FORMAT.md` as optional formats: if a genuine
glossary term or a hard-to-reverse decision is resolved, create
`GLOSSARY.md` or `docs/adr/NNNN-slug.md` lazily, following those formats. Do not
create empty placeholders.

## Use the project's vocabulary

When naming a domain concept in a test, hypothesis, or refactor proposal, follow
the terms used in the current source and the routed references. The upstream
skills' vocabulary (module, interface, seam, adapter, depth, leverage, locality)
is defined in `.agents/skills/codebase-design/SKILL.md` and is additive, not a
replacement for Clipboard's own terms.

## Coding standards for `/code-review`

`code-review` asks for `CODING_STANDARDS.md` / `CONTRIBUTING.md` and carries a
Fowler smell baseline. This repo has `CONTRIBUTING.md` (project conventions) but
no `CODING_STANDARDS.md`. Its documented standards are:

- `CONTRIBUTING.md` (project conventions and development setup);
- `.agents/skills/clipboard-dev/SKILL.md` (non-negotiable principles, cross-cutting
  change gates, commit discipline);
- `.agents/skills/clipboard-dev/references/*` (subsystem contracts and style gates);
- the mechanical gates in `.githooks/` and `package.json` scripts.

The repo standards override the smell baseline. Tooling-enforced checks
(prettier, cargo fmt, clippy, svelte-check) must not be re-reported as findings.
