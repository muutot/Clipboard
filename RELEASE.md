# Clipboard Desktop v1.7.1

> 标签独立分页、导入/导出设置重排、搜索键盘导航与悬浮面板定位修复
>
> Released: 2026-09-24

---

## 新功能

### 详情面板

- **标签独立标签页** — 标签移至详情面板独立 tab，按分组渲染、每行一个标签，输入框样式与共享控件对齐 | [`efbe20f`](https://github.com/muutot/Clipboard/commit/efbe20f) [`3a085e3`](https://github.com/muutot/Clipboard/commit/3a085e3) [`5e76598`](https://github.com/muutot/Clipboard/commit/5e76598)

### 搜索

- **列表输入即聚焦搜索框** — 在列表上直接键入即可聚焦搜索框，无需先点击 | [`4f14f35`](https://github.com/muutot/Clipboard/commit/4f14f35)
- **无内联提示时方向键进入列表** — 搜索框有焦点且无内联提示、无建议列表时，上/下方向键移动列表选中并聚焦卡片；左/右键仍移动光标 | [`570cf32`](https://github.com/muutot/Clipboard/commit/570cf32)

### 快捷键

- **切换分组仅保留 Tab / Shift+Tab** — 移除切换分组默认绑定中的左右方向键，默认值改为 `Tab` / `Shift+Tab` | [`3bf789c`](https://github.com/muutot/Clipboard/commit/3bf789c)

---

## 设置界面

### 导入 / 导出

- **拆分为独立设置卡片** — 导入与导出各自成为独立卡片 | [`f39d3fe`](https://github.com/muutot/Clipboard/commit/f39d3fe)
- **行布局重排** — 导入在上、导出在下，堆叠行消除横向溢出，分隔线移到导出行与选项之上 | [`23c0f17`](https://github.com/muutot/Clipboard/commit/23c0f17) [`d56b38d`](https://github.com/muutot/Clipboard/commit/d56b38d) [`b3ee544`](https://github.com/muutot/Clipboard/commit/b3ee544) [`9d6e537`](https://github.com/muutot/Clipboard/commit/9d6e537)
- **传输行对齐与格式选择** — 传输标题在左、控件在右同一行；格式选择收窄至半宽并与按钮贴合，去掉冗余标签，导出说明精简、导出动作右对齐 | [`22c80d3`](https://github.com/muutot/Clipboard/commit/22c80d3) [`905bf57`](https://github.com/muutot/Clipboard/commit/905bf57) [`489b83b`](https://github.com/muutot/Clipboard/commit/489b83b) [`86ee332`](https://github.com/muutot/Clipboard/commit/86ee332) [`353e109`](https://github.com/muutot/Clipboard/commit/353e109)

---

## 修复

- **悬浮面板定位** — 依据真实外侧尺寸定位，确保面板始终落于工作区内 | [`4f8f9e8`](https://github.com/muutot/Clipboard/commit/4f8f9e8)
- **卡片键盘焦点** — 点击后保持键盘焦点，Space 可正常打开详情 | [`e790e67`](https://github.com/muutot/Clipboard/commit/e790e67)

---

## 构建与发布

- **GitCode 同步拆为独立手动工作流** — 同步逻辑从 Release 工作流抽出为 `.github/workflows/sync-gitcode.yml`，发布后手动触发，不再随 Release 自动执行 | [`ddc615b`](https://github.com/muutot/Clipboard/commit/ddc615b)

---

## 构建产物

- **MSI 安装包**: `Clipboard_1.7.1_x64_en-US.msi`
- **NSIS 安装包**: `Clipboard_1.7.1_x64-setup.exe`
