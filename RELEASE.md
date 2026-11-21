# Clipboard Desktop v1.8.5

> 主窗口透明度彻底修复：浅色背景现在能真正透出桌面；浅色调色板改为象牙白 #f8f8f9，彩色图标默认开启
>
> Released: 2026-11-21

---

## 新功能

### 设置

- **彩色图标默认开启** — 新配置默认以每个图标自带的彩色渲染，而不是单色；仍可在设置中关闭 | [`0079f796`](https://github.com/muutot/Clipboard/commit/0079f796)
- **搜索防抖可调** — 防抖时长改为可配置项（0–1000ms），设为 0 时改为回车触发搜索 | [`92f80fc8`](https://github.com/muutot/Clipboard/commit/92f80fc8)

---

## 透明度修复

这一版把主窗口透明度上三个互相独立的问题全部修掉了。

### 浅色背景现在真的能透出桌面

之前无论在设置里怎么拖动不透明度，浅色背景看起来都和完全不透明一样。根因在原生窗口层：`tauri-runtime-wry` 会给每个透明窗口创建一个 softbuffer 表面并填满整窗（丢弃 alpha），这层画在 WebView2 下方，于是 CSS 的透明度只能和它合成，永远到不了桌面。浅色 `#f8f8f9` 在白底上合成只差 3 个色阶，所以看起来毫无变化；暗色因为往白色方向变化跨了 91 个色阶，才「看起来能用」——那实际是变灰，不是透出桌面。

现在只在背景本该不透明时才填这层表面，半透明背景完全不填，CSS 透明度可以直接和桌面合成。 | [`fd340bf5`](https://github.com/muutot/Clipboard/commit/fd340bf5)

### 不透明度滑杆不再「调了没反应」

原实现只在保存值 ≥ 100 时才把窗口建成不透明，而 WebView2 的默认底色在控制器创建时就固定、且不可逆。一旦建成不透明，之后拉低不透明度只能在同一颜色上叠加，必须重启才「生效」。现在窗口始终以透明方式创建。 | [`033ab1f5`](https://github.com/muutot/Clipboard/commit/033ab1f5)

### 背景色不再叠加两层

`body` 和 `.app-shell` 之前都在用同一个透明度系数涂背景色，两层半透明叠起来后实际是 `1-(1-a)²`：滑杆 60% 时桌面只透 16%。现在只由 `.app-shell` 一层负责，滑杆值就是实际值。 | [`0ffbba09`](https://github.com/muutot/Clipboard/commit/0ffbba09)

### 其他

- **页面不再写主窗口原生底色** — 避免一次页面侧写入把主窗口永久冻成不透明白色 | [`7c018ccf`](https://github.com/muutot/Clipboard/commit/7c018ccf)

---

## 视觉

- **浅色调色板改为象牙白** — 浅色背景由 `#f5f5f5` 调整为 `#f8f8f9`，不透明度 100% 时精确呈现该颜色 | [`40318634`](https://github.com/muutot/Clipboard/commit/40318634)

---

## 构建与依赖

- **Rust 2024 edition** — 全仓升级到 edition 2024 | [`8f85f6d0`](https://github.com/muutot/Clipboard/commit/8f85f6d0)
- **SvelteKit 3** — 升级 SvelteKit 3 与 adapter-static 4 | [`9a15bad2`](https://github.com/muutot/Clipboard/commit/9a15bad2)
- **Windows 构建修复** — 链接前预创建 ort-sys 的 DLL 目录（覆盖各 target triple 与 copy_dylibs 查找路径），失败时 dump 布局取证；补入 NSIS 安装包所需的 `RestartManager.nsh` | [`740c3e00`](https://github.com/muutot/Clipboard/commit/740c3e00) [`41a18056`](https://github.com/muutot/Clipboard/commit/41a18056) [`bd615a5e`](https://github.com/muutot/Clipboard/commit/bd615a5e) [`bae645dd`](https://github.com/muutot/Clipboard/commit/bae645dd) [`70908ef3`](https://github.com/muutot/Clipboard/commit/70908ef3) [`9938d8d7`](https://github.com/muutot/Clipboard/commit/9938d8d7)
- **依赖更新** — jsdom 31.0.0 及其余 npm 依赖对齐最新兼容版本 | [`bf768e85`](https://github.com/muutot/Clipboard/commit/bf768e85) [`ea22dd3a`](https://github.com/muutot/Clipboard/commit/ea22dd3a)

---

## 内部整理

- **clippy 收敛** — 三处 `collapsible-if` 与 macOS 采集器守卫按 clippy 建议重构 | [`5a0f0ce1`](https://github.com/muutot/Clipboard/commit/5a0f0ce1) [`f5cbebae`](https://github.com/muutot/Clipboard/commit/f5cbebae) [`890021c1`](https://github.com/muutot/Clipboard/commit/890021c1)
- **测试稳定性** — 有界命令测试吸收夹具启动开销 | [`82cc9f00`](https://github.com/muutot/Clipboard/commit/82cc9f00)
