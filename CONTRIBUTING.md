# 贡献指南

感谢你对 Clipboard 项目的关注！本文档将帮助你搭建开发环境并提交代码。

## 行为准则

- 保持代码简洁，不添加未使用的依赖。
- 新增功能默认提供设置项（开关或选择框），并接入 i18n（中/英）。
- 设置样式改动遵循项目风格门禁：复用既有语义变量与卡片模式。
- 每个提交单元应可独立解释，不含无关文件。

## 开发环境

### 前置依赖

| 工具    | 最低版本      | 说明                    |
| ------- | ------------- | ----------------------- |
| Node.js | 18+           | 前端构建                |
| Rust    | 1.80+         | 后端编译                |
| Windows | 10/11         | 主要开发平台            |
| macOS   | 12+           | 辅助平台                |
| Linux   | Ubuntu 22.04+ | 辅助平台（X11/Wayland） |

### Windows 额外依赖

- [Microsoft Visual C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（含 Windows SDK）
- WebView2 Runtime（Windows 10 需手动安装，Windows 11 已内置）

### macOS 额外依赖

```bash
xcode-select --install
```

### Linux 额外依赖

```bash
# Ubuntu/Debian
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev

# 如需 OCR 支持（Tesseract）
sudo apt install tesseract-ocr tesseract-ocr-chi-sim
```

## 快速开始

```bash
# 克隆仓库
git clone <repo-url>
cd clipboard

# 安装前端依赖
npm install

# 启动开发服务器（前端热重载 + Tauri 窗口）
npm run dev

# 类型检查
npm run check

# 生产构建
npm run build

# 完整验证（CI 级别）
npm run verify
```

## 项目结构

参见 `.opencode/skills/clipboard-dev/SKILL.md` 中的完整架构说明。

核心目录：

```
src/                    # 前端（Svelte 5 + TypeScript）
  routes/
    +page.svelte        # 主界面
    settings/+page.svelte  # 设置窗口
  lib/
    components/         # UI 组件
    services/           # 前端服务层
    types/              # TypeScript 类型
    i18n/               # 国际化
src-tauri/              # 后端（Rust）
  src/
    storage/            # SQLite 数据层
    search/             # Tantivy 搜索
    ocr/                # OCR 引擎
    keyboard/           # 快捷键管理
    platform/           # 平台适配
```

## 开发约定

### 前端（Svelte 5）

- 使用 runes 语法：`$state`、`$derived`、`$effect`、`$props`
- 组件接口通过 `interface Props` 定义
- 样式使用 scoped `<style>` 块 + CSS 自定义属性（主题变量）
- 设置面板样式统一以 `GeneralSettingsPanel.svelte` 为基准

### 后端（Rust）

- `#[serde(rename_all = "camelCase")]` 用于所有序列化结构体
- Tauri command 返回 `Result<T, String>`
- 模块按功能划分：`storage/`、`search/`、`ocr/`、`keyboard/`

### i18n

- 键名使用点分路径：`"general.language"`
- 添加到 `zh-CN.ts` 和 `en.ts` 两处
- 同步更新 `types.ts` 中的类型定义

### 提交规范

- 提交信息格式：`type(scope): description`
- 常见 type：`feat`、`fix`、`style`、`docs`、`refactor`
- 常见 scope：`settings`、`ui`、`backend`、`storage`、`keyboard`
- 保持提交最小化，一个提交只做一件事

## 调试

### 前端

- 使用浏览器 DevTools（开发模式下 Tauri 窗口可打开 DevTools）
- `console.log` 输出在 Tauri 终端可见

### 后端

- 使用 `dbg!()` 宏或 `println!()` 输出调试信息
- 日志级别通过 `RUST_LOG` 环境变量控制

## 测试

```bash
# Rust 单元测试
npm run test:rust       # cargo test
cargo test -p clipboard_desktop_lib -- --nocapture

# Rust 代码检查
npm run lint:rust       # cargo clippy -D warnings

# 前端类型检查
npm run check           # svelte-check

# 格式化检查
npm run format:check    # Prettier + cargo fmt
npm run format          # 自动修复
```

## 问题反馈

- 使用 GitHub Issues 提交 Bug 报告或功能建议
- 附带复现步骤、预期行为和实际行为
- 如涉及 UI，请附上截图
