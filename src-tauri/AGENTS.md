# src-tauri/ — Rust backend (Tauri 2)

Read `skills/clipboard-dev/SKILL.md` first. Before editing backend subsystems, read
`skills/clipboard-dev/references/backend-architecture.md`; contract (command/event/config/schema)
changes also require `skills/clipboard-dev/references/data-contracts.md`.

Hard rules:

- **Never run the extreme release build locally.** Fat LTO / opt-level 3 / codegen-units 1 are
  enabled only by the environment variables in `.github/workflows/release.yml` on `v*` tag push.
  The local `[profile.release]` is intentionally fast and unoptimized.
- Local clippy gate must match CI: `cargo clippy --workspace --all-targets -- -D warnings`
  (plain `--lib` misses test-target lints).
- Blocking work inside synchronous Tauri commands (S3 downloads, database repair, file copy)
  must run in `spawn_blocking`.
- CLI `paste` and the Local API `POST /paste` must pass through privacy controls, sensitive
  content filters, and `max_text_capture_bytes`.
- Code behind `#[cfg(target_os = ...)]` is not compiled locally on other platforms; the matching
  CI job is the only gate. Never report platform completion from a compiled stub — see
  `docs/PITFALLS.md`.
- Tauri contract: keep the command name, Rust arguments/result, frontend `invoke`, serde casing,
  error handling, and tests aligned.
- Sync file-size limits are clamped to 1 KiB–2 GiB; 0 and `u64::MAX` are invalid.
- Protect local data first: be conservative around migrations, cleanup, custom resource roots,
  recycle-bin records, OCR, and derived search data.
