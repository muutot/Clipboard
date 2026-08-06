# Clipboard Desktop v1.3.0

> 剪贴板桌面管理工具，Windows 原生剪贴板监控、全文搜索、OCR 识别、多引擎支持
>
> Released: 2026-08-06

---

## 版本说明

v1.3.0 聚焦「标签管理」与「图标自定义」：新增完整标签系统（标签注册表、管理面板、彩色标签、卡片 / 详情内联编辑、Ctrl+T 快速加标签、主窗口联动）；图标支持彩色模式与逐图标自定义颜色；剪贴板新增 RTF 富文本采集与粘贴、双击粘贴、悬停粘贴子菜单、卡片 #N 序号角标；更新源可切换 GitHub / GitCode，并支持导入 PPaste 备份。同时优化历史分组与筛选的后端分页、标签索引与来源图标查询性能，并修复导入、DIB 解码、自触发守卫、软删除复活等一系列健壮性问题。

## 标签系统

- **标签注册表与管理命令** — 新增 tag 领域模型、标签注册 / 管理后端命令 | [`833ec0dc`](https://github.com/muutot/Clipboard/commit/833ec0dc)
- **标签管理面板** — 设置中新增标签管理面板与彩色标签 chips | [`b51dde5f`](https://github.com/muutot/Clipboard/commit/b51dde5f) [`5f893c0c`](https://github.com/muutot/Clipboard/commit/5f893c0c)
- **标签编辑对话框** — 支持重命名与每个标签独立配色 | [`72cdee54`](https://github.com/muutot/Clipboard/commit/72cdee54)
- **条目标签** — 标签存储在 metadata_json，卡片与详情面板内联编辑 | [`dbf83608`](https://github.com/muutot/Clipboard/commit/dbf83608)
- **Ctrl+T 快速加标签** — 为选中条目快速添加标签 | [`1528b8c5`](https://github.com/muutot/Clipboard/commit/1528b8c5)
- **标签索引** — 通过派生 item_tags 联结表索引标签，检索性能优化 | [`ae7ce02d`](https://github.com/muutot/Clipboard/commit/ae7ce02d)

## 图标自定义

- **彩色图标模式** — 新增 color icon mode，设置中一键切换 | [`c74663ab`](https://github.com/muutot/Clipboard/commit/c74663ab)
- **逐图标颜色定制** — Appearance Icons 下每个图标可独立配色 | [`df82a025`](https://github.com/muutot/Clipboard/commit/df82a025)
- **图标缓存管理** — 图标缓存管理器支持按应用替换与去重视图 | [`9034b4c0`](https://github.com/muutot/Clipboard/commit/9034b4c0)
- **复选框着色修复** — 彩色图标模式下复选框对勾保持 currentColor | [`ea66ab43`](https://github.com/muutot/Clipboard/commit/ea66ab43)

## 剪贴板与粘贴

- **RTF 富文本** — 采集与粘贴 RTF 格式化内容 | [`11e93177`](https://github.com/muutot/Clipboard/commit/11e93177)
- **双击粘贴** — 双击卡片直接粘贴，可在设置中开关 | [`eec30868`](https://github.com/muutot/Clipboard/commit/eec30868)
- **悬停粘贴子菜单** — 右键菜单新增悬停粘贴子菜单，统一单菜单协调 | [`962cb40d`](https://github.com/muutot/Clipboard/commit/962cb40d)
- **#N 序号角标** — 每张卡片固定右列显示 #N 位置标记 | [`80efcaa9`](https://github.com/muutot/Clipboard/commit/80efcaa9)
- **净化粘贴统一** — 统一 clean-paste 文本键与粘贴路径，修复快速粘贴窗口还原 | [`4cdd2743`](https://github.com/muutot/Clipboard/commit/4cdd2743) [`56562b58`](https://github.com/muutot/Clipboard/commit/56562b58)

## 导入与更新

- **PPaste 备份导入** — 支持导入 PPaste .Pastebackup 备份，含大小上限与日期解析加固 | [`13e89afd`](https://github.com/muutot/Clipboard/commit/13e89afd) [`1e3bc782`](https://github.com/muutot/Clipboard/commit/1e3bc782)
- **导入健壮性** — 跳过非便携记录、处理 zip 反斜杠路径、缺失图片友好提示、避免重复计数、超限警告 | [`3b93c61a`](https://github.com/muutot/Clipboard/commit/3b93c61a) [`5607102b`](https://github.com/muutot/Clipboard/commit/5607102b) [`4bcf5bb1`](https://github.com/muutot/Clipboard/commit/4bcf5bb1) [`fcaf9915`](https://github.com/muutot/Clipboard/commit/fcaf9915) [`fd148f82`](https://github.com/muutot/Clipboard/commit/fd148f82)
- **可配置更新源** — 更新检查可在 GitHub / GitCode 之间切换 | [`1e8770af`](https://github.com/muutot/Clipboard/commit/1e8770af)
- **关于面板** — 显示程序安装位置 | [`7e683bf8`](https://github.com/muutot/Clipboard/commit/7e683bf8)

## 设置与界面

- **历史分组后端分页** — 分组与筛选轴改为后端分页，长列表更流畅 | [`384e1d66`](https://github.com/muutot/Clipboard/commit/384e1d66)
- **转移卡片现代化** — 按格式导入与 DatePicker | [`63ea73df`](https://github.com/muutot/Clipboard/commit/63ea73df)
- **快捷键设置对齐** — 键盘设置与实际实现的快捷键保持一致 | [`4fc71580`](https://github.com/muutot/Clipboard/commit/4fc71580)
- **箭头按键显示** — 快捷键绑定中箭头显示为 ↑↓←→ 字形，支持 Tab / Shift+Tab | [`6d2e430e`](https://github.com/muutot/Clipboard/commit/6d2e430e) [`d6b5ec71`](https://github.com/muutot/Clipboard/commit/d6b5ec71)
- **设置搜索精准定位** — 搜索结果可靠定位到目标设置卡片，路径与真实导航标签对齐 | [`73631ca9`](https://github.com/muutot/Clipboard/commit/73631ca9) [`007df845`](https://github.com/muutot/Clipboard/commit/007df845) [`2e84e8f5`](https://github.com/muutot/Clipboard/commit/2e84e8f5)
- **统一 Checkbox 组件** — 复选框视觉统一为共享组件 | [`f1411d27`](https://github.com/muutot/Clipboard/commit/f1411d27)

## 性能与健壮性

- **来源图标查询重写** — 去除相关子查询，图标加载更快 | [`d2626372`](https://github.com/muutot/Clipboard/commit/d2626372)
- **OCR 模型异步下载** — PP-OCR 模型下载改为异步文件 IO | [`c78d331e`](https://github.com/muutot/Clipboard/commit/c78d331e)
- **列表索引映射** — 非虚拟列表渲染改用索引映射，大批量渲染更高效 | [`4acb3c5b`](https://github.com/muutot/Clipboard/commit/4acb3c5b)
- **DIB 解码加固** — 对畸形 DIB 头做边界保护；CF_UNICODETEXT 终止符扫描受分配大小约束 | [`83966fd7`](https://github.com/muutot/Clipboard/commit/83966fd7) [`d4c4e8b6`](https://github.com/muutot/Clipboard/commit/d4c4e8b6)
- **自触发守卫** — 毒锁自恢复，注册守卫保证复制永不中断 | [`cf9743ef`](https://github.com/muutot/Clipboard/commit/cf9743ef) [`e9e95a06`](https://github.com/muutot/Clipboard/commit/e9e95a06)
- **软删除复活** — 内容重新复制时恢复已软删除的记录 | [`edd012af`](https://github.com/muutot/Clipboard/commit/edd012af)
- **共享资源保护** — 条目重命名不再连带重命名共享资源文件 | [`1aeb5277`](https://github.com/muutot/Clipboard/commit/1aeb5277)
- **搜索索引** — 应用索引变更后重载 reader；移除冗余 search_outbox_sequence 索引 | [`b2602b7c`](https://github.com/muutot/Clipboard/commit/b2602b7c) [`bd84568f`](https://github.com/muutot/Clipboard/commit/bd84568f)
- **设置深拷贝** — 默认值深拷贝，克隆不再共享可变容器 | [`ed137353`](https://github.com/muutot/Clipboard/commit/ed137353)
- **卡片高度估算** — 文本卡片高度估算与裁剪后预览对齐 | [`0a27444a`](https://github.com/muutot/Clipboard/commit/0a27444a)

## 修复

- **批量收藏切换** — 收藏组批量取消收藏时隐藏删除操作 | [`6be9a2df`](https://github.com/muutot/Clipboard/commit/6be9a2df)
- **查看器保存失败** — 保存失败时提示，而非打开原始文件 | [`eae37200`](https://github.com/muutot/Clipboard/commit/eae37200)
- **详情索引守卫** — 文件详情索引防陈旧选中 | [`ac7a2853`](https://github.com/muutot/Clipboard/commit/ac7a2853)
- **清空历史文案** — 补全清空历史的 Toast 翻译 | [`5544087d`](https://github.com/muutot/Clipboard/commit/5544087d)
- **DIB 测试平台门控** — dib_to_png 测试限定 Windows 目标 | [`e121afdb`](https://github.com/muutot/Clipboard/commit/e121afdb)

---

## 构建产物

- **Windows (x64)** — MSI + NSIS 安装包
- **macOS (Apple Silicon, arm64)** — DMG 安装包
- **Linux (x64)** — deb / rpm / AppImage
