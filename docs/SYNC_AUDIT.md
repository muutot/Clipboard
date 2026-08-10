# 同步子系统审计清单（21 项）

> 本文档受版本管理，是同步子系统审计的权威记录。每完成一项，更新状态与提交号；证据以代码为准，不要凭意愿勾选。

## 严重：数据正确性

| #   | 问题                                                                                                      | 状态    | 证据 / 提交                                |
| --- | --------------------------------------------------------------------------------------------------------- | ------- | ------------------------------------------ |
| 1   | 增量 oplog 不携带实际文本内容（text/html/rtf/metadata/is_favorite/source_app 丢失）                       | ✅ 完成 | `adc39a1`                                  |
| 2   | 图片/文件资源从不真正传输（serialize_oplog 只传元数据，create_oplog_backup 未接入同步流程）               | ✅ 完成 | `924085c`（内嵌 OplogResource）            |
| 3   | 前端/后端 SyncUploadResult 字段完全不匹配（uploadedEntries vs itemsSynced）                               | ✅ 完成 | `c287d07`                                  |
| 4   | S3 是伪实现（Authorization 为 `AWS key:placeholder`，无 SigV4，_region 被忽略，仅虚拟主机式 URL）         | ✅ 完成 | `201ecbd`（SigV4 + 端点解析 + path-style） |
| 5   | S3 oplog 清理曾用去前缀短名删除错误 key；更根本的问题是无设备确认时不应删除任何远端日志                   | ✅ 完成 | 见下方                                     |
| 6   | 应用远程 oplog/导入基线触发本地 trigger，造成回声广播（收到的条目又生成 unsynced changelog 再广播回远端） | ✅ 完成 | 见下方                                     |

## 高

| #   | 问题                                                                                               | 状态    | 证据 / 提交 |
| --- | -------------------------------------------------------------------------------------------------- | ------- | ----------- |
| 7   | auto_sync 是完全的死配置：有 UI 开关和 store 方法，但没有后台 worker/timer 调用 sync_upload_backup | ✅ 完成 | 见下方      |
| 8   | 每次同步全量重放 oplog；旧的全局 mtime 水位又会跨远端继承并误跳过可覆盖对象                        | ✅ 完成 | `881167c`   |
| 9   | 按文件数清理无法证明长期离线设备已接收历史，可能造成不可恢复的数据缺口                             | ✅ 完成 | `17aac63`   |
| 10  | ConfigStore 锁贯穿整个网络同步，命令是阻塞同步调用，慢网络阻塞其他配置访问                         | ✅ 完成 | 见下方      |
| 11  | 解密失败静默回退原始字节，改密码后旧文件按密文解析失败被跳过，无明确报错                           | ✅ 完成 | 见下方      |

## 低 / 优化

| #   | 问题                                                                          | 状态                     | 证据 / 提交 |
| --- | ----------------------------------------------------------------------------- | ------------------------ | ----------- |
| 12  | 每个网络调用重建 reqwest Client（webdav.rs:24）                               | ✅ 完成（OnceLock 复用） | 见下方      |
| 13  | device_id 曾有重复实现和不稳定 hostname 回退，机器改名/同名设备会破坏身份语义 | ✅ 完成                  | `5e41957`   |
| 14  | 文件名 .json 后缀实际是 bincode，仅靠 fallback 兼容，语义混乱                 | ✅ 完成                  | 见下方      |
| 15  | 同步导入/应用后不广播 clipboard-history-invalidated，主界面可能不刷新         | ✅ 完成                  | 见下方      |
| 16  | 同步无并发锁，手动+未来自动同步并发时合并逻辑存在竞争                         | ✅ 完成                  | 见下方      |

## 后续架构审计

| #   | 问题                                                                          | 状态    | 证据 / 提交 |
| --- | ----------------------------------------------------------------------------- | ------- | ----------- |
| 17  | 独立设备捕获相同文本时 ID 不同，后续编辑/删除只按 ID 匹配，无法收敛到同一实体 | ✅ 完成 | `52679ec`   |
| 18  | apply_remote_oplog 吞掉单条 SQL 错误并继续提交，损坏对象仍可能被当作已应用    | ✅ 完成 | `3a4a714`   |

## 本轮架构复核

| #   | 问题                                                                                                          | 状态    | 证据 / 提交 |
| --- | ------------------------------------------------------------------------------------------------------------- | ------- | ----------- |
| 19  | 远端资源路径、缓存和资源池对象缺少统一的路径边界与内容摘要校验，可能穿越托管根或把损坏资源写入本地            | ✅ 完成 | `929d63e`   |
| 20  | WebDAV/S3 各自维护同步、快照、下载和资源池编排，重复状态机容易产生 provider 行为漂移                          | ✅ 完成 | `b5dbde5`   |
| 21  | `sync_download_backup` 将 IPC 文件名直接拼入临时目录，绝对路径、父目录、反斜杠或 Windows ADS 名可逃逸目标目录 | ✅ 完成 | `7eeba18`   |

## #5 证据

- 历史实现中 `list_s3_objects` 返回去前缀短名，清理逻辑若未重拼 `remotePath` 会删除错误 key；即使 key 修正，仅凭数量删除也无法证明安全。
- ✅ 当前修复：`commands/sync/mod.rs` 的 WebDAV/S3 编排不再调用 `delete_from_webdav` / `delete_from_s3`，低层 transport 删除函数不参与正常同步或快照刷新。S3 上传/下载仍统一通过 `s3_object_key(remote_path, name)` 构造真实 key。

## #6 证据

- `src-tauri/src/storage/migrations.rs:163` `clipboard_items_sync_insert` 触发器：任何 `INSERT INTO clipboard_items` 都写入 `sync_changelog`（operation='insert'，device_id=本机）。
- `src-tauri/src/storage/pool.rs:271` `import_baseline_items` 与 `pool.rs:336` `apply_remote_oplog` 走裸 SQL 写 `clipboard_items`，无任何触发器抑制 → 接收端的条目会再次进入本机 `sync_changelog`，下次同步再广播回远端（回声）。
- 导入 1 万条基线就会回传 1 万条。
- ✅ 修复：三个 sync 触发器加 `WHEN NOT EXISTS (SELECT 1 FROM sync_metadata WHERE key='sync_suppress_changelog' AND value='1')` 守卫，且每次打开库 `DROP`+`CREATE`（老库下次打开即升级）；`apply_remote_oplog`/`import_baseline_items` 在同一事务内先置标记、提交前置回。搜索触发器不读标记，接收条目仍入 `search_outbox`。新增 3 个回声回归测试。

## #7 证据

- `src-tauri/src/config/store.rs:364-378`：`auto_sync()`/`set_auto_sync()`/`auto_sync_interval_secs()` 已存在。
- `src-tauri/src/commands/sync/mod.rs:176`：`sync_upload_backup` 仅作为 `#[tauri::command]` 注册（lib.rs:632），没有后台 worker 周期性调用它。
- UI 开关：`StorageSettingsDialog.svelte:3439`。
- 全库搜索无任何 worker 调 `sync_upload_backup`。
- ✅ 修复：新增 `commands/sync/auto.rs` `AutoSyncWorker`（仿 `SearchSyncWorker` 模式：stop_flag + join handle + Drop 停止），在 `lib.rs` setup 启动并 manage 为 `Mutex<Option<AutoSyncWorker>>`。worker 每秒重读受管 `ConfigStore` 的 `auto_sync()`/`auto_sync_interval_secs()`（设置改动无需重启即生效），启用且距上次尝试超过间隔时调用 `run_sync`。命令体重构为 `run_sync(&AppHandle)`（从 handle 解析受管状态），命令与 worker 共用同一入口，`SYNC_RUN_LOCK` 保证与手动同步串行。

## #8 证据

- 全局 `modified_ms` 水位既依赖远端元数据精度，也会在切换 provider/endpoint/path 后继承到另一远端，导致合法对象被误跳过。
- ✅ 当前修复：新增 `sync_remote_state` 与 `sync_applied_oplogs`，按 provider/endpoint/path/非秘密账户字段的 SHA-256 scope 隔离初始化、统计和已应用对象。新 oplog 使用不可变名称 `oplog-{device_id}-s{first_sequence}-e{last_sequence}-{sha256}`，成功处理后按 scope/object/revision 记账；旧时间戳命名对象可能被老客户端覆盖，因此每次都保守重读。

## #9 证据

- “最新 N 个文件”不是安全回收条件：长期离线设备可能从未见过被删 oplog，而新基线也不能证明它已经完成切换。
- ✅ 当前修复：删除 WebDAV/S3 自动按数量清理、首次多基线合并后的替换删除，以及手动维护中的 baseline/oplog 删除。首次同步只合并导入全部基线；兼容命令 `sync_compact_remote` 现在先完整同步，再追加一个新基线，前端称为“刷新同步快照”。
- `maxRemoteOplogFiles` 暂时仅保留配置，不执行删除；必须先设计每设备 acknowledgement/lease，再实现可证明安全的垃圾回收。

## #10 证据

- `src-tauri/src/commands/sync/mod.rs:181-183`：`config.lock()` 在 `sync_upload_backup` 开头取得，贯穿整个 `sync_upload_webdav`/`sync_upload_s3` 的网络 I/O，直到末尾（line 224-226 还写 status）才释放。
- 违背 `backend-architecture.md` 的锁纪律：配置锁应只覆盖读取配置的短临界区。
- ✅ 修复：新增 `SyncSettings` 快照结构（`sync_upload_backup`/`sync_list_remote_backups`/`sync_download_backup` 在锁内一次性采集全部同步相关配置后即释放锁）；`sync_upload_webdav`/`sync_upload_s3`/`encrypt_if_configured`/`decrypt_if_configured`/`resolve_s3_config` 改为读快照，不再持有锁；仅在成功后重新取锁写 `update_sync_status`。删除不再使用的 `max_remote_oplog_files(config)`。

## #11 证据

- `src-tauri/src/commands/sync/mod.rs:891-896`：`decrypt_if_configured` 里 `sync::crypto::decrypt(&data, &pwd).or(Ok(data))`，解密失败静默返回原始密文字节，之后按密文解析 oplog 失败被跳过，无明确报错。
- ✅ 修复：保留文档化的回退（密码设置前上传的明文远端文件仍可读），但解密失败时先 `println!("[sync] warning: ...")` 明确告警（改密码/密码不匹配可见），不再静默吞掉。

## #12 证据

- `src-tauri/src/sync/webdav.rs:24-41`：`build_client` 每次调用都 `Client::builder()` 新建。
- S3 侧已改为 `OnceLock` 共享客户端（`src-tauri/src/sync/s3.rs:28`）。
- ✅ 修复：WebDAV 改用 `shared_client()`（`OnceLock`，仅超时配置），鉴权改为每个请求 `.basic_auth(u, p)` 附加（Basic 头语义不变），删除 `build_client`。5 个调用点全部复用同一连接池。

## #13 证据

- `src-tauri/src/commands/sync/mod.rs:744-752`：`get_device_id` 回退 `"unknown"`。
- `src-tauri/src/sync/backup.rs:408-415`：另一份 `get_device_id` 回退 `"unknown-device"`。
- 仅合并 hostname 读取仍不稳定：机器改名会产生新身份，同名设备会碰撞，generic fallback 更会让多台设备共享 `unknown`。
- ✅ 当前修复：`Database::from_connection` 在 schema 创建后生成并持久化 v4 UUID；已有 UUID 重开保持不变。旧 hostname 值一次性迁移到 `legacy_device_id`，仅用于识别升级前本机已经上传的 oplog；空值、`unknown`、`unknown-device` 不保存为 alias。baseline/full backup/oplog manifest 和远端对象命名统一读取数据库 UUID，删除环境变量 hostname 实现。WebDAV/S3 下载循环同时识别当前 UUID 与 legacy alias，避免升级后重放自己的旧 oplog。新增持久性、迁移、fallback 与对象名识别回归测试。

## #15 证据

- 同步导入/应用路径（mod.rs 各 `import_baseline_items`/`apply_remote_oplog` 调用）后没有 emit `clipboard-history-invalidated`。
- 对比：`src-tauri/src/commands/export.rs:171` 与 `commands/clipboard/operations.rs:386` 有该事件。
- ✅ 修复：`sync_upload_backup` 命令接收 `AppHandle`，透传给 `sync_upload_webdav`/`sync_upload_s3`；任一路径应用远端数据（baseline 导入或 oplog apply 且 applied>0）后 emit `clipboard-history-invalidated`（deleted_ids 为空，触发主界面刷新）。

## #16 证据

- `sync_upload_backup` 与 auto-sync worker 都会执行同一套非重入的 oplog 上传/应用/快照逻辑，无任何并发保护时，交错执行会产生竞争。
- ✅ 修复：`commands/sync/mod.rs` 新增模块级 `static SYNC_RUN_LOCK: Mutex<()>`；`sync_upload_backup` 与 worker 共用的 `run_sync` 入口 `try_lock`，获取失败快速返回 `"sync already in progress"` 而非排队/交错。

## #14 证据

- 远端 oplog 历史文件名曾是 `oplog-{device_id}-{timestamp}.json`，但载荷实际为 bincode，`.json` 后缀具误导性且同名对象可被覆盖。
- ✅ 当前修复：wire 只保留单一 bincode v2 envelope，不再做 JSON fallback；新对象名为 `oplog-{device_id}-s{first_sequence}-e{last_sequence}-{sha256}`（无扩展名），序列范围和完整内容哈希共同形成不可变标识。旧时间戳命名对象仍可读取，但不会因元数据账本而被跳过。基线仍为真实 ZIP（内含 `baseline.bin` 与 `manifest.json`）。

## #17 证据

- 文本/链接捕获 ID 包含本机时间戳；两台设备独立捕获相同内容时，`kind + content_hash` 相同但 `id` 不同。SQLite 本地唯一约束会阻止重复插入，但旧 `apply_remote_oplog` 的更新/删除只查远端 ID，因此后续变更无法命中本地保留的另一 ID。
- ✅ 当前修复：新增 `sync_item_aliases(alias_id → item_id)`。baseline/oplog 首次遇到不同 ID 的同 kind/hash 文本或链接时保留本地行并记录远端 alias；后续内容即使编辑并改变 hash，更新/删除仍通过 alias 命中同一实体。alias 外键随实体永久删除级联清理。
- LWW 同时改为以 `COALESCE(modified_at_ms, created_at_ms)` 比较；远端应用期间 `clipboard_items_set_modified` 与 sync changelog trigger 一起受 suppression 守卫，接收端时钟不再覆盖发送端时间。新增 oplog 合并、baseline 合并、alias 重开持久性和远端时间戳回归测试。

## #18 证据

- 旧 `apply_remote_oplog` 对 insert/update/delete 都使用 `let _ = tx.execute(...)`，并且 insert 采用 `INSERT OR IGNORE`；CHECK/UNIQUE 等约束错误或 0-row 结果被吞掉，`applied` 仍加一，随后事务提交。WebDAV/S3 外层对 `Err` 也只打印日志并继续。
- ✅ 当前修复：所有语句使用 `?` 传播错误，insert 改为普通 `INSERT`，`applied` 只累计 SQLite 实际影响行数；未知 operation 返回 `InvalidSyncOperation`。任一记录失败会回滚整批数据、alias 与 suppression 状态。
- WebDAV/S3 apply 失败立即返回包含远端对象名的错误；只有整批成功后才写 `sync_applied_oplogs`，失败对象保留为下次可重试。新增“首条成功、次条约束失败仍整批回滚”和未知 operation 回归测试，并验证失败后本地 changelog 未被永久抑制。

## #19 证据

- 新资源统一使用 `category/sha256-{digest}.{ext}` 内容寻址；inline、资源池下载、本地缓存和上传源都验证摘要，损坏缓存可重新抓取，失败时不落盘。
- wire 路径统一拒绝绝对路径、父目录、反斜杠、未知类别、Windows 异常名、过深路径和 symlink；pool manifest/object path 复用同一校验。资源 materialize/rewrite 失败会带远端对象名中止，同步应用账本保持可重试。
- 保留旧 image/file hash 命名兼容，并以资源模块单测覆盖路径穿越、摘要不匹配、缓存修复、pool 缺失和 symlink 逃逸。

## #20 证据

- `RemoteStore` 定义 provider-neutral 的列表、原始上传/下载、加密 payload 和 remote scope 合约；blanket `PoolStorage` 实现让 baseline、oplog 与 `resources/<rel_path>` 使用同一对象路径和加密规则。
- `ConfiguredRemoteStore` 是唯一 WebDAV/S3 分支；手动/自动同步、远端列表/下载和非破坏快照刷新统一经过 `sync_with_remote_store` / `refresh_remote_snapshot`。内存 remote 测试覆盖 payload 加密与资源池对象路径。

## #21 证据

- `sync_download_backup` 在任何网络请求和 `temp_dir.join(...)` 之前验证文件名必须是一个安全的 `.zip` 普通组件。
- 回归测试覆盖正反斜杠穿越、绝对路径、盘符、Windows ADS、URL 编码分隔符、保留设备名和非 ZIP 文件名；无效输入不会形成远端对象路径或本地目标路径。
