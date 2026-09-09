# TODO Roadmap — 1.5.2 / 1.5.3 / 1.6

> 版本映射：原 roadmap v1.6 → **1.5.2**（信任边界收尾），v1.7 → **1.5.3**
> （性能/可观测余量），v1.8 → **1.6**（结构还债 + 平台渐进）。
> 勾选规则（按 `skills/clipboard-dev`）：只有直接实现证据 + 相称验证覆盖
> 完整措辞时才可勾选；stub、配置项、UI 壳不算完成。

## 已验证完成（审计债已闭环，不再立项）

- [x] SEC-01 本地 API 认证 — `src-tauri/src/cli/api.rs:30-33,258-297` token + Host/Origin 校验 + 回归测试
- [x] SEC-02 asset scope 去 `"**"` — `src-tauri/tauri.conf.json:34`
- [x] SEC-03 rename 路径穿越 — `operations.rs:462,484-492` sanitize + file_name 断言
- [x] SEC-04 `copy_file_to` 已删除；`delete_icon_files` 已约束 — `files.rs:164-177,209-212`
- [x] SEC-06 OCR 下载尊重 localOnly + 超时 — `commands/ocr.rs:180-202`
- [x] SEC-07/08 open scheme 白名单 + tag 编码 — `files.rs:215-229`，`update.rs` urlencoding
- [x] STA-02 迁移前停后台 writer — `commands/config/storage.rs:86`
- [x] STA-03 rename 先写 DB 后改名 + 回滚 — `operations.rs:498-513`
- [x] STA-04 捕获 sequence 比对防混合 — `commands/capture.rs:430-441`
- [x] STA-05 DST midnight 容错 — `search/date_parser.rs:56-69`
- [x] STA-07 API 每连接独立线程 — `cli/api.rs` serve
- [x] PERF-01 Windows 内存探针复用 WinAPI — `performance/mod.rs:290-297`
- [x] PERF-02 单遍 raw+normalized hash — `commands/capture.rs:451-468`
- [x] PERF-03 PPaste 单事务导入 — `export/ppaste.rs:225-230`
- [x] PERF-04 canvas 测量 memo + 字体失效 — `src/lib/utils/virtual-scroll.ts:105-128`
- [x] TEST-01 Vitest 基建 + utils 覆盖；TEST-02 命令层补测（capture/operations/files/sync/export/update 均有 `#[test]`）
- [x] TEST-03 CI 不再忽略依赖清单 — `ci.yml` paths-ignore 仅 CHANGELOG/RELEASE
- [x] A11Y-01~05 可激活目标排除、toolbar 可达、焦点陷阱、隐藏按钮出 Tab 序、子菜单键盘 — `+page.svelte:2478-2496` 及 `focus.ts` 单测
- [x] FE-01 OFFSET 去重 + 失效 — `+page.svelte:699-738,985-988`；FE-02 桌面空列表启动
- [x] MAINT-02 漂移已收敛：`updateItem` 单漏斗同步四副本 — `+page.svelte:130-176`
- [x] DES-01 OCR 回调代替直接变异；DES-02 密码管理器单一名单
- [x] CFG-01 conf 损坏隔离重建 — `config/store.rs:26-33`；CFG-02 dbg_log 仅 debug — `lib.rs:8-21`
- [x] OBS 日志通道 `log_event` + 超限截断；剩余 `eprintln!` 仅 bench/CLI/logging 自身
- [x] PRIV-01 非法正则告警；PRIV-02 fail-closed（`state.rs:166-171` into_inner）
- [x] FB-01 tesseract_languages 回退；FB-02 索引打开重试 + 重建兜底（`search/index.rs:60-89`）；FB-03 tesseract 只出 full_text（`ocr/tesseract.rs:96-106`）
- [x] TS-01 删除多余 unsafe impl + Send 静态断言（`ocr/ppocr.rs:20,187-195`）
- [x] REL-01 action pin SHA + 权限收敛 + sha256sum 产物（`release.yml:194`）

## 1.5.2 — 信任边界收尾 + 数据安全

- [x] SEC-05 跨平台秘密存储：`platform::secret_store` 门面 + Windows DPAPI（存量）+ macOS login Keychain（`apple-native-keyring-store`）+ Linux Secret Service（`dbus-secret-service-keyring-store`，`crypto-rust`+`vendored` 免系统包）。`conf.json` 只存 `dpapi1:`/`oskey1:` 信封与标记；OS store 不可达时明文回退并打日志（不锁死）；mock-store 回环单测在全平台跑，mac/Linux 完整路径由 CI 覆盖。跟进：`conf/api.token` 仍为明文文件，另行立项。
- [x] SEC-04 余量：`replace_icon_file` 的 `source_path` 仅 `is_file` 校验。已加三重门（扩展名白名单对齐 dialog 过滤器 + 10MiB 上限 + 光栅可解码校验，`commands/files.rs::validate_replace_source`），4 个负向/正向单测通过；残余风险（被攻破渲染层搬运其他合法图片）已在注释记录，dialog 流程属用户显式同意。
- [x] REL-01 余量（决议：暂不做 SBOM）：产物已有 sha256sum（`release.yml`），第三方 action 已 pin SHA 且写权限收敛到 build job，`Cargo.lock` 锁定全量依赖。手写非标准 SBOM 价值低、标准工具（cargo-cyclonedx）引入 CI 工具链成本与收益不成比例；若未来进应用商店/企业分发再立项。
- [x] Linux uri-list 解析回归测试：`platform::parse_uri_list` 共享纯函数被 X11/Wayland 共用（`platform/mod.rs`），`file://` 解码、换行分隔、注释/非法行丢弃单测通过（`platform::tests::parse_uri_list_*`）。

## 1.5.3 — 性能 / 可观测余量

- [x] PERF-01 余量：macOS 改 `task_info`（`MACH_TASK_BASIC_INFO`）in-process 查询，`ps` 降级为兜底（`performance/mod.rs`）。绑定形状逐项对锁定的 libc 0.2.189 源码核实（含 `packed(4)` 非对齐读）；mac 一致性单测（原生 vs ps 4x 容差）由 mac CI 跑，Windows 侧回归全过。
- [x] 日志红线复核：全量审计 `log_event!`/`eprintln!`/`dbg_log` 插值——仅 id/路径/计数/配置键/错误展示，无剪贴板正文、OCR 文本、秘密、请求体（唯一例外：非法敏感正则原文的可视化警告，属设计意图）；前端 `console` 仅 DevTools，顺手去掉一处 clipboard 衍生 payload 插值（`ClipboardCard.svelte`）。红线写入 CONTRIBUTING。
- [ ] 同步引擎真实 S3 冒烟：现有 `ObjectStore` 替身与真实语义偏差（etag 引号/list 分页）仅靠单测。验收：MinIO nightly 冒烟（手动或 CI 可选 job）或明确记录不做。

## 1.6 — 结构还债 + 平台渐进

- [ ] MAINT-01 拆分巨型文件：`+page.svelte` ~3854 行、`DetailPanel.svelte` 2121 行、`ClipboardCard.svelte` 1555 行。先补 Vitest 基线再按视图区块拆（列表/筛选/批量/键盘导航），状态经 context/store 下发。验收：现有测试全过 + 键盘导航回归。
- [ ] MAINT-02 单一 source of truth：`Map<id, item>` + 派生 id 视图替代四副本手工同步（当前 `updateItem` 漏斗仅是缓解）。验收：四副本一致性单测 + Svelte 5 深代理兼容验证记录。
- [ ] MAINT-03 公共 API 注释：`capture.rs` / `search/index.rs` pub 项补 `///`（`export`/`sync/v1` 已完整可作样板）。
- [ ] 平台渐进（每项独立可验，不打包承诺）：
  - [ ] macOS 文件路径捕获：当前返回空（`platform/macos.rs:670-679`），调研 NSPasteboard 实现或记录不做。
  - [ ] Linux 图标提取：X11/Wayland `extract_app_icon` 均为 stub（`linux_x11.rs:1020-1032`），补实现或显式降级提示。
  - [ ] Wayland 写入自触发标记缺失 + 非 Windows 500ms 轮询回环风险：文档化现状与用户可见提示。
