# Clipboard Desktop v1.8.4

> 批量粘贴重做：合并为单次写入单次交付，附批量诊断；一批保存/隐私/更新链路的正确性修复，以及复制性能优化
>
> Released: 2026-11-11

---

## 新功能

### 批量粘贴

- **单次写入单次交付** — 批量粘贴不再逐条写入剪贴板，合并为一次写入一次交付，减少中间状态与触发次数 | [`dd104f87`](https://github.com/muutot/Clipboard/commit/dd104f87)
- **批量诊断** — 交付失败时输出批量粘贴诊断，便于定位是哪一条、哪一环失败 | [`2e4730fd`](https://github.com/muutot/Clipboard/commit/2e4730fd)

---

## 正确性加固

### 粘贴链路

- **批量租约** — 每次批量操作只租用一个已校验的粘贴目标，中途窗口切换不再改变落点 | [`5244015b`](https://github.com/muutot/Clipboard/commit/5244015b)
- **窗口隐藏时暂存** — 后续批量条目在窗口隐藏期间暂存，避免中途打断前台应用 | [`41d37415`](https://github.com/muutot/Clipboard/commit/41d37415)
- **shift 范围方向** — shift 多选粘贴时保留选择方向，正序/倒序不再被反向 | [`f6dd90ae`](https://github.com/muutot/Clipboard/commit/f6dd90ae)
- **HTML 写入走后端** — 桌面端 HTML 写入统一经由后端 writer，前端直写路径移除 | [`2752cf86`](https://github.com/muutot/Clipboard/commit/2752cf86)

### 保存与文件

- **保存前能力检查** — 选择目标位置前先检查保存能力，资源缺失在选目录前明确解释 | [`1dd83bd0`](https://github.com/muutot/Clipboard/commit/1dd83bd0) [`67adb86f`](https://github.com/muutot/Clipboard/commit/67adb86f)

### 隐私

- **结构化凭证拦截** — 敏感内容过滤覆盖结构化凭证；默认保持未升级的用户也会升级过滤规则 | [`ca8554b8`](https://github.com/muutot/Clipboard/commit/ca8554b8)

### 存储与导入

- **导入校验** — 导入的元数据先校验再落库；旧版资源引用在导入后保留 | [`1c26f08d`](https://github.com/muutot/Clipboard/commit/1c26f08d)
- **键盘记录定位** — 键盘配置定位导入的记录 ID 不再经 CSS 插值，特殊字符 ID 不失配 | [`4efe1a38`](https://github.com/muutot/Clipboard/commit/4efe1a38)

### 更新与 Markdown

- **语义化版本比较** — 更新检查保留完整语义化版本优先级，预发布/修订号比较不再错判 | [`08ab6247`](https://github.com/muutot/Clipboard/commit/08ab6247)
- **Markdown 阻断图片** — 预览中被沙箱阻断的远程图片给出说明，不发起远程请求 | [`39603748`](https://github.com/muutot/Clipboard/commit/39603748)

---

## 性能

- **复制提速** — 复制时复用已捕获的 SHA-256，文件读取移出 UI 线程 | [`1e74f3b3`](https://github.com/muutot/Clipboard/commit/1e74f3b3)

---

## 工程与发布

- **静态 VC 运行时** — Tauri 配置声明静态链接 VC 运行时 | [`30e8b8f3`](https://github.com/muutot/Clipboard/commit/30e8b8f3)
- **zstd 安全补丁** — 同步兼容的 zstd 安全补丁版本 | [`b4969dac`](https://github.com/muutot/Clipboard/commit/b4969dac)
- **技能目录迁移** — 全部 agent skills 迁入 `.agents/skills/` 供 DSH 自动发现 | [`764a2f28`](https://github.com/muutot/Clipboard/commit/764a2f28) [`9d3b8eaa`](https://github.com/muutot/Clipboard/commit/9d3b8eaa) [`491764cf`](https://github.com/muutot/Clipboard/commit/491764cf)
- **测试与文档** — 粘贴测试覆盖后端文本写入；i18n 区分本地资源主机与语言键；备份认证流格式原型与上限断言；文件保存能力多回退验证；路线图按审计与竞品报告重建 | [`82c4498e`](https://github.com/muutot/Clipboard/commit/82c4498e) [`67a1723a`](https://github.com/muutot/Clipboard/commit/67a1723a) [`e14343ae`](https://github.com/muutot/Clipboard/commit/e14343ae) [`b719572d`](https://github.com/muutot/Clipboard/commit/b719572d) [`f68b26d8`](https://github.com/muutot/Clipboard/commit/f68b26d8) [`258183f5`](https://github.com/muutot/Clipboard/commit/258183f5)

---

## 构建产物

- **Windows**: `Clipboard_1.8.4_x64-setup.exe`（SignPath 签名）、`Clipboard_1.8.4_x64_en-US.msi`
- **macOS**: `Clipboard_1.8.4_aarch64.dmg`
- **Linux**: `Clipboard_1.8.4_amd64.AppImage`（内嵌 zsync 更新信息）、`Clipboard_1.8.4_amd64.deb`、`Clipboard-1.8.4-1.x86_64.rpm`
