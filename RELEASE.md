# Clipboard Desktop v${version}

> 剪贴板桌面管理工具 — Windows 原生剪贴板监控、全文搜索、OCR 识别、多引擎支持
>
> Released: ${date}

---

## 核心功能

### 剪贴板监控与采集

- **Windows 剪贴板轮训监控** | 支持文本、链接、图片、文件多种格式自动采集 | [`7cf5cbd`](https://github.com/muutot/Clipboard/commit/7cf5cbd) [`ee8ab6f`](https://github.com/muutot/Clipboard/commit/ee8ab6f)
- **自动去重与自触发抑制** — 写入剪贴板时标记自身来源，避免重复采集同一内容 | [`54a696b`](https://github.com/muutot/Clipboard/commit/54a696b) [`694ba14`](https://github.com/muutot/Clipboard/commit/694ba14) [`75c0af8`](https://github.com/muutot/Clipboard/commit/75c0af8)
- **智能溯源** — 记录来源应用名称与可执行路径 | [`a79e041`](https://github.com/muutot/Clipboard/commit/a79e041) [`1b3ce2a`](https://github.com/muutot/Clipboard/commit/1b3ce2a)
- **文件管理** — 采集时自动复制文件到托管存储，支持大小限制与大文件链接保留 | [`3891c09`](https://github.com/muutot/Clipboard/commit/3891c09) [`c49a21e`](https://github.com/muutot/Clipboard/commit/c49a21e)

### 全文搜索

- **Tantivy 全文检索引擎** — 支持 CJK 中文 N-gram 分词、多字段排序、分页缓存 | [`97db771`](https://github.com/muutot/Clipboard/commit/97db771) [`b3bcbf7`](https://github.com/muutot/Clipboard/commit/b3bcbf7) [`4a4dbdc`](https://github.com/muutot/Clipboard/commit/4a4dbdc)
- **搜索分页与缓存** — 可配置分页大小、缓存条目上限，FIFO/LRU 淘汰策略 | [`3608081`](https://github.com/muutot/Clipboard/commit/3608081) [`5604e45`](https://github.com/muutot/Clipboard/commit/5604e45) [`d096be0`](https://github.com/muutot/Clipboard/commit/d096be0)
- **搜索排序规则** — 多级排序字段拖拽排序、防重复、自定义规则 | [`d6ac952`](https://github.com/muutot/Clipboard/commit/d6ac952) [`3f9a48a`](https://github.com/muutot/Clipboard/commit/3f9a48a) [`060a9d5`](https://github.com/muutot/Clipboard/commit/060a9d5)
- **搜索建议** — 可配置的搜索建议显示 | [`c2e6703`](https://github.com/muutot/Clipboard/commit/c2e6703) [`63289a9`](https://github.com/muutot/Clipboard/commit/63289a9)
- **一键重建索引** | [`e2028f6`](https://github.com/muutot/Clipboard/commit/e2028f6)

### OCR 文字识别

- **三种引擎支持** — Tesseract / Windows 原生 OCR / PP-OCRv6 (Rust ONNX) | [`4366a2d`](https://github.com/muutot/Clipboard/commit/4366a2d) [`94d97b7`](https://github.com/muutot/Clipboard/commit/94d97b7) [`be5cb62`](https://github.com/muutot/Clipboard/commit/be5cb62)
- **后台工作队列** — 异步处理、失败重试、同哈希复用结果 | [`1cb0dcf`](https://github.com/muutot/Clipboard/commit/1cb0dcf) [`e5b8015`](https://github.com/muutot/Clipboard/commit/e5b8015)
- **引擎热切换** — 设置中切换引擎无需重启应用 | [`f18790c`](https://github.com/muutot/Clipboard/commit/f18790c) [`389aa2d`](https://github.com/muutot/Clipboard/commit/389aa2d)
- **手动重试与状态反馈** | [`d1e6a3f`](https://github.com/muutot/Clipboard/commit/d1e6a3f) [`ab9ab1b`](https://github.com/muutot/Clipboard/commit/ab9ab1b)

### 回收站与收藏

- **软删除保护** — 删除后进入回收站，可恢复或永久删除 | [`61dcf4e`](https://github.com/muutot/Clipboard/commit/61dcf4e) [`5836962`](https://github.com/muutot/Clipboard/commit/5836962)
- **批量操作** — 批量收藏、恢复、删除，原子事务保护 | [`8b864ae`](https://github.com/muutot/Clipboard/commit/8b864ae) [`4f4146a`](https://github.com/muutot/Clipboard/commit/4f4146a)
- **按类型清理** — 按内容类型批量删除并级联清理搜索索引与 OCR 结果 | [`b464788`](https://github.com/muutot/Clipboard/commit/b464788) [`c988845`](https://github.com/muutot/Clipboard/commit/c988845)
- **定时历史清理** — 可配置的自动清理策略 | [`33f4ddc`](https://github.com/muutot/Clipboard/commit/33f4ddc)

---

## 界面特性

- **虚拟滚动列表** — 自适应卡片高度，支持图文混排、紧凑模式 | [`3494536`](https://github.com/muutot/Clipboard/commit/3494536) [`8fcfae3`](https://github.com/muutot/Clipboard/commit/8fcfae3) [`9a78132`](https://github.com/muutot/Clipboard/commit/9a78132)
- **详情面板** — 同画布切入 / 左右分栏两种展示模式 | [`64bb49b`](https://github.com/muutot/Clipboard/commit/64bb49b) [`c7cd722`](https://github.com/muutot/Clipboard/commit/c7cd722)
- **图片全屏查看器** — 缩放、平移、拖拽，独立窗口展示 | [`724c091`](https://github.com/muutot/Clipboard/commit/724c091) [`70670cc`](https://github.com/muutot/Clipboard/commit/70670cc) [`2f1171d`](https://github.com/muutot/Clipboard/commit/2f1171d)
- **紧凑模式** — 可自定义间距、高度、圆角、字号 | [`0331f3a`](https://github.com/muutot/Clipboard/commit/0331f3a) [`9e7833e`](https://github.com/muutot/Clipboard/commit/9e7833e) [`28f3000`](https://github.com/muutot/Clipboard/commit/28f3000)
- **内联编辑** — 双击编辑标题、文本内容、代码块、文件名 | [`4ce60ef`](https://github.com/muutot/Clipboard/commit/4ce60ef) [`c659348`](https://github.com/muutot/Clipboard/commit/c659348) [`3caab67`](https://github.com/muutot/Clipboard/commit/3caab67)
- **多选与右键菜单** — Ctrl+点击多选、批量操作、右键上下文菜单 | [`ca33351`](https://github.com/muutot/Clipboard/commit/ca33351) [`4c3d121`](https://github.com/muutot/Clipboard/commit/4c3d121)
- **键盘快捷键** — Ctrl+C/D/F/E 快速操作、Tab/方向键导航、Esc 退出 | [`d5277c8`](https://github.com/muutot/Clipboard/commit/d5277c8) [`f52d90c`](https://github.com/muutot/Clipboard/commit/f52d90c)
- **状态栏反馈** — 操作结果 Toast 提示、可关闭 | [`66b17f6`](https://github.com/muutot/Clipboard/commit/66b17f6) [`71b6890`](https://github.com/muutot/Clipboard/commit/71b6890)

---

## 自定义设置

### 主题系统

- **19 属性颜色系统** — 覆盖全部 UI 组件，深色/浅色预设 | [`bdf3550`](https://github.com/muutot/Clipboard/commit/bdf3550) [`563e014`](https://github.com/muutot/Clipboard/commit/563e014) [`8717302`](https://github.com/muutot/Clipboard/commit/8717302)
- **自定义方案管理** — 保存/应用/删除多套自定义主题 | [`c3480c6`](https://github.com/muutot/Clipboard/commit/c3480c6)
- **透明度支持** — #RRGGBBAA 八位 hex、窗口透明度调节 | [`b96b89a`](https://github.com/muutot/Clipboard/commit/b96b89a) [`fe3b275`](https://github.com/muutot/Clipboard/commit/fe3b275)
- **占位符颜色独立设置** | [`797faa5`](https://github.com/muutot/Clipboard/commit/797faa5)

### 字体与显示

- **独立字号控制** — 卡片标题/预览/备注文字字号分别设置 | [`b824138`](https://github.com/muutot/Clipboard/commit/b824138) [`2b4bb9a`](https://github.com/muutot/Clipboard/commit/2b4bb9a)
- **分页控制** — 单页加载数、最大加载条目上限、缓存条目数 | [`177d6d6`](https://github.com/muutot/Clipboard/commit/177d6d6) [`2799c7f`](https://github.com/muutot/Clipboard/commit/2799c7f) [`ddd7a42`](https://github.com/muutot/Clipboard/commit/ddd7a42)
- **操作按钮** — 悬停/常显两种显示模式 | [`a825943`](https://github.com/muutot/Clipboard/commit/a825943)
- **辅助文本行数** — 控制卡片辅助文字显示行数 | [`af6fbea`](https://github.com/muutot/Clipboard/commit/af6fbea)

### 存储管理

- **独立资源目录** — 图片、文件可配置独立存储路径 | [`e815687`](https://github.com/muutot/Clipboard/commit/e815687) [`4aa810c`](https://github.com/muutot/Clipboard/commit/4aa810c)
- **图标缓存管理** — 查看、清理应用图标缓存 | [`3eeb57c`](https://github.com/muutot/Clipboard/commit/3eeb57c) [`8a5f44d`](https://github.com/muutot/Clipboard/commit/8a5f44d)
- **磁盘用量统计** — 按类型展示存储用量与性能指标 | [`dbb1ab6`](https://github.com/muutot/Clipboard/commit/dbb1ab6) [`2d728f6`](https://github.com/muutot/Clipboard/commit/2d728f6)
- **安装时自选数据目录** | [`83e1729`](https://github.com/muutot/Clipboard/commit/83e1729)

### 隐私

- **密码管理器拦截** — 自动识别并拒绝采集密码管理器内容 | [`517ace1`](https://github.com/muutot/Clipboard/commit/517ace1) [`a4dac5f`](https://github.com/muutot/Clipboard/commit/a4dac5f)
- **敏感内容过滤** — 信用卡、私钥、凭证等正则匹配过滤 | [`72db7b7`](https://github.com/muutot/Clipboard/commit/72db7b7)
- **暂停采集** — 一键暂停/恢复剪贴板监控 | [`517ace1`](https://github.com/muutot/Clipboard/commit/517ace1)

### 窗口

- **无边框窗口** — 自定义拖拽区域、隐藏系统标题栏 | [`49779bc`](https://github.com/muutot/Clipboard/commit/49779bc) [`e1346eb`](https://github.com/muutot/Clipboard/commit/e1346eb)
- **窗口位置记忆** — 记住并恢复窗口位置，多显示器支持 | [`76657a6`](https://github.com/muutot/Clipboard/commit/76657a6) [`3af475e`](https://github.com/muutot/Clipboard/commit/3af475e)
- **最小宽度限制** — 730px 最小宽度避免布局破碎 | [`94f1f34`](https://github.com/muutot/Clipboard/commit/94f1f34) [`4d074b4`](https://github.com/muutot/Clipboard/commit/4d074b4)

---

## 平台支持

- **Windows** — 原生剪贴板 API、键盘 Hook（双修饰符连击检测）、SHGetFileInfoW 图标提取、置顶窗口、系统托盘与自启动 | [`7cf5cbd`](https://github.com/muutot/Clipboard/commit/7cf5cbd) [`9899100`](https://github.com/muutot/Clipboard/commit/9899100) [`ebfabf2`](https://github.com/muutot/Clipboard/commit/ebfabf2) [`973ebd1`](https://github.com/muutot/Clipboard/commit/973ebd1)
- **macOS** — 平台适配层（辅助功能权限、键盘 Hook、托盘、剪贴板监控） | [`ce97163`](https://github.com/muutot/Clipboard/commit/ce97163)
- **Linux X11/Wayland** — 平台适配层（托盘、全局快捷键、剪贴板监控、忽略应用） | [`ce97163`](https://github.com/muutot/Clipboard/commit/ce97163)

---

## 其他

- **CLI 命令行** — 复制/粘贴/搜索/导出/统计/删除，支持完整参数解析 | [`5f2fd06`](https://github.com/muutot/Clipboard/commit/5f2fd06) [`b7f640e`](https://github.com/muutot/Clipboard/commit/b7f640e) [`79dc54f`](https://github.com/muutot/Clipboard/commit/79dc54f)
- **HTTP 本地 API** — 健康检查、粘贴、列表、搜索、删除端点 | [`8af11b4`](https://github.com/muutot/Clipboard/commit/8af11b4) [`78b5d68`](https://github.com/muutot/Clipboard/commit/78b5d68)
- **导出/导入** — JSON / CSV / 纯文本格式，支持分页导出 | [`2e0b9f3`](https://github.com/muutot/Clipboard/commit/2e0b9f3) [`0d3f26e`](https://github.com/muutot/Clipboard/commit/0d3f26e)
- **i18n 国际化** — 简体中文 / English 完整双语支持 | [`fa34aa7`](https://github.com/muutot/Clipboard/commit/fa34aa7)
- **数据库自动备份与恢复** — 轮转备份、校验、损坏自动恢复 | [`6dba5fe`](https://github.com/muutot/Clipboard/commit/6dba5fe) [`385679f`](https://github.com/muutot/Clipboard/commit/385679f)
- **性能监控与诊断** — 启动计时、内存追踪、搜索延迟百分位统计 | [`25488b6`](https://github.com/muutot/Clipboard/commit/25488b6) [`d401f20`](https://github.com/muutot/Clipboard/commit/d401f20)
- **辅助功能** — 高对比度、弱动效、字体缩放、屏幕阅读器支持 | [`abe4fa0`](https://github.com/muutot/Clipboard/commit/abe4fa0)
- **内容检测与快捷操作** — URL/邮箱/电话/颜色/货币/IP/日期自动识别 | [`d12353a`](https://github.com/muutot/Clipboard/commit/d12353a) [`11c2f23`](https://github.com/muutot/Clipboard/commit/11c2f23)
- **文本变换** — 去空白/大小写转换/JSON 格式化/Base64/URL 编解码/哈希 | [`9ff684c`](https://github.com/muutot/Clipboard/commit/9ff684c)
- **Markdown 预览** — 代码块语法高亮、安全沙箱预览 | [`e59e06c`](https://github.com/muutot/Clipboard/commit/e59e06c)

---

## 构建产物

- **MSI 安装包**: `Clipboard_${version}_x64_en-US.msi`
- **NSIS 安装包**: `Clipboard_${version}_x64-setup.exe`
