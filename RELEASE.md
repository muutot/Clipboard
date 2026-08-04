# Clipboard Desktop v1.2.0

> 剪贴板桌面管理工具，Windows 原生剪贴板监控、全文搜索、OCR 识别、多引擎支持
>
> Released: 2026-08-04

---

## 版本说明

v1.2.0 聚焦「复制粘贴体验」与「界面设置统一」：新增富文本格式粘贴（CF_HTML）、一键净化粘贴（去空白 / 折叠空白 / 去除 URL 跟踪参数）、搜索索引懒加载同步、自定义搜索框占位符、多格式导入导出、版本更新检查与毛玻璃窗口效果；设置与主界面的下拉选择器全面统一为自定义 Popover 样式；硬编码中文文案全部接入 i18n。同时修复搜索分页越界、窗口位置钳制、剪贴板轮询重启等可靠性问题，并加固本地 API（移除通配 CORS）与 CSV 导出（防公式注入）安全边界。

## 复制粘贴体验

- **富文本格式粘贴** — 采集并持久化 CF_HTML，卡片新增按原格式粘贴操作 | [`554b4cdb`](https://github.com/muutot/Clipboard/commit/554b4cdb) [`89429ec1`](https://github.com/muutot/Clipboard/commit/89429ec1) [`c8ce56d3`](https://github.com/muutot/Clipboard/commit/c8ce56d3)
- **净化粘贴** — 内置净化管线（trim、空白折叠、URL 跟踪参数移除），全局开关 + 卡片 / 详情面板逐条净化粘贴，覆盖纯文本与格式化粘贴路径 | [`d9b8cab0`](https://github.com/muutot/Clipboard/commit/d9b8cab0) [`9e842289`](https://github.com/muutot/Clipboard/commit/9e842289) [`97fead6f`](https://github.com/muutot/Clipboard/commit/97fead6f) [`30ad79b8`](https://github.com/muutot/Clipboard/commit/30ad79b8)

## 设置与界面

- **统一 Popover 下拉** — 设置面板原生 select 替换为主界面同款自定义下拉，弹出定位、右对齐、宽度自适应、悬停样式全链路统一 | [`43242c9f`](https://github.com/muutot/Clipboard/commit/43242c9f) [`31173876`](https://github.com/muutot/Clipboard/commit/31173876) [`42de5630`](https://github.com/muutot/Clipboard/commit/42de5630) [`01d2c54a`](https://github.com/muutot/Clipboard/commit/01d2c54a)
- **毛玻璃窗口效果** — 新增 frosted glass 窗口视觉效果 | [`ad4646de`](https://github.com/muutot/Clipboard/commit/ad4646de)
- **多格式导入导出** — 导入 / 导出 GUI，支持收藏、内容类型、日期范围等完整导出选项 | [`525e2cb0`](https://github.com/muutot/Clipboard/commit/525e2cb0) [`7f315fb6`](https://github.com/muutot/Clipboard/commit/7f315fb6)
- **版本更新检查** — 通过 GitHub Releases 检查新版本 | [`312e82e3`](https://github.com/muutot/Clipboard/commit/312e82e3)
- **搜索索引同步模式** — 支持 lazy / background 两种同步策略，可在设置中切换 | [`ee72886f`](https://github.com/muutot/Clipboard/commit/ee72886f) [`f57907dd`](https://github.com/muutot/Clipboard/commit/f57907dd)
- **自定义搜索框占位符** — 搜索框占位文案可自定义 | [`746e3ab5`](https://github.com/muutot/Clipboard/commit/746e3ab5)
- **侧栏显示真实版本号** | [`3f4aa5a0`](https://github.com/muutot/Clipboard/commit/3f4aa5a0)

## i18n 国际化

- **文案全面接入 i18n** — 硬编码中文替换为 `_t()` 调用，修复乱码并补齐语言切换 Toast 文案 | [`0a713621`](https://github.com/muutot/Clipboard/commit/0a713621) [`80e6e25f`](https://github.com/muutot/Clipboard/commit/80e6e25f)

## 搜索性能与可靠性

- **分块文档查询** — 大批量 IN 查询按 500 分块，避免超长 SQL | [`3aa49c17`](https://github.com/muutot/Clipboard/commit/3aa49c17)
- **搜索页缓存优化** — 缓存时避免额外克隆；无待处理事件时跳过 outbox 同步 | [`a6b21886`](https://github.com/muutot/Clipboard/commit/a6b21886) [`1b306bcd`](https://github.com/muutot/Clipboard/commit/1b306bcd)
- **搜索前排空 outbox** — 新采集内容立即参与搜索，修复索引延迟 | [`10ea45ca`](https://github.com/muutot/Clipboard/commit/10ea45ca)
- **页大小钳制** — 请求分页大小按配置上限钳制，防止越界 | [`9800b0bd`](https://github.com/muutot/Clipboard/commit/9800b0bd)
- **索引健康探针** — validate 改为探测索引健康状态 | [`91b6a98f`](https://github.com/muutot/Clipboard/commit/91b6a98f)

## 安全与健壮性

- **CSV 公式注入防护** — 以 `=` `+` `-` `@` 开头的单元格自动加引号 | [`109e8565`](https://github.com/muutot/Clipboard/commit/109e8565)
- **本地 API 加固** — 移除通配 CORS 头，新增 `X-Content-Type-Options: nosniff` | [`0b20d3be`](https://github.com/muutot/Clipboard/commit/0b20d3be)
- **全屏查看器 DOM 安全构建** — 改用安全元素 API 构建 DOM，消除 innerHTML 注入面 | [`f8bb29ec`](https://github.com/muutot/Clipboard/commit/f8bb29ec)
- **配置原子写入** — 配置与键盘设置采用临时文件 + rename 原子落盘，避免写入中断损坏 | [`a5a1b1ff`](https://github.com/muutot/Clipboard/commit/a5a1b1ff)
- **CLI 存储路径一致** — 进程 CLI 通过 `StoragePaths` 解析数据库路径，自定义数据目录与 GUI 保持一致 | [`78f34143`](https://github.com/muutot/Clipboard/commit/78f34143)

## 修复

- **窗口位置钳制** — 工作区小于最小尺寸时以工作区为界，避免越界 | [`93578629`](https://github.com/muutot/Clipboard/commit/93578629)
- **多字节截断** — 标题 / 预览截断不再拆散多字节字符 | [`22c23701`](https://github.com/muutot/Clipboard/commit/22c23701)
- **详情面板同步** — 条目变更时保持打开中的详情面板同步 | [`5bdb85e7`](https://github.com/muutot/Clipboard/commit/5bdb85e7)
- **剪贴板轮询重启** — 统一 capture 循环，修复监控重启 | [`471898f1`](https://github.com/muutot/Clipboard/commit/471898f1)
- **下拉弹窗稳定性** — 修复弹出定位 / 重开定位 / 选项宽度一系列 Popover 问题 | [`462ac5e0`](https://github.com/muutot/Clipboard/commit/462ac5e0) [`3060b4f1`](https://github.com/muutot/Clipboard/commit/3060b4f1)

---

## 构建产物

- **Windows (x64)** — MSI + NSIS 安装包
- **macOS (Apple Silicon, arm64)** — DMG 安装包
- **Linux (x64)** — deb / rpm / AppImage
