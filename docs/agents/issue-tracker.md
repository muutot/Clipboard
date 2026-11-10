# Issue tracker: GitHub

Issues, specs, and tickets for this repo live as GitHub issues in
[`muutot/Clipboard`](.git/config). Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a heredoc
  for multi-line bodies.
- **Read an issue**: `gh issue view <number> --json number,title,body,labels,comments`.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments`.
- **Comment on an issue**: `gh issue comment <number> --body "..."`.
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` /
  `--remove-label "..."`.
- **Close**: post the explanation with `gh issue comment`, then `gh issue close <number>`.

Infer the repo from `git remote -v`; `gh` does this automatically when run inside
this clone. Before the first `gh issue create` of a session, confirm the target
with `gh repo view --json nameWithOwner` — it must be `muutot/Clipboard`, never
`mattpocock/skills` or an upstream parent.

## Pull requests as a triage surface

**PRs as a request surface: no.** `/triage` is not vendored here. If PRs ever
become a request surface, flip this flag and describe the `gh pr` equivalents.

## When a skill says "publish to the issue tracker"

Create a GitHub issue in `muutot/Clipboard`.

## When a skill says "fetch the relevant ticket"

Read it with `gh issue view` as above.

## Clipboard overrides

- Most planning work in this repo is tracked in `TODO.md`, not in GitHub issues.
  When a skill asks for a "spec" or "ticket", prefer the `TODO.md` item and the
  evidence in `.agents/skills/code-auditor/result/` unless the user names a GitHub issue.
- Commits must follow the gitmoji contract in `.agents/skills/clipboard-dev/SKILL.md`
  (English only, one verified minimal unit per commit), not the generic commit
  instructions in upstream `implement` / `implement-spec`. Those two skills are
  not vendored here.
