# Clipboard Desktop v1.8.3

> 主题默认跟随系统亮暗外观、界面字体族设置，以及一轮覆盖设置面板、存储备份、同步、OCR 与本地 API 的大规模并发与生命周期加固
>
> Released: 2026-11-08

---

## 新功能

### 主题与外观

- **跟随系统亮暗主题** — 默认主题从硬编码深色改为跟随系统外观，未检测到系统偏好时回退亮色，系统切换时实时应用；新装用户不再出现「浅色系统显示深色界面」 | [`e6bc5656`](https://github.com/muutot/Clipboard/commit/e6bc5656)
- **界面字体族设置** — 设置面板新增 UI 字体选择，可在系统无衬线与等宽字体间切换，代码编辑器与预览不受影响 | [`fd09037f`](https://github.com/muutot/Clipboard/commit/fd09037f)

### 界面细节

- **搜索框自动清空** — 主窗口隐藏或失焦数秒后自动清空搜索框，避免下次打开残留过期过滤条件 | [`2e7d1668`](https://github.com/muutot/Clipboard/commit/2e7d1668)
- **浮窗空状态** — 浮窗空状态与主窗口空状态风格对齐 | [`ed861a11`](https://github.com/muutot/Clipboard/commit/ed861a11)
- **窗口透明度实时生效** — 透明度设置保存后立即应用到窗口，无需重开设置页 | [`52a5539a`](https://github.com/muutot/Clipboard/commit/52a5539a)

---

## 界面与样式收敛

- **设计令牌统一** — 操作按钮高度统一 30px；11.5px/12px 字号收敛到 `--font-size-small`；收藏星标色 `--favorite-color`；弹窗遮罩 `--modal-scrim` 与浮层阴影 `--shadow-float` 令牌化；卡片悬停边框随主题 | [`b32a51f5`](https://github.com/muutot/Clipboard/commit/b32a51f5) [`e819edae`](https://github.com/muutot/Clipboard/commit/e819edae) [`78d256bb`](https://github.com/muutot/Clipboard/commit/78d256bb) [`3e5dd002`](https://github.com/muutot/Clipboard/commit/3e5dd002) [`80b01055`](https://github.com/muutot/Clipboard/commit/80b01055) [`967dd75e`](https://github.com/muutot/Clipboard/commit/967dd75e) [`bc23e9a5`](https://github.com/muutot/Clipboard/commit/bc23e9a5)
- **过滤图标配色重排** — 工具栏过滤图标颜色按文本/链接/图片/文件/收藏语义对齐 | [`a379e629`](https://github.com/muutot/Clipboard/commit/a379e629)
- **主题预设清理** — 切换主题时正确清理已修改预设状态 | [`303f9252`](https://github.com/muutot/Clipboard/commit/303f9252)

---

## 正确性加固

### 设置面板可靠性

本版本对全部设置面板做了系统的并发与生命周期收敛，消除快速操作下的状态错乱：

- **保存串行化** — 键盘重置与绑定修改、忽略应用、OCR 配置、历史上限、同步配置等并发保存改为串行执行，避免后写覆盖先写 | [`03f58f22`](https://github.com/muutot/Clipboard/commit/03f58f22) [`21f2f689`](https://github.com/muutot/Clipboard/commit/21f2f689) [`38d63fec`](https://github.com/muutot/Clipboard/commit/38d63fec) [`ef387012`](https://github.com/muutot/Clipboard/commit/ef387012) [`6959c6f6`](https://github.com/muutot/Clipboard/commit/6959c6f6)
- **过期响应丢弃** — 面板切换后，旧面板的 OCR/同步/来源映射/图标缓存/备份/传输/存储工具等异步结果一律丢弃，不再污染新面板 | [`72b75e27`](https://github.com/muutot/Clipboard/commit/72b75e27) [`bc636253`](https://github.com/muutot/Clipboard/commit/bc636253) [`8cfad5d5`](https://github.com/muutot/Clipboard/commit/8cfad5d5) [`5b8c54a2`](https://github.com/muutot/Clipboard/commit/5b8c54a2) [`5733ed81`](https://github.com/muutot/Clipboard/commit/5733ed81) [`bd5dbf01`](https://github.com/muutot/Clipboard/commit/bd5dbf01) [`30cb6cd5`](https://github.com/muutot/Clipboard/commit/30cb6cd5) [`b00c9e26`](https://github.com/muutot/Clipboard/commit/b00c9e26) [`3dc8e229`](https://github.com/muutot/Clipboard/commit/3dc8e229)
- **草稿保护** — 保存失败时保留用户当前输入；较新的草稿不再被旧快照覆盖（忽略应用、存储目录、同步凭据） | [`4b80713c`](https://github.com/muutot/Clipboard/commit/4b80713c) [`6a0daf2e`](https://github.com/muutot/Clipboard/commit/6a0daf2e) [`ffd8653a`](https://github.com/muutot/Clipboard/commit/ffd8653a)
- **失败门控** — 来源映射/标签规则加载失败后阻止覆盖写入；历史上限在设置成功水合前不落盘 | [`ad7d7c2e`](https://github.com/muutot/Clipboard/commit/ad7d7c2e) [`602329bd`](https://github.com/muutot/Clipboard/commit/602329bd) [`8f6427f8`](https://github.com/muutot/Clipboard/commit/8f6427f8) [`f980de44`](https://github.com/muutot/Clipboard/commit/f980de44)
- **状态快照与刷新** — 存储统计停止冗余刷新循环、始终保留最新快照；来源映射刷新后正确水合；过期确认弹窗自动取消 | [`15308768`](https://github.com/muutot/Clipboard/commit/15308768) [`9bdbcf23`](https://github.com/muutot/Clipboard/commit/9bdbcf23) [`18a1869d`](https://github.com/muutot/Clipboard/commit/18a1869d) [`b297779b`](https://github.com/muutot/Clipboard/commit/b297779b) [`b1318a0f`](https://github.com/muutot/Clipboard/commit/b1318a0f)
- **反馈本地化** — 数据库修复与快捷键冲突提示接入 i18n | [`2578efb8`](https://github.com/muutot/Clipboard/commit/2578efb8) [`aef58b06`](https://github.com/muutot/Clipboard/commit/aef58b06)

### 详情面板与预览

- **查找定位** — 查找可跨越内联预览标记命中短语；Unicode 查询按原始文本偏移匹配；界面标签不再混入预览搜索结果；查看器查找计数在高亮交叠时不再错乱 | [`12379dd6`](https://github.com/muutot/Clipboard/commit/12379dd6) [`783e2aca`](https://github.com/muutot/Clipboard/commit/783e2aca) [`4ab0f64c`](https://github.com/muutot/Clipboard/commit/4ab0f64c) [`5f3642a2`](https://github.com/muutot/Clipboard/commit/5f3642a2)
- **编辑器** — 所见即所得：选中文本不再包含显示标记，行尾行号标记不影响保存内容，保存文本编辑时保留当前元数据；截断代码内容在预览中明确披露 | [`43fed810`](https://github.com/muutot/Clipboard/commit/43fed810) [`ee109119`](https://github.com/muutot/Clipboard/commit/ee109119) [`f3f47b62`](https://github.com/muutot/Clipboard/commit/f3f47b62) [`b5fe0542`](https://github.com/muutot/Clipboard/commit/b5fe0542)

### 存储与备份原子性

- **原子写入** — 搜索索引、键盘配置、应用设置、图标缓存、资源保存统一改为「唯一临时目录 → 校验 → 原子发布」，临时名受路径长度约束 | [`fce73db9`](https://github.com/muutot/Clipboard/commit/fce73db9) [`4e11e999`](https://github.com/muutot/Clipboard/commit/4e11e999) [`5311761f`](https://github.com/muutot/Clipboard/commit/5311761f) [`44180d2a`](https://github.com/muutot/Clipboard/commit/44180d2a) [`bd0be2fb`](https://github.com/muutot/Clipboard/commit/bd0be2fb) [`784bef97`](https://github.com/muutot/Clipboard/commit/784bef97) [`1d94b9cf`](https://github.com/muutot/Clipboard/commit/1d94b9cf)
- **备份与恢复** — 恢复时将只读备份展开为可写副本；恢复与备份轮转代际协同；拒绝在校验不可用时还原；重叠轮转去重；旧格式文件清单资源保留 | [`c43b1540`](https://github.com/muutot/Clipboard/commit/c43b1540) [`5c093753`](https://github.com/muutot/Clipboard/commit/5c093753) [`df497b7a`](https://github.com/muutot/Clipboard/commit/df497b7a) [`14bb458c`](https://github.com/muutot/Clipboard/commit/14bb458c) [`d9e4a1f4`](https://github.com/muutot/Clipboard/commit/d9e4a1f4) [`40c50575`](https://github.com/muutot/Clipboard/commit/40c50575) [`67963e5b`](https://github.com/muutot/Clipboard/commit/67963e5b)
- **路径迁移与 schema** — 迁移时保留用户元数据；schema 版本在写入事务内复核；Windows 替换路径校验与规范化；历史策略请求原子持久化 | [`8b0bf508`](https://github.com/muutot/Clipboard/commit/8b0bf508) [`8444e3ae`](https://github.com/muutot/Clipboard/commit/8444e3ae) [`bbd55122`](https://github.com/muutot/Clipboard/commit/bbd55122) [`21b53589`](https://github.com/muutot/Clipboard/commit/21b53589)
- **记录一致性** — 另存为不再与去重竞争、继承图标全程受保护；重命名持久化保留并发修改；资源别名与失败目标如实保留 | [`b81264ad`](https://github.com/muutot/Clipboard/commit/b81264ad) [`8efa0e16`](https://github.com/muutot/Clipboard/commit/8efa0e16) [`da7d6f7a`](https://github.com/muutot/Clipboard/commit/da7d6f7a) [`d6a8819a`](https://github.com/muutot/Clipboard/commit/d6a8819a)

### 同步

- 同步配置替换前先保留延迟上传的媒体；命名空间轮换时保留隐私注册；状态判定前预占 SQLite 写锁；设备身份缺失时串行初始化 | [`431542e3`](https://github.com/muutot/Clipboard/commit/431542e3) [`37904b69`](https://github.com/muutot/Clipboard/commit/37904b69) [`88c96cd1`](https://github.com/muutot/Clipboard/commit/88c96cd1) [`058e1ff9`](https://github.com/muutot/Clipboard/commit/058e1ff9)

### OCR

- 模型安装以目录租约串行化；模型摘要 sidecar 原子发布；下载量按锁定资产大小钳制；任务轮询期间持续刷新状态并保留最新全量快照 | [`8394448b`](https://github.com/muutot/Clipboard/commit/8394448b) [`64c676da`](https://github.com/muutot/Clipboard/commit/64c676da) [`83d0c1ca`](https://github.com/muutot/Clipboard/commit/83d0c1ca) [`d3b88c34`](https://github.com/muutot/Clipboard/commit/d3b88c34) [`a3f88404`](https://github.com/muutot/Clipboard/commit/a3f88404)

### 本地 API / CLI / 更新

- **API 加固** — 端口发布失败时停止监听器；拒绝非法 UTF-8 与歧义请求体封帧；表单编码查询空格正确解码；粘贴返回规范化落库记录；凭据写入前保护 token 文件，显式访问授权收敛 | [`4ab013b8`](https://github.com/muutot/Clipboard/commit/4ab013b8) [`eceb00b2`](https://github.com/muutot/Clipboard/commit/eceb00b2) [`514872a4`](https://github.com/muutot/Clipboard/commit/514872a4) [`31b98dbd`](https://github.com/muutot/Clipboard/commit/31b98dbd) [`d41afcfe`](https://github.com/muutot/Clipboard/commit/d41afcfe) [`6aba61c3`](https://github.com/muutot/Clipboard/commit/6aba61c3) [`123762ed`](https://github.com/muutot/Clipboard/commit/123762ed)
- **更新检查** — 发布说明与最新版本检查结果分离；响应体解析前限长；断点续传前校验资产身份；AppImage 发布内嵌 zsync 更新信息 | [`492e3ef4`](https://github.com/muutot/Clipboard/commit/492e3ef4) [`e2ef7224`](https://github.com/muutot/Clipboard/commit/e2ef7224) [`3117e103`](https://github.com/muutot/Clipboard/commit/3117e103) [`38747d16`](https://github.com/muutot/Clipboard/commit/38747d16)
- **CLI / 导入导出** — CLI 剪贴板辅助读取加上限；导出纯文本保留记录边界换行；导入隔离临时 PPaste 数据库、诊断失败不丢已提交结果、容量告警如实上报 | [`91956d7e`](https://github.com/muutot/Clipboard/commit/91956d7e) [`2d71b9b6`](https://github.com/muutot/Clipboard/commit/2d71b9b6) [`c62ec16f`](https://github.com/muutot/Clipboard/commit/c62ec16f) [`2a69572d`](https://github.com/muutot/Clipboard/commit/2a69572d) [`a556d9e3`](https://github.com/muutot/Clipboard/commit/a556d9e3) [`d4c415c0`](https://github.com/muutot/Clipboard/commit/d4c415c0)

### 剪贴板与杂项

- 失败的剪贴板写入只取消自身注册；托盘复制保留文本前后空白；超长可执行路径正确读取；图标缓存清理覆盖非 PNG 条目；PPaste 维度探测不再解码图片；窗口销毁后历史停止读取；浮窗监听注册后刷新历史；搜索手动重建完成正确失效缓存；快捷键保存后同步到主窗口；测试夹具上限校验改用小载荷，不再依赖慢速环境的管道吞吐；模型下载先落盘再报进度，保证已下载字节数对磁盘可见 | [`135f35c1`](https://github.com/muutot/Clipboard/commit/135f35c1) [`dc57d5d9`](https://github.com/muutot/Clipboard/commit/dc57d5d9) [`3c5a666e`](https://github.com/muutot/Clipboard/commit/3c5a666e) [`6fd803d2`](https://github.com/muutot/Clipboard/commit/6fd803d2) [`c749e11e`](https://github.com/muutot/Clipboard/commit/c749e11e) [`68045a59`](https://github.com/muutot/Clipboard/commit/68045a59) [`90af4d96`](https://github.com/muutot/Clipboard/commit/90af4d96) [`fd4769ad`](https://github.com/muutot/Clipboard/commit/fd4769ad) [`e5dfd29c`](https://github.com/muutot/Clipboard/commit/e5dfd29c) [`d862c515`](https://github.com/muutot/Clipboard/commit/d862c515) [`c3fc4f6d`](https://github.com/muutot/Clipboard/commit/c3fc4f6d)

---

## 工程与发布

- **CI** — S3 冒烟测试要求精确计数；API 通知回复按内容长度读取；ACL 解码改用定长数组；OCR 下载夹具改用阻塞 socket；首绘脚本经 Vite 加载测试 | [`7351b536`](https://github.com/muutot/Clipboard/commit/7351b536) [`33ddebf7`](https://github.com/muutot/Clipboard/commit/33ddebf7) [`8bfef701`](https://github.com/muutot/Clipboard/commit/8bfef701) [`39a978ab`](https://github.com/muutot/Clipboard/commit/39a978ab) [`d16f8fb8`](https://github.com/muutot/Clipboard/commit/d16f8fb8)
- **测试与文档** — 前台窗口测试模块归位、主题断言收窄、OCR 生命周期 mock 类型修复；同步取消契约与忽略测试清单文档修正 | [`f7b8781b`](https://github.com/muutot/Clipboard/commit/f7b8781b) [`4940f832`](https://github.com/muutot/Clipboard/commit/4940f832) [`cfb1416e`](https://github.com/muutot/Clipboard/commit/cfb1416e) [`0ac37a78`](https://github.com/muutot/Clipboard/commit/0ac37a78) [`4d9d1232`](https://github.com/muutot/Clipboard/commit/4d9d1232)

---

## 构建产物

- **Windows**: `Clipboard_1.8.3_x64-setup.exe`（SignPath 签名）、`Clipboard_1.8.3_x64_en-US.msi`
- **macOS**: `Clipboard_1.8.3_aarch64.dmg`
- **Linux**: `Clipboard_1.8.3_amd64.AppImage`（内嵌 zsync 更新信息）、`Clipboard_1.8.3_amd64.deb`、`Clipboard-1.8.3-1.x86_64.rpm`
