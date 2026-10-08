# Clipboard Desktop v1.7.4

> Linux 事件驱动剪贴板监控、多窗口实时同步、OCR 可靠性增强，以及一批深层数据安全修复
>
> Released: 2026-10-08

---

## 新功能

### Linux 剪贴板监控

- **事件驱动监控** — Linux 不再依赖纯轮询：X11 会话通过 XFixes 选择区事件、Wayland 会话通过 data-control 协议实时感知剪贴板变化，轮询仅作为回退手段，采集延迟显著降低；`XDG_SESSION_TYPE` 缺失时额外探测 `WAYLAND_DISPLAY`，systemd/启动器环境下不再误判为 X11 而导致 Wayland 会话完全失采 | [`1ad670b`](https://github.com/muutot/Clipboard/commit/1ad670b8) [`0862e21`](https://github.com/muutot/Clipboard/commit/0862e219)
- **X11 断连存活** — X server 重启或会话注销时监控线程有序退出并停止采集，不再因 Xlib 默认 IO 错误处理器而杀死整个应用 | [`824b15a`](https://github.com/muutot/Clipboard/commit/824b15ad)
- **X11 大文本读取** — 超大剪贴板属性按 4MiB 分块读全；INCR 增量传输暂时显式拒绝并记录日志，替代原先的静默截断 | [`38fb8d`](https://github.com/muutot/Clipboard/commit/38fb8ddd)

### 多窗口实时同步

- **条目变更全窗口广播** — 收藏、重命名、删除等条目级变更现在广播到每一个打开的窗口，主窗口与快捷粘贴浮窗不再各自维护一份过时的副本 | [`e2c646f`](https://github.com/muutot/Clipboard/commit/e2c646f1) [`6542366`](https://github.com/muutot/Clipboard/commit/65423664)
- **复制即置顶于所有窗口** — 从任意窗口复制一条历史记录，该条目在每个窗口的列表中都同步置顶 | [`92373c`](https://github.com/muutot/Clipboard/commit/92373cfc) [`92373c`](https://github.com/muutot/Clipboard/commit/92373cfc)

### OCR 识别

- **失败重试** — 识别失败按有界次数自动重试，瞬时故障不再直接判死 | [`e138cb8`](https://github.com/muutot/Clipboard/commit/e138cb8f)
- **超大图防护** — 超尺寸图片先裁剪到识别上限再解码，不再把整幅巨型图完整载入内存 | [`148ed41`](https://github.com/muutot/Clipboard/commit/148ed411)

### 设置与界面

- **快捷键后端缺失提示** — 操作系统没有全局快捷键后端时在设置页明确告知，不再静默无效 | [`4a7b473`](https://github.com/muutot/Clipboard/commit/4a7b473b)
- **「系统标题栏」开关修复** — 补齐缺失的 `set-decorations` 权限，该设置项此前静默失效 | [`e99b12a`](https://github.com/muutot/Clipboard/commit/e99b12a0)
- **默认卡片间距加大** — 新默认值提高卡片内边距与间距，默认外观更舒展 | [`f78aa8c`](https://github.com/muutot/Clipboard/commit/f78aa8c3)
- **下拉框可达性与标签精简** — 选择器占位项可被键盘聚焦，侧栏云同步标签与磁盘说明文案缩短 | [`884b34a`](https://github.com/muutot/Clipboard/commit/884b34a9) [`1f1398e`](https://github.com/muutot/Clipboard/commit/1f1398ec) [`264ac82`](https://github.com/muutot/Clipboard/commit/264ac826)
- **标签页输入固定顶部** — 详情页标签添加输入框固定在标签页顶部，长标签列表下不再随滚动消失 | [`4cb46bf`](https://github.com/muutot/Clipboard/commit/4cb46bf2)

---

## 修复

### 数据安全与正确性

- **历史列表分页不再重复/丢行** — 从历史复制条目会将其置顶，导致 OFFSET 分页在翻页时重放或跳过行；分页改为以最近一页末行（effective_ts, created_at, id）为锚点的游标式分页，置顶条目落在锚点上方，下方行序永不位移 | [`3006f9`](https://github.com/muutot/Clipboard/commit/3006f948)
- **恢复的条目不再被保留期清理误删** — 从回收站恢复的旧条目现在会刷新捕获时间，不会再因早于保留期阈值而被随后的自动清理立即硬删 | [`c40dc8`](https://github.com/muutot/Clipboard/commit/c40dc895)
- **API token 文件创建即属主私有** — 本地 API token 文件直接以 owner-only 权限创建，消除「先创建后收窄」窗口期内其他用户读取的可能 | [`612fc9f`](https://github.com/muutot/Clipboard/commit/612fc9ff)
- **批量收藏失败回滚精确化** — 混合收藏/未收藏条目的批量操作失败时按逐项快照回滚，不再错误取消原本已收藏的条目 | [`7bc69c`](https://github.com/muutot/Clipboard/commit/7bc69cc8)
- **重命名与孤儿清理互斥** — 新增存储维护锁，条目重命名与孤儿文件清理串行执行，消除竞态窗口 | [`987c03`](https://github.com/muutot/Clipboard/commit/987c037e)
- **未配置的同步提供者不再拖垮媒体读取** | [`237fc3`](https://github.com/muutot/Clipboard/commit/237fc3a8)

### 采集稳定性

- **错误熔断覆盖图片/文件** — 媒体采集连续写库失败时进入与文本路径一致的退避熔断 | [`a7f0eb`](https://github.com/muutot/Clipboard/commit/a7f0eb0e)
- **自粘贴误捕窗口加宽** — 文本自触发标记有效期从 2 秒放宽到 5 秒，采集线程被大图阻塞时不再重复捕获自粘贴内容 | [`684f70`](https://github.com/muutot/Clipboard/commit/684f703c)
- **复制失败原因可诊断** — 文件消失与剪贴板占用分别报错并记录日志，复制失败不再无迹可查 | [`5a90cf9`](https://github.com/muutot/Clipboard/commit/5a90cf90) [`83e8a32`](https://github.com/muutot/Clipboard/commit/83e8a32a)

### 平台

- **macOS 不再虚报快捷粘贴能力** | [`4be12ac`](https://github.com/muutot/Clipboard/commit/4be12ac6)
- **X11 代码清理** — 补齐 Linux-only clippy 警告、修正偏移推进的无符号除法与过时注释 | [`bb1ce80`](https://github.com/muutot/Clipboard/commit/bb1ce80a) [`ba868b6`](https://github.com/muutot/Clipboard/commit/ba868b6e) [`11345af`](https://github.com/muutot/Clipboard/commit/11345af7)

### 发布链完整性

- **CI 校验 tag 绑定** — 推送 `v*` tag 时 CI 强制校验 HEAD 必须是 release commit，把「tag 只能绑定 release commit」从本地约定升级为远程强制门 | [`907d65`](https://github.com/muutot/Clipboard/commit/907d6558)
- **拒绝复用旧 tag** — 同版本重跑发布流程时，若已存在的 tag 与当前 HEAD 不一致则报错终止 | [`bef4b4`](https://github.com/muutot/Clipboard/commit/bef4b4c3)
- **补齐 clippy 1.99 门禁告警** — 修复 CI stable 工具链升级后 `needless_return` 判为编译错误的遗留告警，Linux/macOS 门禁恢复全绿 | [`d1587f`](https://github.com/muutot/Clipboard/commit/d1587fa6)

---

## 内部改进

- **单一数据源条目存储** — 四处条目副本收敛为一个带不变量测试的记录映射，前端状态不再各自为政 | [`cf9ba91`](https://github.com/muutot/Clipboard/commit/cf9ba916) [`8a49dd3`](https://github.com/muutot/Clipboard/commit/8a49dd33) [`332bca5`](https://github.com/muutot/Clipboard/commit/332bca59)
- **自触发标记惰性分配** | [`3f52b0f`](https://github.com/muutot/Clipboard/commit/3f52b0f6)

---

## 文档

- **README 全面修订** — 特性表合并隐私/扩展章节、修正默认快捷键与平台支持状态等事实性错误 | [`ba6a430`](https://github.com/muutot/Clipboard/commit/ba6a4303)
- **性能与能力声明对齐实测** — 以实测数据替换未测量的 100k P95 声明，全局热键/快捷粘贴按平台限定表述 | [`420f2ba`](https://github.com/muutot/Clipboard/commit/420f2bac) [`2c070b1`](https://github.com/muutot/Clipboard/commit/2c070b12)
- **搜索索引真实 schema 归档** | [`1393b52`](https://github.com/muutot/Clipboard/commit/1393b528) [`c827ab2`](https://github.com/muutot/Clipboard/commit/c827ab22)

---

## 构建产物

- **Windows**: `Clipboard_1.7.4_x64_en-US.msi` / `Clipboard_1.7.4_x64-setup.exe`
- **macOS (Apple Silicon)**: `Clipboard_1.7.4_aarch64.app.tar.gz` / `.dmg`
- **Linux (x64)**: `Clipboard_1.7.4_amd64.AppImage` / `.deb` / `.rpm`
