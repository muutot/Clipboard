# 同步子系统审计清单（16 项）

> 本文档受版本管理，是同步子系统审计的权威记录。每完成一项，更新状态与提交号；证据以代码为准，不要凭意愿勾选。

## 严重：数据正确性

| #   | 问题                                                                                                            | 状态    | 证据 / 提交                                |
| --- | --------------------------------------------------------------------------------------------------------------- | ------- | ------------------------------------------ |
| 1   | 增量 oplog 不携带实际文本内容（text/html/rtf/metadata/is_favorite/source_app 丢失）                             | ✅ 完成 | `adc39a1`                                  |
| 2   | 图片/文件资源从不真正传输（serialize_oplog 只传元数据，create_oplog_backup 未接入同步流程）                     | ✅ 完成 | `924085c`（内嵌 OplogResource）            |
| 3   | 前端/后端 SyncUploadResult 字段完全不匹配（uploadedEntries vs itemsSynced）                                     | ✅ 完成 | `c287d07`                                  |
| 4   | S3 是伪实现（Authorization 为 `AWS key:placeholder`，无 SigV4，_region 被忽略，仅虚拟主机式 URL）               | ✅ 完成 | `201ecbd`（SigV4 + 端点解析 + path-style） |
| 5   | S3 oplog 清理删错 key：list_s3_objects 返回去前缀短名，delete 未重新拼 remote_path/ 前缀，且 `let _ =` 静默失败 | ✅ 完成 | 见下方                                     |
| 6   | 应用远程 oplog/导入基线触发本地 trigger，造成回声广播（收到的条目又生成 unsynced changelog 再广播回远端）       | ✅ 完成 | 见下方                                     |

## 高

| #   | 问题                                                                                               | 状态    | 证据 / 提交 |
| --- | -------------------------------------------------------------------------------------------------- | ------- | ----------- |
| 7   | auto_sync 是完全的死配置：有 UI 开关和 store 方法，但没有后台 worker/timer 调用 sync_upload_backup | ⏳ 待办 | 见下方      |
| 8   | 每次同步全量重放远端所有其他设备的 oplog，无已应用水位线，带宽/CPU 膨胀                            | ✅ 完成 | 见下方      |
| 9   | cleanup 只删其他设备的文件，本设备多轮 rollover 的旧 oplog 永不清理，远端无限增长                  | ✅ 完成 | 见下方      |
| 10  | ConfigStore 锁贯穿整个网络同步，命令是阻塞同步调用，慢网络阻塞其他配置访问                         | ✅ 完成 | 见下方      |
| 11  | 解密失败静默回退原始字节，改密码后旧文件按密文解析失败被跳过，无明确报错                           | ✅ 完成 | 见下方      |

## 低 / 优化

| #   | 问题                                                                                    | 状态                     | 证据 / 提交 |
| --- | --------------------------------------------------------------------------------------- | ------------------------ | ----------- |
| 12  | 每个网络调用重建 reqwest Client（webdav.rs:24）                                         | ✅ 完成（OnceLock 复用） | 见下方      |
| 13  | 两份 get_device_id 重复且回退值不一致（backup.rs:"unknown-device" vs mod.rs:"unknown"） | ✅ 完成                  | 见下方      |
| 14  | 文件名 .json 后缀实际是 bincode，仅靠 fallback 兼容，语义混乱                           | ⏳ 待办                  | 见下方      |
| 15  | 同步导入/应用后不广播 clipboard-history-invalidated，主界面可能不刷新                   | ✅ 完成                  | 见下方      |
| 16  | 同步无并发锁，手动+未来自动同步并发时合并逻辑存在竞争                                   | ✅ 完成                  | 见下方      |

## #5 证据

- `src-tauri/src/commands/sync/mod.rs:695`：`cleanup_old_s3_oplogs` 把 `list_s3_objects` 返回的 `name`（短名）直接传给 `delete_from_s3`。
- `src-tauri/src/sync/s3.rs:519`：`parse_s3_list_response` 用 `key.split('/').next_back()` 只保留最后一段（去前缀）。
- 配了 `remotePath` 时，真实对象 key 是 `{remotePath}/{name}`，短名删除会命中根路径的错误对象；`let _ =` 让错误完全静默。
- 对比：WebDAV 的 `cleanup_old_remote_oplogs`（mod.rs:782）把 `name` 传给 `delete_from_webdav(endpoint, remote_path, name, ...)`，由 webdav.rs 内部拼接路径，是正确的。
- ✅ 修复：提取 `s3_object_key(remote_path, name)`（trim 斜杠，空则取原名），`sync_upload_s3` 与 `cleanup_old_s3_oplogs` 共用；cleanup 删除前用 prefix 重拼完整 key，`let _ =` 改为 `?` 传播错误。新增 4 个单测。

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

## #8 证据

- 每个同步周期，webdav 与 s3 的下载循环（`sync_upload_webdav`/`sync_upload_s3`）都无差别下载、解密、解析远端所有其他设备的 oplog 文件，即使内容早已在上一轮应用过；随远端 oplog 数量线性增长，带宽/CPU 膨胀。
- ✅ 修复：`sync_metadata` 新增键 `sync_applied_oplog_watermark_ms`，`Database::get/set_sync_applied_oplog_watermark`（pool.rs:229/243）持久化「已成功应用远端 oplog 的最大 `modified_ms`」；两个下载循环开头读水位线，`entry.modified_ms <= watermark` 的文件直接跳过（无 mtime 的文件不跳过，保守处理），每成功应用一个文件用其 mtime 滚动 `max_applied_mtime`，循环结束后写回。新增回环单测。

## #9 证据

- `src-tauri/src/commands/sync/mod.rs:781`（webdav）与 `mod.rs:693`（s3）：cleanup 只删除 `!name.contains(device_id)` 的文件，即只删其他设备的；本设备 rollover 后的旧 oplog 永不清除。
- ✅ 修复：两个 cleanup 函数去掉本设备排除，改为 `keep_name` 参数只保护本次刚写入的文件（按 mtime 排序后保留最新 `max_files` 个，刚写入的是最新，天然安全），本设备多轮 rollover 产生的旧 oplog 同样受 `max_files` 上限约束被清理。

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
- ✅ 修复：合并为 `sync::device_id()`（`sync/mod.rs`），回退统一为 `"unknown"`；backup.rs 与 commands/sync/mod.rs 均改为调用共享实现，删除重复定义。新增回退单测。

## #15 证据

- 同步导入/应用路径（mod.rs 各 `import_baseline_items`/`apply_remote_oplog` 调用）后没有 emit `clipboard-history-invalidated`。
- 对比：`src-tauri/src/commands/export.rs:171` 与 `commands/clipboard/operations.rs:386` 有该事件。
- ✅ 修复：`sync_upload_backup` 命令接收 `AppHandle`，透传给 `sync_upload_webdav`/`sync_upload_s3`；任一路径应用远端数据（baseline 导入或 oplog apply 且 applied>0）后 emit `clipboard-history-invalidated`（deleted_ids 为空，触发主界面刷新）。

## #16 证据

- `sync_upload_backup` 与（即将接入的）auto-sync worker 都会执行同一套非重入的 oplog 合并/应用/清理逻辑，无任何并发保护，交错执行会产生竞争。
- ✅ 修复：`commands/sync/mod.rs` 新增模块级 `static SYNC_RUN_LOCK: Mutex<()>`；`sync_upload_backup` 入口 `try_lock`，获取失败快速返回 `"sync already in progress"` 而非排队/交错。auto-sync worker 复用同一命令入口即自动被串行化。
