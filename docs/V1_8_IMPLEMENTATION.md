# v1.8 implementation checklist

Base: `v1.7.5` at `b8659bed6f3cbfaaee2a80b83788cedbdeab4bc5`.
Branch: `v1.8.0`. Each unit is verified and committed separately.
This tracks implementation, not a version bump or release.

- [x] Include frontend tests in both local verification gates (320 Vitest tests pass).
- [ ] Search all history for single characters and dates; apply combined filters before candidate limits and pagination.
- [ ] Save named searches with complete filter state and relative dates.
- [ ] Return bounded list summaries and hydrate full content for detail/copy/edit.
- [ ] Select/copy OCR blocks and highlight search matches on images.
- [ ] Extend automatic tags with source/type conditions, preview and history application.
- [ ] Export/import a self-contained resource backup with validation and restore preview.
- [ ] Extract route state controllers while preserving item-store ownership and page appearance.
- [ ] Share item-operation behavior across GUI, CLI and local API.
- [ ] Cancel synchronization cooperatively through engine and transport.
- [ ] Stream exports from a consistent database snapshot.
- [ ] Hash file bytes during staging without a second full read.
- [ ] Cache content-dependent card measurements and avoid needless whole-list work.
- [ ] Measure search input-to-paint latency and validate performance candidates with synthetic data.

## Verification

Use synthetic data and temporary directories for destructive, import and backup tests.
Run focused tests during each unit and the integration gate after cross-layer work.
Preserve single-character recall, global sorting, mutation invalidation, original clipboard
payloads, resource ownership, atomic publication, and cross-window consistency.
Record native-platform and visual checks separately from static/unit results.
