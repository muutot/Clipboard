# Clipboard Desktop v1.5.2

> macOS 构建修复、密钥链存储与卡片高度打磨
>
> Released: 2026-09-10

---

## macOS 构建与性能修复

- **构建修复** — `libc::mach_task_self` 已废弃，改用 `mach2` trap，修复 `-D warnings` 下 macOS 编译失败 | [`ac5241f`](https://github.com/muutot/Clipboard/commit/ac5241fad6f078991a3fd852f2c19e26bcc71cd5)
- **内存采样** — 经 `task_info`（`MACH_TASK_BASIC_INFO`）进程内查询 RSS，失败时回退到 `ps`，替代每次快照 fork 与 CSV 解析 | [`c1a8e0d`](https://github.com/muutot/Clipboard/commit/c1a8e0da3bc95c519884249938c0eeb7bb01c4f7)

---

## 安全与隐私

- **密钥链存储** — 同步密钥在 macOS 走钥匙串、Linux 走 Secret Service，统一经平台 secret-store 门面路由 | [`170765c`](https://github.com/muutot/Clipboard/commit/170765c6b6b68078063074f559577b41178fbd21) [`2e720ca`](https://github.com/muutot/Clipboard/commit/2e720ca04e88ea48493f43b6717ea895278f2212)
- **审计日志脱敏** — 审计日志打码，剪贴板原文不再输出到控制台 | [`9e996f6`](https://github.com/muutot/Clipboard/commit/9e996f6b9c7e8562cbf7171e7354bcf5a7372ebe)
- **图标来源校验** — 替换图标仅接受真实图片输入 | [`bc0eb8c`](https://github.com/muutot/Clipboard/commit/bc0eb8c4c48e5b9ca06e3d7e35ad3196333ece71)

---

## 设置与主题打磨

- **卡片高度** — 紧凑布局改为常开默认并移除开关；卡片高度设置统一为内容高度+内边距展开，自定义标题卡片接入 `cardCustomTitleHeight` 滑条 | [`a427b85`](https://github.com/muutot/Clipboard/commit/a427b85057451fa303b46f4c8352bcd7eded12cf) [`7aef5e9`](https://github.com/muutot/Clipboard/commit/7aef5e93a744e0ab15e04d578889822237306228) [`b44e086`](https://github.com/muutot/Clipboard/commit/b44e0863cf473d12bbac53a1c599762adb2ba07b) [`589395d`](https://github.com/muutot/Clipboard/commit/589395d68870dd4aa8b90852fffd0667aa6479c4)
- **主题** — 内置主题显示只读颜色列表，仅自定义主题可编辑；应用 prettier 格式化 | [`6fcd204`](https://github.com/muutot/Clipboard/commit/6fcd2049e2cb40ef371d189334514334abf013d8) [`3524852`](https://github.com/muutot/Clipboard/commit/35248520e0dd263765d29610b51a2a099457d57b)
- **主界面** — 筛选按钮内边距、搜索头垂直内边距、工具栏分组间隙与筛选下拉右置分组等细节对齐 | [`f5ec4c5`](https://github.com/muutot/Clipboard/commit/f5ec4c562a5aca667d47deb53c1ce22709766b04) [`08a2ee2`](https://github.com/muutot/Clipboard/commit/08a2ee2e2b7bce757da73289d83aaf0c33eb669a) [`0548502`](https://github.com/muutot/Clipboard/commit/0548502bd4f52f0b518680f2befe3cb5cd281398) [`f42eb8f`](https://github.com/muutot/Clipboard/commit/f42eb8f1b3421ad38459691ab3b6c82eb1107425) [`67841ba`](https://github.com/muutot/Clipboard/commit/67841ba6585c5db60ddddd8726a0a3023123d73f)

---

## 工程与文档

- **平台重构** — X11/Wayland 共用 uri-list 解析器；移除无用 `TEXT_HEIGHT`/`IMAGE_HEIGHT` 常量 | [`3019868`](https://github.com/muutot/Clipboard/commit/30198682f93c5b19f46ed74fa4cb6f587d459198) [`37c1f87`](https://github.com/muutot/Clipboard/commit/37c1f87ec423cd628b53c0b36d00eb6779e4fd16)
- **路线图** — 落定 1.5.2–1.6 已验证路线，MinIO/SBOM 延期并注明原因；提交信息强制英文 | [`b1ee17f`](https://github.com/muutot/Clipboard/commit/b1ee17fd4b69d90cb3579492b96c232efb986e68) [`89df404`](https://github.com/muutot/Clipboard/commit/89df404650118a58fe5983c0f8f30315599bf4c8) [`9fe097e`](https://github.com/muutot/Clipboard/commit/9fe097e9562391cfa8ba0123b5674b6120074ccd) [`0c32143`](https://github.com/muutot/Clipboard/commit/0c3214396e5de2635e2d5004d29fdd0b24baba87)

---

## 构建产物

- **MSI 安装包**: `Clipboard_1.5.2_x64_en-US.msi`
- **NSIS 安装包**: `Clipboard_1.5.2_x64-setup.exe`
