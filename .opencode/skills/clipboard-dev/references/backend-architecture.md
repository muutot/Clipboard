# Backend Architecture — Detailed Reference

## Database Pool (`storage/pool.rs`)

`DatabasePool` provides connection pooling with separate read/write paths:
- **Write path**: Single `Arc<Database>` connection for all writes
- **Read path**: `Vec<Acc<Database>>` with round-robin selection via `Mutex<usize>`
- **RAII**: `PooledConnection` wrapper ensures automatic connection recycling

## Database Recovery (`storage/recovery.rs`)

Automatic corruption detection and recovery:
- `recover_database_if_needed()` — validates DB with `PRAGMA quick_check`
- Tries `.backup` then `.backup.prev` if current DB is corrupt
- Quarantines corrupt files with `.quarantine.{timestamp}` suffix
- `promote_backup_to_primary()` — rotates backups
- `validate_database_file()` — validates via `PRAGMA quick_check`

## Search Index (`search/index.rs`)

### Search Cache Pattern
- `cached_ids: Mutex<Option<(String, usize, Vec<String>)>>` — caches (query, max_results, item_ids)
- Cache key: `(query, max_results)` — exact match required
- **Invalidation**: query changes, max_results increase, `apply_changes()`, `begin_full_rebuild()`
- See [search-cache-strategy.md](search-cache-strategy.md) for full details

### Search Flow
1. `search_all_ids(query, max_results)` — Tantivy search, cache IDs
2. `search_page(offset, limit)` — slice cached IDs, fetch from SQLite
3. Frontend pagination: first call offset=0, subsequent calls increment by pageSize

## Search Outbox (`search/sync.rs`)

Changes logged to `search_outbox` table, synchronizer processes in batches:
- Decouples clipboard writes from index updates
- `SearchSynchronizer` polls outbox, applies changes to Tantivy index

## Content Module (`content/`)

| File | Description |
|---|---|
| `detector.rs` | Content type detection (text/link/image/file), date detection, phone/email/URL detection |
| `thumbnail.rs` | Image thumbnail generation (JPEG, max 400px) |
| `hash.rs` | Content hashing with dedup + icon dedup |
| `file_store.rs` | File copy/storage with hash verification |
| `resource_metadata.rs` | MIME types, schema versioning, file metadata |
| `transform.rs` | Text transforms (case, strip, JSON format, base64) |
| `actions.rs` | Quick actions (email, URL, phone detection) |

## Platform Adapters (`platform/`)

| File | Lines | Description |
|---|---|---|
| `windows_clipboard.rs` | — | Windows clipboard monitoring |
| `windows_hotkey.rs` | — | Windows global hotkey registration |
| `macos.rs` | 877 | macOS clipboard + hotkey |
| `linux_x11.rs` | 1055 | Linux X11 clipboard + hotkey |
| `linux_wayland.rs` | 884 | Linux Wayland clipboard + hotkey |

## CLI API Server (`cli/api.rs`)

Loopback-only HTTP API server for scripts/automation:
- 644 lines, serves export endpoints
- Bind to `127.0.0.1` only (security)
