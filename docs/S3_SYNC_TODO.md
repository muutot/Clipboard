# S3 同步功能实现 TODO

> 本文只依据当前存储格式编写，不读取、不复用、也不评估仓库内现有 sync 实现。当前存储格式证据见第 2 节；schema 中已存在的 `sync_*` 表仅作为存储表面看待，字段语义以本文档定义的新协议为准。

## 1. 目标

场景：

- 至少 3 台设备，每台本地已有 10 万条互不相同的记录。
- 每台设备平均每天新增 200 条记录。
- 各设备通过同一个 S3 桶作为唯一同步通道。

三个并列优化目标：

- 本地性能最好：同步不阻塞 UI，增量应用快，后台占用低。
- 存储占用最小：本地与 S3 都不保存冗余数据，派生数据不同步。
- 上传下载流量最少：只传增量，不重放历史，blob 按需下载。

## 2. 当前存储格式基线

以下事实来自存储层源码，是本 TODO 的唯一输入。证据文件：

- `src-tauri/src/storage/schema.rs`
- `src-tauri/src/storage/database.rs`
- `src-tauri/src/storage/paths/store.rs`
- `src-tauri/src/storage/paths/types.rs`
- `src-tauri/src/content/file_store.rs`
- `src-tauri/src/content/hash.rs`
- `src-tauri/src/search/manifest.rs`
- `src-tauri/src/config/types.rs`
- `src-tauri/src/config/store.rs`
- `src-tauri/src/commands/cleanup.rs`
- `src-tauri/src/storage/repository/impls.rs`

### 2.1 目录与文件

| 路径                                          | 内容                               | 是否可重建 |
| --------------------------------------------- | ---------------------------------- | ---------- |
| `conf/conf.json`                              | 应用配置 JSON，含同步/S3 配置区    | 否         |
| `storage/database/clipboard.sqlite3`          | SQLite v1 主库，WAL 模式           | 否         |
| `storage/database/clipboard.sqlite3-wal/-shm` | WAL/SHM 伴随文件                   | 是         |
| `storage/database/search-index/`              | Tantivy 搜索索引 + `manifest.json` | 是         |
| `storage/image/<sha256>.<ext>`                | 图片源文件，内容寻址               | 否         |
| `storage/image/previews/`                     | 图片预览缓存                       | 是         |
| `storage/files/<sha256>.<ext>`                | 文件副本，内容寻址                 | 否         |
| `storage/icons/`                              | 应用图标缓存，内容寻址             | 是         |

注意：超过复制大小上限的文件可能保留原始外部路径而不进入 `storage/files`，因此 `resource_path` 并不总是本地受管文件。

### 2.2 SQLite v1 关键表

- `clipboard_items`：`id`、`kind(text/link/image/file)`、`title`、`text_content`、`html_content`、`rtf_content`、`resource_path`、`preview_path`、`content_hash`、`source_app`、`icon_path`、`size_bytes`、`created_at_ms`、`last_used_at_ms`、`is_favorite`、`deleted`、`deleted_at_ms`、`modified_at_ms`、`sync_writer_device_id`、`metadata_json`，并有 `UNIQUE(kind, content_hash)`。
- `ocr_results`：按 `item_id` 关联的 OCR 文本、block JSON、模型版本等，属于派生数据。
- `tags`、`item_tags`：标签及其记录关联。
- `search_outbox`：搜索索引增量队列，派生自主表。
- `sync_metadata`：key-value，已有 `sync_enabled`、`sync_suppress_changelog`、`device_id` 等语义。
- `sync_item_aliases`：`alias_id -> item_id`，可用于同内容不同 ID 的合并映射。
- `sync_outbox`：自增 `sequence`、`item_id`、`operation(upsert/delete)`、`kind`、`content_hash`、`modified_at_ms`、`writer_device_id`。
- `sync_tombstones`：删除墓碑，含 `modified_at_ms`、`writer_device_id`，用于防止旧记录复活。
- `sync_publication_state`：每个 `remote_scope` 的 epoch、快照指针、已发布 sequence、最后 segment 等。
- `sync_cursors`：每个 `(remote_scope, device_id)` 的拉取游标。
- `sync_remote_resources`：已确认远端对象的 key、sha256、size。

主表触发器在 `sync_enabled = 1` 且未抑制 changelog 时写入 `sync_outbox` 与 `sync_tombstones`。当前更新触发器把 `last_used_at_ms` 变化也视为同步变化，这会放大网络流量，需要在数据契约阶段处理。

### 2.3 哈希与去重

- 文本/链接/文件记录哈希规则为 SHA256(kind + text + resource_path)；图片支持按解码后 RGBA 计算的归一化哈希。
- blob 文件名是 `{sha256}.{ext}`，同一内容天然只存一份。
- `UNIQUE(kind, content_hash)` 保证单机不重复；跨机同内容不同 ID 的收敛依赖别名映射。
- `resource_path`、`preview_path`、`icon_path` 是本地绝对路径，不能作为同步契约字段直接传输。

### 2.4 存储层结论

必须同步：记录元数据、文本/HTML/RTF 正文、标签、收藏状态、删除墓碑、版本信息。

禁止同步：搜索索引、OCR 结果、预览图、图标缓存、`last_used_at_ms` 等本地派生或本地使用数据；blob 只按需同步。

### 2.5 存储条目限制与清理语义

- `HistoryConfig` 当前默认 `max_items = 10_000`、`retention_days = 30`、`recycle_bin_days = 7`、`favorites_exempt = true`，配置持久化在 `conf/conf.json`。
- 容量与保留清理都是硬删除：容量超出时按 `created_at_ms ASC` 删除最旧的非收藏记录；保留期到期删除 `created_at_ms` 早于阈值的非收藏记录。
- 回收站是软删除：用户删除先置 `deleted = 1` 并记录 `deleted_at_ms`，超过 `recycle_bin_days` 后永久删除。
- 清理同时会清掉孤儿搜索索引与孤儿 blob 文件。
- 同步开启时，这些删除会经过主表删除触发器进入 `sync_outbox` 与 `sync_tombstones`，因此清理删除天然会传播到其他设备，而不是只作用于本机。
- 目标场景是每台 100k 条，当前默认上限只有 10k，必须显式提高上限并定义 scope 级策略，否则单机清理会把其他设备的记录一并删掉。

## 3. 设计取舍

1. 元数据增量 + 周期快照。日常只上传/下载增量 segment；定期生成一份压缩全量快照用于新设备引导和损坏恢复，避免重放全部历史。
2. 内容寻址去重。blob 以 SHA256 为 key，每个 scope 内只上传一次；上传使用条件写避免重复 PUT。
3. blob 按需下载。普通同步只交换元数据；图片/文件在打开或预览时才按哈希下载并校验，预览与图标本地重建。
4. 版本化收敛。以 `(modified_at_ms, writer_device_id)` 作为全序版本；删除用 tombstone；同内容不同 ID 通过别名表合并。
5. 本地批量应用。网络 IO 在 worker 线程完成且不持有数据库锁；应用端按批事务写入，通过现有触发器只增量更新搜索索引，绝不整库重建。
6. 压缩与小对象。segment 与 snapshot 使用 gzip（可选 zstd）压缩；发现用 HEAD/小指针对象优先，ListObjectsV2 兜底，不做全桶扫描。
7. 可恢复与幂等。所有远端对象不可变，失败重试不改变内容；本地游标在成功应用后才推进。
8. 容量与保留必须是 scope 级且确定性。`max_items`/`retention_days` 按同步 scope 统一生效，驱逐候选按 `(created_at_ms, id)` 全序选取且收藏豁免；任何设备执行清理都收敛到同一批 tombstone，避免“A 机删掉 B 机刚同步的记录”或删除风暴。

## 4. S3 对象布局

桶前缀建议为 `{remote_path}/v1/{scope}/{epoch}/`，其中 `scope` 区分同步组，`epoch` 在协议/数据契约升级或手动重置时轮换。

| 对象                                       | 可变性 | 用途                                                           |
| ------------------------------------------ | ------ | -------------------------------------------------------------- |
| `changelog/{device_id}/{seq:010}.jsonl.gz` | 不可变 | 单设备增量段，每行一个 upsert/delete/tombstone                 |
| `snapshots/{seq:010}.jsonl.gz`             | 不可变 | 全量状态快照，含 live 记录、墓碑、标签、各设备已发布 seq 地图  |
| `current.json`                             | 可变   | 指向最新 snapshot，含 epoch、snapshot_sha256、记录数           |
| `latest/{device_id}.json`                  | 可变   | 指向该设备最新 segment，含 seq、sha256、size                   |
| `blobs/{sha256}`                           | 不可变 | 内容寻址 blob，无扩展名，扩展名由记录元数据保存                |
| `acks/{device_id}.json`                    | 可变   | 可选：设备已应用游标，用于 GC 判断                             |
| `meta/scope.json`                          | 不可变 | scope 协议版本、schema 版本、加密参数、快照策略、容量/保留策略 |

约束：

- segment、snapshot、blob 一旦上传不可修改，名称单调递增或内容确定。
- 上传用 `If-None-Match: *`，重复上传不覆盖已有对象。
- `current.json` 更新采用 last-write-wins，读取方校验 epoch 与 checksum；快照上传成功后才切换指针。
- blob key 不依赖扩展名，避免同哈希不同扩展名产生重复对象。

## 5. 核心流程

### 5.1 发布

1. worker 按间隔或手动触发。
2. 读取 `sync_outbox` 中 `sequence > published_sequence` 的条目，按条目数/字节数分组。
3. 新 image/file 记录先按策略上传缺失 blob，再上传 metadata segment；超过大小上限的资源跳过并标记。
4. 压缩 segment、计算 SHA256、上传到 `changelog/{device_id}/{seq}.jsonl.gz`，再更新 `latest/{device_id}.json`。
5. 上传成功后推进 `sync_publication_state.published_sequence` 并清理已发布 outbox；失败保留 outbox 重试。

### 5.2 拉取

1. 读取 `current.json` 或列出 snapshot，判断 epoch/快照是否需要切换。
2. 对每个已知 peer 读取 `latest/{device_id}.json` 或列出 changelog 前缀，按 `sync_cursors` 下载缺失 segment。
3. 校验 sha256/size 后解析，按 `(modified_at_ms, writer_device_id)` 仲裁是否应用。
4. 应用期间设置 `sync_suppress_changelog`，防止远端变更回环成新的出站 op；批量事务写入并依赖触发器增量更新搜索索引。
5. 每批成功后推进 peer cursor；blob 不在此阶段下载，只登记待下载资源。

### 5.3 引导与恢复

- 新设备/损坏恢复只下载 `current.json` + 最新 snapshot + 快照之后的增量 segment。
- 不重放全部历史；snapshot 记录各设备已发布 seq，用于确定增量起点。
- 搜索索引在本地数据应用后通过现有 outbox/重建机制补齐。

### 5.4 垃圾回收

- 保留最新 snapshot 与自 snapshot 之后的 segment；旧 segment 在 peer ack 或保留期过后删除。
- 旧 snapshot 在新 snapshot 确认并切换指针后保留一个版本用于回退。
- 孤儿 blob 只有确认无任何 snapshot/segment 引用后才可删除，优先级低于元数据 GC。
- S3 Lifecycle 只清理不完整 multipart 上传与过期备份，不直接清理业务对象。

### 5.5 容量与保留驱逐

1. 每次本地变更应用结束或按清理周期检查 `deleted = 0` 计数；超过 scope 级 `max_items` 时按 `(created_at_ms, id)` 选择最旧的非收藏记录作为候选。
2. 驱逐走当前存储的硬删除路径，同一事务内写入 tombstone/outbox；小规模驱逐作为普通 delete op 发布，大范围驱逐见 5.6；重复驱逐必须幂等。
3. 保留期清理同样走 scope 级 `retention_days`，所有设备按同一阈值删除，避免旧记录在设备间反复复活。
4. 用户手动删除仍走软删除 + 回收站；回收站内恢复产生更新的 upsert，可覆盖 tombstone。
5. 永久删除后由本地孤儿清理删除无引用 blob；远端 blob 只有在快照/segment 均无引用且所有设备 ack 后才 GC。

### 5.6 大范围驱逐收敛

当单次驱逐量超过阈值（建议 > 1000 条）时，逐条 delete op 会放大上传、下载与本地写入成本，改为：

1. 发布端在本地按 scope 规则完成驱逐，只上传一个紧凑的驱逐标记，含 `evictionVersion`、`ruleVersion`、`scopeMaxItems`、`appliedAtMs`。
2. 在线端收到标记后不下载逐条 tombstone，按同一确定性规则在本地执行驱逐，并把结果登记为待发布/待快照状态。
3. 下一次快照在驱逐完成后生成，固化剩余 live 记录与驱逐 tombstone，作为新设备/离线设备的引导基线。
4. 快照确认且 peer ack 后，驱逐前的旧 segment 与冗余 tombstone 可 GC。

小规模驱逐（如 < 1000 条）继续走 5.5 的普通 delete op，避免等待快照。

## 6. 量化验收目标

| 指标           | 目标                                                                   |
| -------------- | ---------------------------------------------------------------------- |
| 单设备每日上行 | 200 条增量 + 开销，目标 < 1 MiB                                        |
| 单设备每日下行 | 两台 peer 共 400 条增量 + 轮询开销，目标 < 1 MiB                       |
| 稳态轮询开销   | 每轮 3-6 个 HEAD/GET 或 1-3 个 List，目标 < 100 KiB/日                 |
| 新设备引导     | 只下载 1 个 snapshot + 后续增量，300k 记录压缩目标 < 100 MiB           |
| 增量应用       | 200 条 p95 < 1 s，不包含 blob 物化                                     |
| 引导建库       | 100k 行数据库应用 < 60 s，搜索索引重建另计                             |
| 同步内存峰值   | 流式解析 snapshot/segment，峰值 < 256 MiB                              |
| 本地存储       | 无重复 blob、无同步的派生数据、outbox 有界                             |
| S3 元数据      | 最新 snapshot + snapshot 后增量，目标 < 2 倍 live 元数据量             |
| 正确性         | 3 台设备随机离线/乱序后最终一致，无丢数据、无重复显示                  |
| 驱逐一致性     | 3 台设备容量/保留策略一致，驱逐候选集相同，无删除风暴与复活            |
| 驱逐流量       | 普通驱逐每条 1 个 delete op；大范围驱逐只发布 1 个驱逐标记，由快照固化 |

## 7. 分阶段实施 TODO

所有条目完成后必须提供直接实现证据：代码、测试、基准或运行日志；stub、配置字段或未连接的模块不算完成。

### 阶段 0：数据契约与配置

- [ ] 定义 wire record schema：upsert/delete/tombstone 行格式、字段名、类型、协议版本；明确排除 `resource_path`、`preview_path`、`icon_path` 等本地路径。
  - 验收：serde 序列化单测覆盖字段集合，样例 JSON 不包含本地绝对路径。
- [ ] 定义同步范围：text/link 全量；image/file 按大小上限；标签与收藏参与；`last_used_at_ms`、OCR、搜索索引、预览、图标不参与。
  - 验收：字段映射测试 + 策略文档。
- [ ] 解决本地使用产生的同步噪音：当前更新触发器把 `last_used_at_ms` 变化写入 outbox，需通过过滤、独立列或触发条件调整，使本地使用不产生网络 op。
  - 验收：模拟本地复制/使用不产生出站 op；真正内容修改产生且只产生 1 个 op。
- [ ] 定义版本仲裁与冲突规则：`(modified_at_ms, writer_device_id)` 全序；同内容不同 ID 经 `sync_item_aliases` 合并；删除与更新按版本比较。
  - 验收：乱序、同版本、重启场景属性测试收敛。
- [ ] 定义 scope 级容量与保留策略：`max_items`/`retention_days` 在同步 scope 内统一生效；确认配置与 UI 支持 100k+；明确本地上限与 scope 上限的关系。
  - 验收：策略文档 + 配置读写测试覆盖不同设备配置的归一化。
- [ ] 同步开启后统一容量/保留设置：`max_items`、`retention_days`、`recycle_bin_days` 改由 scope 级配置生效；同步设置页新增这三条设置，原 `StorageSettingsDialog` 的三条在同步开启时禁用并显示提示，不隐藏。
  - 验收：开启同步后本地 history 设置不可写并显示来源提示；关闭同步后恢复可编辑；配置读写测试覆盖归一化与回写。
- [ ] 定义驱逐与回收站语义：容量/保留驱逐按 `(created_at_ms, id)` 确定性硬删除并生成 tombstone；用户删除走软删除/回收站；恢复生成更新的 upsert。
  - 验收：模拟超限、保留到期、回收站恢复的收敛测试，无复活、无重复 tombstone。
- [ ] 定义大范围驱逐标记：wire 协议新增紧凑驱逐 op/标记，含 `evictionVersion`、`ruleVersion`、`scopeMaxItems`、`appliedAtMs`，与逐条 delete op 并存。
  - 验收：serde 单测 + 协议文档；200k 驱逐不产生 200k 条 delete op。
- [ ] 确定 S3/同步配置存储：复用或扩展 `conf/conf.json` 的 sync 区，覆盖 endpoint、region、bucket、prefix、scope、credential、加密参数；凭据不进入日志。
  - 验收：配置读写单测；grep 日志输出无 secret。

### 阶段 1：S3 客户端与对象 IO

- [ ] S3 客户端抽象：put/get/head/delete/list/multipart，支持自定义 endpoint、path-style、region。
  - 验收：对 S3-compatible mock（MinIO/moto 等）的集成测试通过；断网、超时、权限错误映射为可恢复/不可恢复错误。
- [ ] 不可变对象幂等上传：`If-None-Match: *`；失败重试同一内容；上传前 HEAD 跳过已存在 blob。
  - 验收：mock 断言重复同步不产生重复 PUT。
- [ ] 下载校验：对象 sha256/size 校验，临时文件 + 原子 rename，并发去重。
  - 验收：损坏对象被拒绝并重试，不污染本地数据。
- [ ] 流式压缩封装：gzip（可选 zstd）+ 校验，segment/snapshot 读写不整包载入内存。
  - 验收：100 MiB snapshot 流式解析峰值内存 < 256 MiB。
- [ ] 大 blob multipart 与断点续传：超过阈值（建议 100 MiB）分片上传，支持中断恢复。
  - 验收：mock 分片上传成功；中断后恢复不重复完整上传。

### 阶段 2：发布端

- [ ] 读取未发布 outbox 并分组：按条目数/字节数生成 segment，header 含 device、from/to seq、epoch、sha256。
  - 验收：segment 内容与 DB 数据一致的确定性单测。
- [ ] 先传 blob 后传 metadata segment；新记录引用缺失或超限 blob 时执行跳过策略并标记。
  - 验收：新图片记录先出现 blob 对象，后出现 segment；超限资源不上传。
- [ ] 上传成功后才推进 published_sequence；失败保持 outbox 可重试，且重复上传幂等。
  - 验收：模拟上传失败/成功后的 DB 状态一致。
- [ ] 清理已发布 outbox，保留 tombstone 与发布状态；重启不重传已发布条目。
  - 验收：重启后 outbox 大小有界，无重复出站。
- [ ] 发布节流与触发：间隔/手动/立即同步，后台 worker 不阻塞 UI。
  - 验收：200 条/日场景满足第 6 节流量目标，UI 交互无卡顿。
- [ ] 大范围驱逐发布：超过阈值（建议单批 > 1000 条）时只发布驱逐标记，不逐条上传 delete op；小规模驱逐仍逐条。
  - 验收：mock 断言 200k 驱逐只产生 1 个标记对象与后续快照，无逐条 delete segment。

### 阶段 3：拉取端

- [ ] 发现与游标：按 peer 维护 `sync_cursors`，`latest` 指针优先、ListObjectsV2 兜底；支持断点续拉。
  - 验收：新增 segment 可被发现；重启不重下已应用 segment。
- [ ] 下载、校验、解析 segment 并原子落盘。
  - 验收：断网重试成功；损坏 segment 隔离不中断其他 peer。
- [ ] 批量应用：每批事务内比较版本、执行 upsert/delete/tombstone、设置 `sync_suppress_changelog` 防回环、触发搜索增量。
  - 验收：200 条应用 p95 < 1 s；应用远端变更后不产生回环出站 op。
- [ ] 拉取应用后超限处理：应用远端记录导致本地计数超过 scope 上限时，在同一批事务内按同一候选规则驱逐并登记待发布 tombstone。
  - 验收：拉取 300k 后按策略降到上限；驱逐不产生回环出站 op。
- [ ] 驱逐标记应用：收到标记后校验 `evictionVersion`/`ruleVersion`，按同一确定性规则本地驱逐并登记待发布状态；不依赖下载逐条 tombstone。
  - 验收：3 台设备不同时刻收到标记后候选集一致，无删除风暴。
- [ ] 冲突与别名：`UNIQUE(kind, content_hash)` 冲突合并到别名表；删除不复活；落后设备旧版本不覆盖新版本。
  - 验收：3 台设备乱序/离线测试最终一致。
- [ ] blob 按需下载：打开/预览时按哈希检查并下载，校验后原子落盘；并发去重；失败显示占位。
  - 验收：普通同步不下载 blob；打开记录后本地出现内容寻址文件。
- [ ] worker 生命周期：可安全停止，数据库锁不跨网络 IO，无残留临时文件。
  - 验收：关闭应用无挂起线程，临时目录为空。

### 阶段 4：快照与引导

- [ ] 快照生成：live 记录 + tombstone + 标签 + 各设备已发布 seq 地图，gzip + sha256。
  - 验收：由快照可重建与源库一致的状态。
- [ ] 快照调度与指针切换：按日或变更阈值生成，`current.json` 单调前进，上传成功后才切换。
  - 验收：新快照 seq 大于旧快照，指针指向已确认对象。
- [ ] 新设备引导：`current.json` + 最新 snapshot + 后续增量，不重放全部历史。
  - 验收：从空库到 300k 条只下载 1 个 snapshot 与增量，流量满足目标。
- [ ] 快照损坏回退：checksum 失败回退上一版本；旧 snapshot 保留一个版本。
  - 验收：模拟损坏后引导成功。
- [ ] 快照清理与生命周期：保留最新与上一版，旧增量按 ack/保留期删除。
  - 验收：桶对象数量与元数据大小有界。
- [ ] 快照固化驱逐：快照内容基于驱逐后的状态生成，包含驱逐 tombstone；快照确认 + peer ack 后 GC 驱逐前的旧 segment。
  - 验收：离线设备从新快照引导后不重放驱逐前历史。

### 阶段 5：性能、存储与流量基准

- [ ] 3 机端到端正确性：每台初始 100k 不同数据，每日 +200，随机离线/乱序后最终一致。
  - 验收：自动化集成测试 + 一致性报告。
- [ ] 容量/保留基准：3 台设备不同增量速度、不同超限时刻、收藏与回收站恢复混合场景。
  - 验收：最终一致且无删除风暴；满足第 6 节驱逐指标。
- [ ] 本地性能基准：增量应用 p95、引导建库耗时、内存峰值、UI 响应。
  - 验收：达到第 6 节目标并记录基线。
- [ ] 存储基准：本地 DB/blob 大小、S3 元数据大小、blob 去重率、outbox 上界。
  - 验收：达到第 6 节目标。
- [ ] 流量基准：每日上行/下行/轮询字节，blob 按需下载次数。
  - 验收：达到第 6 节目标。
- [ ] 故障恢复基准：断网、S3 5xx、限流、进程 kill、重复启动。
  - 验收：本地数据不损坏，恢复后收敛。

### 阶段 6：运维与可选增强

- [ ] GC 与 ack：peer 上报已应用游标，安全删除旧 segment/snapshot/孤儿 blob。
  - 验收：3 机 ack 后桶大小下降，无存活引用被误删。
- [ ] 同步状态 UI：最近同步时间、状态、待处理数、手动同步入口；不展示密钥。
  - 验收：设置页集成测试与视觉检查。
- [ ] 同步设置页容量/保留设置与禁用态：同步页新增 `max_items`/`retention_days`/`recycle_bin_days`；`StorageSettingsDialog` 对应三条在同步开启时禁用并显示原因提示，不隐藏。
  - 验收：设置页集成测试与视觉检查；关闭同步后三条恢复可用；按仓库 settings style gate 执行。
- [ ] S3 Event Notification/SQS 推送，减少轮询。
  - 验收：推送模式下轮询请求数为 0。
- [ ] 可选同步增强：OCR 结果按需同步、近期缩略图预取、多 scope/多桶、客户端 AEAD 加密。
  - 验收：每项独立开关与测试。

## 8. 明确不做

- 不同步搜索索引、OCR、预览、图标等可重建派生数据。
- 不做启动即全量下载 blob；blob 只按需物化。
- 不做 P2P 或自建服务端；同步通道仅限 S3-compatible。
- 不做账户/密码找回系统；凭据由用户在各设备配置。
- 不做冲突编辑 UI；采用确定性的版本全序与别名合并。
- 不做“本机上限触发删除但不同步”的本地裁剪：删除必须走 scope 级策略并生成 tombstone，否则其他设备会把记录重新同步回来。

## 9. 风险与待决问题

- 超过大小上限的 image/file 记录：默认跳过并本地标记，还是压缩后同步，需要产品决策。
- 加密策略：客户端 AEAD 加密 metadata，blob 用 SSE-S3/SSE-C，或两者组合；`sync_password` 变更需要 epoch 轮换。
- `last_used_at_ms` 触发噪音必须在阶段 0 解决，否则 200 条/日假设不成立。
- 设备永久离线会使 ack 型 GC 停摆，需要保留期兜底策略。
- 300k 记录 snapshot 的实际压缩比与引导耗时需要先基准，再定调度周期。
- S3 兼容性差异（path-style、region 空值、R2/MinIO 行为）需要 mock 之外的真实端点验证。
- 3 台设备各 100k 且同步后 union 可能 300k，与单机 `max_items = 100k` 冲突：需要决定 scope 上限设为 union 规模，还是接受确定性全局驱逐（后者会删掉部分记录）。
- 容量驱逐的发布者与版本：所有设备都执行清理会产生重复 tombstone，需要统一 writer/版本或幂等合并策略。
- 保留期与恢复窗口的交互：回收站恢复、离线设备晚到、保留期硬删除之间需要版本仲裁测试。
- 开启同步时不同设备原有三条配置可能不同，需要归一化规则：以 scope 配置覆盖、首次开启写入，还是要求用户确认。
- 驱逐标记发布后、快照完成前，离线设备恢复时仍可能拉取到驱逐前的旧增量，需要定义补齐与收敛窗口。
