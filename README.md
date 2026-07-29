<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="static/logo-dark.png">
    <img src="static/logo-light.png" alt="Clipboard Desktop">
  </picture>
</h1>

<p align="center">
  <strong>高性能 · 本地优先 · 全平台剪贴板管理器</strong><br>
  <sub>Built with Tauri 2 · Rust · Svelte 5 · Tantivy · SQLite</sub>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue" alt="platform">
  <img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="license">
  <img src="https://img.shields.io/badge/Tauri-2.x-ffc131?logo=tauri" alt="tauri">
  <img src="https://img.shields.io/badge/Svelte-5.x-ff3e00?logo=svelte" alt="svelte">
  <img src="https://img.shields.io/badge/Rust-edition2021-dea584?logo=rust" alt="rust">
</p>

---

## 简介

Clipboard Desktop 是一款**高性能、跨平台、本地优先**的剪贴板管理工具。它在后台静默运行，持续记录剪贴板历史。通过全局热键（默认 `Alt+V`）一键唤起，支持**全文搜索（含中文分词）**、**图片 OCR 文字识别**、收藏、编辑、快速粘贴等操作。

> **隐私承诺**：所有数据存储在本地，不包含任何遥测或上传行为。OCR 完全离线运行，无需联网。

### 快速预览

```
Alt+V 唤起 → 键入关键词搜索 → ↑↓ 导航 → Enter 粘贴
```

---

## 核心特性

<table>
<tr>
<td width="50%">

### 📋 剪贴板记录

- **文本** / **链接** / **图片** / **文件** 四种内容类型
- 智能内容识别：邮箱、电话、颜色值、日期、货币、IP
- 快速操作：一键发邮件、拨号、打开链接、查看日期
- 内容哈希去重，避免重复记录
- 自触发抑制，防止捕获自身写入
- 历史上限 10,000 条 / 30 天保留（可配置）

### 🔍 全文搜索

- Tantivy 全文检索引擎，N-gram 中文分词
- 多关键词无序 AND 匹配 + 相关性评分
- 自然语言日期搜索（"昨天"、"上周"）
- 来源应用名称参与搜索
- 搜索建议（下拉 / 内联提示）
- P95 < 30ms（10 万条记录）

</td>
<td width="50%">

### 🖼️ 图片 OCR

- PP-OCR 引擎，本地运行无需联网
- 后台任务队列，增量写入搜索索引
- 模型选择 / 下载 / 安装 / 热重载
- 识别状态面板，可查看 OCR 文本

### 🎨 现代 UI

- Svelte 5 响应式界面，暗色主题
- 虚拟滚动，万级列表流畅滚动
- 详情面板：覆盖切入 / 左右分栏双模式
- 图片全屏查看器，支持缩放拖拽
- 紧凑模式，自定义字体大小
- 自定义主题颜色（20 个 CSS 变量），暗/亮预设
- 系统托盘，透明窗口，开机自启

### ⚙️ 完善设置

- 双栏设置界面，分类导航 + 全局搜索
- 通用 / 外观 / 键盘 / 存储 / OCR / 主题 / 忽略应用 / 统计
- 5 级字体大小调节 + 像素输入
- 性能监控面板（启动耗时、搜索延迟、内存占用）

</td>
</tr>
</table>

### 🔒 隐私与安全

- **本地优先**：所有数据存储于本地 SQLite，无遥测
- **离线 OCR**：PP-OCR 引擎本地运行，无需联网
- **剪贴板暂停**：一键暂停/恢复记录
- **忽略应用**：密码管理器自动识别（1Password、Bitwarden、KeePass 等）
- **敏感内容**：可配置正则匹配模式

### 🔌 扩展能力

- **CLI 命令行**：`clipboard search` / `clipboard list` / `clipboard copy`
- **本地 API**：HTTP 接口供外部脚本调用
- **导入导出**：JSON / CSV / 纯文本，支持类型和日期筛选
- **快捷键系统**：全局热键 + 应用内快捷键 + 双击修饰键（Shift+Shift）

---

## 技术栈

<p align="left">
  <img src="https://skillicons.dev/icons?i=tauri,rust,svelte,typescript,vite,sqlite" alt="tech stack" />
</p>

| 层级     | 技术                       | 说明                                                       |
| :------- | :------------------------- | :--------------------------------------------------------- |
| 桌面框架 | **Tauri 2**                | 轻量跨平台桌面壳，Rust 后端 + Web 前端                     |
| 后端     | **Rust** (edition 2021)    | 剪贴板监听、存储、搜索、OCR、快捷键                        |
| 数据库   | **SQLite** (rusqlite 0.40) | 版本化 Schema，仓储模式，自动备份恢复                      |
| 搜索引擎 | **Tantivy 0.26**           | 全文检索，自定义 N-gram 分词（中文友好）                   |
| OCR      | **oar-ocr 0.8** (PP-OCR)   | 本地 ONNX 推理，支持 Tesseract 备选                        |
| 前端     | **Svelte 5 + SvelteKit**   | Runes 响应式语法，SPA 模式                                 |
| 构建     | **Vite 6 + Cargo**         | 前端 HMR + Rust 增量编译                                   |
| 打包     | **Tauri Bundler**          | NSIS (Windows) / App Bundle (macOS) / Deb/AppImage (Linux) |

---

## 快速开始

### 环境要求

- **Node.js** >= 18
- **Rust** 稳定版工具链
- 对应平台的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)

### 安装

```powershell
# Windows
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
```

```sh
# macOS
brew install node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

```sh
# Linux (Ubuntu/Debian)
sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev patchelf libssl-dev libgtk-3-dev \
  libjavascriptcoregtk-4.1-dev libsoup-3.0-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 开发运行

```sh
npm install          # 安装前端依赖
npm run tauri dev    # 启动桌面应用（热重载）
npm run dev          # 或仅运行前端（浏览器预览 + 演示数据）
```

### 生产构建

```sh
npm run tauri build  # 在 src-tauri/target/release/bundle/ 生成安装包
```

---

## 多平台支持状态

| 功能                       | Windows       | macOS                              | Linux X11              | Linux Wayland                    |
| :------------------------- | :------------ | :--------------------------------- | :--------------------- | :------------------------------- |
| 读取剪贴板文本             | ✅ 原生 Win32 | ✅ 原生 ObjC FFI                   | ✅ 原生 Xlib FFI       | ⚠️ `wl-paste`                    |
| 写入剪贴板（含自触发标记） | ✅ 原生 Win32 | ⚠️ `pbcopy`（无标记）              | ⚠️ `xclip`（无标记）   | ⚠️ `wl-copy`（无标记）           |
| 读取剪贴板图片             | ✅ 原生 Win32 | ⚠️ `pngpaste` / `osascript`+`sips` | ⚠️ `xclip`             | ⚠️ `wl-paste`                    |
| 读取文件路径               | ✅ 原生 Win32 | ❌ 未实现                          | ❌ 未实现              | ❌ 未实现                        |
| 获取前台应用               | ✅ 原生 Win32 | ✅ 原生 ObjC FFI                   | ✅ 原生 Xlib + `/proc` | ⚠️ `swaymsg`/`hyprctl`/`xdotool` |
| 提取应用图标               | ✅ 原生 Win32 | ⚠️ `plutil` + `sips`               | ❌ 未实现              | ❌ 未实现                        |

- ✅ **原生 API** — 直接 FFI 调用系统接口，无外部依赖
- ⚠️ **命令行工具** — 依赖外部可执行文件，部分工具可能需额外安装
- ❌ **未实现** — 返回空值/错误，尚不支持

> **注意**：非 Windows 平台使用 500ms 文本轮询检测剪贴板变更，且不检查自触发标记，可能产生写入回环。

---

## 开发命令

| 命令                   | 说明                                                  |
| :--------------------- | :---------------------------------------------------- |
| `npm run dev`          | 启动 Vite 开发服务器（仅前端）                        |
| `npm run build`        | 前端生产构建                                          |
| `npm run tauri dev`    | Tauri 桌面应用开发模式                                |
| `npm run tauri build`  | Tauri 桌面应用生产构建                                |
| `npm run check`        | TypeScript / Svelte 类型检查                          |
| `npm run format`       | 代码格式化（Prettier + `cargo fmt`）                  |
| `npm run format:check` | 格式检查（不修改文件）                                |
| `npm run test:rust`    | Rust 单元测试                                         |
| `npm run lint:rust`    | Rust Clippy 检查                                      |
| `npm run verify`       | **全量检查**：格式 + 类型 + 构建 + Rust 测试 + Clippy |

### 推荐 IDE 插件

- [Svelte for VS Code](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode)
- [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

---

## 项目结构

```
clipboard/
├── src/                         # Svelte 5 前端 (SPA)
│   ├── routes/
│   │   ├── +page.svelte         # 主界面（剪贴板列表、搜索、详情）
│   │   └── settings/+page.svelte # 设置界面
│   └── lib/
│       ├── components/          # 可复用组件
│       ├── services/            # Tauri IPC 封装层
│       ├── types/               # TypeScript 类型定义
│       ├── i18n/                # 国际化（zh-CN / en）
│       ├── utils/               # 工具函数
│       └── data/                # 浏览器预览演示数据
├── src-tauri/                   # Rust 后端
│   ├── tauri.conf.json          # Tauri 2 配置
│   ├── Cargo.toml               # Rust 依赖
│   └── src/
│       ├── main.rs              # 入口
│       ├── lib.rs               # 命令注册与应用初始化
│       ├── domain/              # 领域模型
│       ├── storage/             # SQLite 数据库与仓储
│       ├── search/              # Tantivy 搜索引擎
│       ├── ocr/                 # OCR 引擎与任务队列
│       ├── keyboard/            # 快捷键解析匹配
│       ├── platform/            # 平台适配（Windows / macOS / Linux）
│       ├── content/             # 内容检测、哈希、缩略图
│       ├── privacy/             # 隐私管理
│       └── export/              # 导入导出
├── docs/                        # 设计文档
│   ├── SEARCH.md                # 搜索架构
│   ├── OCR.md                   # OCR 架构
│   └── PITFALLS.md              # 开发陷阱与约定
├── static/                      # 静态资源
├── TODO.md                      # 项目路线图
├── CONTRIBUTING.md              # 贡献指南
└── package.json
```

---

## 文档

| 文档                                                         | 内容                                          |
| :----------------------------------------------------------- | :-------------------------------------------- |
| [TODO.md](TODO.md)                                           | 项目路线图与进度追踪                          |
| [CONTRIBUTING.md](CONTRIBUTING.md)                           | 开发环境搭建、编码规范、提交规范              |
| [docs/SEARCH.md](docs/SEARCH.md)                             | 搜索架构：Tantivy 索引、N-gram 分词、查询策略 |
| [docs/OCR.md](docs/OCR.md)                                   | OCR 管线：引擎选择、模型管理、任务队列        |
| [docs/PITFALLS.md](docs/PITFALLS.md)                         | Svelte 5 / Tauri / Rust 开发陷阱              |
| [docs/DEFAULTS_AND_PRIVACY.md](docs/DEFAULTS_AND_PRIVACY.md) | 默认策略与隐私边界                            |

---

## 贡献

欢迎提交 Issue 和 Pull Request。请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 了解开发规范和提交流程。

提交消息格式遵循 **gitmoji 约定**：

```
<gitmoji> <type>[<scope>]: <message>
```

示例：`✨ feat[search]: add backend SearchResultCache` | `🐛 fix[viewer]: handle window close state`

---

## 许可证

AGPL-3.0 © Clipboard Desktop Contributors
