# Clipboard Desktop v1.5.1

> 增量打磨：分组标签显示、同步链路与设置面板的细节修复
>
> Released: 2026-09-01

---

## 分组与紧凑模式

- **分组标签显示模式** — 新增图标+文字/仅图标/仅文字三档切换，适配紧凑与常规布局 | [`6e37bf2`](https://github.com/muutot/Clipboard/commit/6e37bf23)
- **紧凑模式默认开启并收紧参数** — 默认启用紧凑布局，微调卡片与行高指标 | [`e720287`](https://github.com/muutot/Clipboard/commit/e7202879) [`b6b5d70`](https://github.com/muutot/Clipboard/commit/b6b5d708)

---

## 同步与设置重构

- **同步页面重构** — S3 页以“测试连接”为首项、移除外层卡片与 Advanced 标题，按 cloud / advanced / s3 重排为 cloud+S3 二级页 | [`a024de9`](https://github.com/muutot/Clipboard/commit/a024de97) [`e9f0cab`](https://github.com/muutot/Clipboard/commit/e9f0cab2) [`7fe056a`](https://github.com/muutot/Clipboard/commit/7fe056ae) [`d1716af`](https://github.com/muutot/Clipboard/commit/d1716afe)
- **设置面板惰性化与入口收敛** — OCR/存储路径/传输/同步/S3 等抽取为独立惰性面板，统一经 `LAZY_PANEL_DESCRIPTORS` 描述表分发；S3 同步配置、传输导入导出、存储路径管理分别由 `SyncPanel`/`TransferPanel`/`StoragePathsPanel` 承载 | [`1a4a6e5`](https://github.com/muutot/Clipboard/commit/1a4a6e5f) [`a5d9772`](https://github.com/muutot/Clipboard/commit/a5d9772b) [`31e6adb`](https://github.com/muutot/Clipboard/commit/31e6adbd) [`c7d6014`](https://github.com/muutot/Clipboard/commit/c7d60141)
- **面包屑与导航** — 面包屑完全由导航元数据推导，多标签组统一二级路径；修复分组切换与存储状态空值访问 | [`af7b99d`](https://github.com/muutot/Clipboard/commit/af7b99d7) [`22a883b`](https://github.com/muutot/Clipboard/commit/22a883bb) [`12c233d`](https://github.com/muutot/Clipboard/commit/12c233dd) [`66f4c05`](https://github.com/muutot/Clipboard/commit/66f4c054)

---

## 样式与交互细节

- **按钮居中与工具栏间距** — 图标+文字按钮在 5 处统一改为 `inline-flex + justify-center + line-height:1` 水平垂直居中；移除工具栏按钮组间 `3px` 间隙 | [`554a35a`](https://github.com/muutot/Clipboard/commit/554a35a3) [`5ec8e44`](https://github.com/muutot/Clipboard/commit/5ec8e442)
- **设置条目 28px 规范化** — 统一入口按钮/下拉至固定 28px 高度，恢复资源路径网格输入的共享控件样式 | [`60b80b4`](https://github.com/muutot/Clipboard/commit/60b80b42) [`71e5aa2`](https://github.com/muutot/Clipboard/commit/71e5aa29)
- **设置视觉打磨** — 补齐 `entry-textarea`/`entry-actions` 共享规则、资源路径与动作行间距、关闭按钮图标化及同步行图标等 | [`338233c`](https://github.com/muutot/Clipboard/commit/338233cf) [`0800776`](https://github.com/muutot/Clipboard/commit/08007763) [`7f28870`](https://github.com/muutot/Clipboard/commit/7f28870a) [`2994cb5`](https://github.com/muutot/Clipboard/commit/2994cb54)
- **同步反馈** — S3 测试连接结果经 toast 展示并防标签抖动；未选 S3 时禁用测试并给出文案提示 | [`0b4b08c`](https://github.com/muutot/Clipboard/commit/0b4b08c1) [`a0ae974`](https://github.com/muutot/Clipboard/commit/a0ae9747) [`e3f9882`](https://github.com/muutot/Clipboard/commit/e3f98821)

---

## 搜索、OCR 与稳定性

- **搜索** — 标题/类型排序支持升序；标题排序与 `cycleFilter` 守卫、历史筛选、搜索缓存维护等抽取为独立工具并补 Vitest 覆盖 | [`417896d`](https://github.com/muutot/Clipboard/commit/417896db) [`6661456`](https://github.com/muutot/Clipboard/commit/6661456d) [`ac11e58`](https://github.com/muutot/Clipboard/commit/ac11e58b) [`990906a`](https://github.com/muutot/Clipboard/commit/990906a8)
- **OCR** — 回退引擎使用 `tesseract_languages` 配置而非硬编码 `chi_sim`；检测滑条迁移至 `SliderEntry` | [`d92a409`](https://github.com/muutot/Clipboard/commit/d92a4098) [`498d031`](https://github.com/muutot/Clipboard/commit/498d0311) [`fd06ac3`](https://github.com/muutot/Clipboard/commit/fd06ac3b)
- **前端一致性** — 修复注释与删除占位符乱码、批量删除结果重命名残留、窗口边界持久化与搜索历史帮助函数等 | [`b3bba33`](https://github.com/muutot/Clipboard/commit/b3bba33e) [`313a8b0`](https://github.com/muutot/Clipboard/commit/313a8b03) [`666c2f5`](https://github.com/muutot/Clipboard/commit/666c2f51) [`8f13e4c`](https://github.com/muutot/Clipboard/commit/8f13e4c2)

---

## 工程与依赖

- **依赖升级** — `zip 8`、`RustCrypto 0.11/0.13 + rand 0.10`、`oar-ocr 0.9.2 / ort rc.13 / window-vibrancy 0.8`；移除 `walkdir`，补 `lockfile` | [`f5cde22`](https://github.com/muutot/Clipboard/commit/f5cde221) [`eb81336`](https://github.com/muutot/Clipboard/commit/eb813364) [`942af68`](https://github.com/muutot/Clipboard/commit/942af689) [`4f3d9b5`](https://github.com/muutot/Clipboard/commit/4f3d9b5c)
- **测试与工具抽取** — 快捷键目标检测、日期查询边界、卡片高度估算及批删规划/回滚快照等补单测 | [`47cd5af`](https://github.com/muutot/Clipboard/commit/47cd5afd) [`f866f40`](https://github.com/muutot/Clipboard/commit/f866f408) [`696959c`](https://github.com/muutot/Clipboard/commit/696959c1) [`88f3443`](https://github.com/muutot/Clipboard/commit/88f34430)

---

## 构建产物

- **MSI 安装包**: `Clipboard_1.5.1_x64_en-US.msi`
- **NSIS 安装包**: `Clipboard_1.5.1_x64-setup.exe`
