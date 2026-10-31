# Clipboard Desktop v1.8.2

> 网页来源识别与来源映射、详情面板代码编辑器（highlight.js 语法高亮 + 可选高亮主题）、详情内查找（Ctrl+F）、设置界面统一主题化，以及一轮并发与数据一致性加固
>
> Released: 2026-10-31

---

## 新功能

### 网页来源与来源映射

- **网页来源解析** — 解析 CF_HTML SourceURL 并暴露前台窗口标题，复制时写入来源元数据；重制内容时替换为最新来源 | [`af1bd6ad`](https://github.com/muutot/Clipboard/commit/af1bd6ad) [`35edb27a`](https://github.com/muutot/Clipboard/commit/35edb27a) [`652dafb9`](https://github.com/muutot/Clipboard/commit/652dafb9) [`e2cd0d03`](https://github.com/muutot/Clipboard/commit/e2cd0d03)
- **来源展示** — 卡片与详情面板显示网页来源，替代原始长标题 | [`07e61433`](https://github.com/muutot/Clipboard/commit/07e61433)
- **来源映射规则** — 新增设置面板：按域名 / 标题关键词将网页来源重命名为自定义名称，规则持久化并跨窗口生效 | [`49b84be9`](https://github.com/muutot/Clipboard/commit/49b84be9) [`41653c44`](https://github.com/muutot/Clipboard/commit/41653c44) [`928b9bdc`](https://github.com/muutot/Clipboard/commit/928b9bdc) [`8248cce9`](https://github.com/muutot/Clipboard/commit/8248cce9)

### 详情面板代码编辑器与高亮

- **highlight.js 语法高亮** — 手写分词器替换为 highlight.js，覆盖 17+ 常用语言，HTML/CSS/Shell 不再误判 | [`51bc22fb`](https://github.com/muutot/Clipboard/commit/51bc22fb)
- **代码高亮主题** — 主题设置新增高亮主题选择器：跟随应用主题 / VS Code Dark+ / One Dark / GitHub Light / Solarized Light，跨窗口实时同步 | [`1a2d06b8`](https://github.com/muutot/Clipboard/commit/1a2d06b8)
- **编辑体验** — 编辑时实时语法高亮，Ctrl+S 保存、Shift+Enter 行尾换行、自动换行开关、单行高亮；markdown 与纯文本编辑同用代码编辑器 | [`94b1f4fe`](https://github.com/muutot/Clipboard/commit/94b1f4fe) [`b0d937f9`](https://github.com/muutot/Clipboard/commit/b0d937f9) [`80d00e82`](https://github.com/muutot/Clipboard/commit/80d00e82) [`d8c7e336`](https://github.com/muutot/Clipboard/commit/d8c7e336) [`75e4e65a`](https://github.com/muutot/Clipboard/commit/75e4e65a)
- **语言手动切换** — 详情面板内按条目手动切换代码语言并持久化记忆；换行行号与行框对齐修复 | [`71bcb8d0`](https://github.com/muutot/Clipboard/commit/71bcb8d0) [`aae5c41d`](https://github.com/muutot/Clipboard/commit/aae5c41d) [`5d027ea7`](https://github.com/muutot/Clipboard/commit/5d027ea7) [`74d9952e`](https://github.com/muutot/Clipboard/commit/74d9952e) [`b0b199bd`](https://github.com/muutot/Clipboard/commit/b0b199bd)

### 查找与快捷键

- **详情内查找** — Ctrl+F 在详情面板内查找定位；收藏快捷键默认改绑 Ctrl+G，老用户已有绑定自动保留 | [`fb19e23b`](https://github.com/muutot/Clipboard/commit/fb19e23b) [`2e344323`](https://github.com/muutot/Clipboard/commit/2e344323) [`621ab2a2`](https://github.com/muutot/Clipboard/commit/621ab2a2) [`f6d0fdfc`](https://github.com/muutot/Clipboard/commit/f6d0fdfc)
- **开发者工具** — 关于页新增开关（默认关闭），开启后任意窗口按 F12 打开 WebView 控制台 | [`651372ae`](https://github.com/muutot/Clipboard/commit/651372ae) [`97fb174a`](https://github.com/muutot/Clipboard/commit/97fb174a)

---

## 设置与界面改进

- **主题化确认弹窗** — 新增统一 ConfirmDialog 组件，替换全部原生 `confirm()`，跟随主题且支持危险操作红标 | [`c4b94e3d`](https://github.com/muutot/Clipboard/commit/c4b94e3d)
- **设置搜索体验** — 搜索结果中点击侧栏分组即退出搜索并显示对应分区内容 | [`6e520fa8`](https://github.com/muutot/Clipboard/commit/6e520fa8)
- **详情面板布局** — 详情板固定占主窗口 75% 并随窗口联动缩放；标题编辑框与原标题完全对齐、编辑描边加粗；工具栏控件与标签按钮样式统一 | [`626f8890`](https://github.com/muutot/Clipboard/commit/626f8890) [`36a40fda`](https://github.com/muutot/Clipboard/commit/36a40fda) [`280b1a3f`](https://github.com/muutot/Clipboard/commit/280b1a3f) [`126fe8bd`](https://github.com/muutot/Clipboard/commit/126fe8bd) [`49a2e21a`](https://github.com/muutot/Clipboard/commit/49a2e21a) [`d4a5e79f`](https://github.com/muutot/Clipboard/commit/d4a5e79f) [`f1881e7a`](https://github.com/muutot/Clipboard/commit/f1881e7a)
- **主题适配收尾** — 图片全屏按钮与遮罩层配色全部随主题切换，浅色主题下不再出现硬编码深色 | [`3b62f129`](https://github.com/muutot/Clipboard/commit/3b62f129) [`e291ad99`](https://github.com/muutot/Clipboard/commit/e291ad99) [`0692a82d`](https://github.com/muutot/Clipboard/commit/0692a82d) [`6cf7ae46`](https://github.com/muutot/Clipboard/commit/6cf7ae46)
- **标签页状态修复** — 修复异步 DOM 依赖导致的标签页回跳预览问题 | [`6ca5bd5d`](https://github.com/muutot/Clipboard/commit/6ca5bd5d)

---

## 正确性加固

### 数据一致性

- **乐观回滚收敛** — 回滚范围限定在自身写入，失败写入不再冲掉已确认的修改；标题、收藏、元数据、批量删除等路径逐一加固 | [`8a0ff884`](https://github.com/muutot/Clipboard/commit/8a0ff884) [`7b4bdd40`](https://github.com/muutot/Clipboard/commit/7b4bdd40) [`31ffb958`](https://github.com/muutot/Clipboard/commit/31ffb958) [`3043998b`](https://github.com/muutot/Clipboard/commit/3043998b) [`ceaeb9e7`](https://github.com/muutot/Clipboard/commit/ceaeb9e7) [`6d910a4f`](https://github.com/muutot/Clipboard/commit/6d910a4f) [`4e2b3ac9`](https://github.com/muutot/Clipboard/commit/4e2b3ac9)
- **编辑器并发保护** — 异步编辑所有权守卫、延迟保存回复保留较新记录、异步菜单关闭前捕获编辑目标 | [`2cb6541e`](https://github.com/muutot/Clipboard/commit/2cb6541e) [`9e62ef56`](https://github.com/muutot/Clipboard/commit/9e62ef56) [`a8302a71`](https://github.com/muutot/Clipboard/commit/a8302a71) [`dfbfbd2f`](https://github.com/muutot/Clipboard/commit/dfbfbd2f)
- **标签系统健壮性** — 失败读取恢复、过期响应丢弃、注册表互斥与事件保留 | [`ea22cdec`](https://github.com/muutot/Clipboard/commit/ea22cdec) [`e8ee8488`](https://github.com/muutot/Clipboard/commit/e8ee8488) [`5c86d64f`](https://github.com/muutot/Clipboard/commit/5c86d64f) [`9409bb62`](https://github.com/muutot/Clipboard/commit/9409bb62) [`c7b45fe0`](https://github.com/muutot/Clipboard/commit/c7b45fe0) [`1e5a0b13`](https://github.com/muutot/Clipboard/commit/1e5a0b13) [`b6596039`](https://github.com/muutot/Clipboard/commit/b6596039)

### 同步与存储

- **同步协议加固** — 拒绝畸形对象清单与封包、GET 缺 ETag 时保留检查点 CAS、非常规端口逐个签名、S3 请求保留字面量 key、加密资源限制与设置对齐、轮换任务取消语义修正 | [`eea97d37`](https://github.com/muutot/Clipboard/commit/eea97d37) [`20f81b85`](https://github.com/muutot/Clipboard/commit/20f81b85) [`6e140567`](https://github.com/muutot/Clipboard/commit/6e140567) [`0c34fb09`](https://github.com/muutot/Clipboard/commit/0c34fb09) [`32649332`](https://github.com/muutot/Clipboard/commit/32649332) [`13e61818`](https://github.com/muutot/Clipboard/commit/13e61818) [`8b5ba0cd`](https://github.com/muutot/Clipboard/commit/8b5ba0cd) [`f148754d`](https://github.com/muutot/Clipboard/commit/f148754d) [`1388eff0`](https://github.com/muutot/Clipboard/commit/1388eff0)
- **存储安全** — 图标/资源缓存原子替换、清理与发布跨进程围栏、自定义根目录所有权复验、链接文件不被所有权标记修改 | [`b1fbb319`](https://github.com/muutot/Clipboard/commit/b1fbb319) [`0c085632`](https://github.com/muutot/Clipboard/commit/0c085632) [`bef805bc`](https://github.com/muutot/Clipboard/commit/bef805bc) [`6b322390`](https://github.com/muutot/Clipboard/commit/6b322390) [`f8903356`](https://github.com/muutot/Clipboard/commit/f8903356) [`e42ed736`](https://github.com/muutot/Clipboard/commit/e42ed736) [`338788e7`](https://github.com/muutot/Clipboard/commit/338788e7)
- **多实例与配置** — 跨实例拒绝过期配置/快捷键保存，键盘配置损坏时保留并恢复；数据库独立副本 fork 同步身份避免互相同步 | [`076d46ca`](https://github.com/muutot/Clipboard/commit/076d46ca) [`5698aa22`](https://github.com/muutot/Clipboard/commit/5698aa22) [`dc065658`](https://github.com/muutot/Clipboard/commit/dc065658) [`616f58fd`](https://github.com/muutot/Clipboard/commit/616f58fd) [`514de97f`](https://github.com/muutot/Clipboard/commit/514de97f)

---

## 工程与发布

- **CI 提速** — 发布构建与验证阶段并行执行，移除冗余 LTO 探针构建 | [`16bbb9a8`](https://github.com/muutot/Clipboard/commit/16bbb9a8)
- **Windows 安装包签名** — 支持 SignPath 签名（配置密钥后自动走签名审批流程） | [`29fb9fb4`](https://github.com/muutot/Clipboard/commit/29fb9fb4)
- **依赖与工具链** — 更新开发工具链安全补丁，适配新 clippy lint | [`6545e206`](https://github.com/muutot/Clipboard/commit/6545e206) [`6385429d`](https://github.com/muutot/Clipboard/commit/6385429d)
