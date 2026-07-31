# Clipboard Desktop v1.1.0

> 剪贴板桌面管理工具，Windows 原生剪贴板监控、全文搜索、OCR 识别、多引擎支持
>
> Released: 2026-07-31

---

## 版本说明

v1.1.0 以代码组织与发布管线可靠性为主：后端按模块彻底拆分，lib.rs 瘦身为边界组合；发布流程简化为「脚本 + 手动策划 RELEASE.md」的两遍模式；同时修复 macOS/Linux 交叉编译与 Clippy 问题，并移除无预编译 ONNX Runtime 的 Intel macOS 构建。

## 代码架构重构

- **后端模块化拆分** — commands / platform / storage / config / memory / cli 等按职责拆分为子模块目录 | [`c2f18f09`](https://github.com/muutot/Clipboard/commit/c2f18f09) [`562dd7b0`](https://github.com/muutot/Clipboard/commit/562dd7b0) [`fe5473b4`](https://github.com/muutot/Clipboard/commit/fe5473b4)
- **lib.rs 瘦身** — 状态、几何、关闭逻辑与 Tauri 命令分别提取到 core / commands 模块 | [`e5b9a59b`](https://github.com/muutot/Clipboard/commit/e5b9a59b) [`c2f18f09`](https://github.com/muutot/Clipboard/commit/c2f18f09)
- **共享逻辑提取** — SelfTriggerGuard 独立为 self_trigger 模块，CLI 文本构建与扫描助手复用 | [`58a3356d`](https://github.com/muutot/Clipboard/commit/58a3356d) [`cbe2dd9a`](https://github.com/muutot/Clipboard/commit/cbe2dd9a)
- **测试模块独立** — lib.rs 测试提取为 lib_tests.rs | [`41ffc36f`](https://github.com/muutot/Clipboard/commit/41ffc36f)

## 发布与构建流程

- **两遍发布流程** — RELEASE.md 改为人工策划，脚本在中间暂停等待，提交前强制格式检查 | [`2504a405`](https://github.com/muutot/Clipboard/commit/2504a405) [`1354e111`](https://github.com/muutot/Clipboard/commit/1354e111) [`14078c5c`](https://github.com/muutot/Clipboard/commit/14078c5c)
- **发布脚本简化** — 移除 verify/build 步骤，保留 regenerate 模式，自动检测是否需要强制推送 | [`0fa68774`](https://github.com/muutot/Clipboard/commit/0fa68774) [`9a40e38f`](https://github.com/muutot/Clipboard/commit/9a40e38f) [`3b269327`](https://github.com/muutot/Clipboard/commit/3b269327)
- **移除 Intel macOS 构建** — ort 不提供 x86_64-apple-darwin 预编译 ONNX Runtime，发布矩阵仅保留 macOS arm64 | [`f7be9e4f`](https://github.com/muutot/Clipboard/commit/f7be9e4f)
- **本地 CI 脚本** — ci:local 一键本地流水线，并移除硬编码 Rust 路径 | [`69baa75b`](https://github.com/muutot/Clipboard/commit/69baa75b) [`1bca1720`](https://github.com/muutot/Clipboard/commit/1bca1720)

## 跨平台构建修复

- **macOS Clippy 修复** — 修复阻断构建的 clippy 错误与未使用导入，补齐 sibling 模块引用前缀 | [`d22b0f1d`](https://github.com/muutot/Clipboard/commit/d22b0f1d) [`ee6e421f`](https://github.com/muutot/Clipboard/commit/ee6e421f) [`b65b00d2`](https://github.com/muutot/Clipboard/commit/b65b00d2)
- **条件编译门控** — Windows 专用模块（如 collect_system_memory）加 cfg 门控，保证交叉编译 | [`f5c0e710`](https://github.com/muutot/Clipboard/commit/f5c0e710)
- **导入清理与格式化** — 清理跨模块未使用导入，统一 cargo fmt / prettier 格式 | [`3ac56566`](https://github.com/muutot/Clipboard/commit/3ac56566) [`393af14e`](https://github.com/muutot/Clipboard/commit/393af14e)

---

## 构建产物

- **Windows (x64)** — MSI + NSIS 安装包
- **macOS (Apple Silicon, arm64)** — DMG 安装包
- **Linux (x64)** — deb / rpm / AppImage
