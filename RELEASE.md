# Clipboard Desktop v1.8.0

> 跨平台全局快捷键与快捷粘贴、命名搜索视图、条件自动标签、原生透明度特效，以及一批隐私与主线程阻塞修复
>
> Released: 2026-10-11

---

## 新功能

### 跨平台能力

- **跨平台全局快捷键** — macOS（Carbon Event Manager）、Linux X11（XGrabKey）与 Wayland（GlobalShortcuts portal）现在注册真实的全局快捷键，不再是平台桩 | [`55eaf9a`](https://github.com/muutot/Clipboard/commit/55eaf9a9) [`dd0fd22`](https://github.com/muutot/Clipboard/commit/dd0fd222)
- **跨平台快捷粘贴** — macOS/X11 目标应用恢复与焦点校验，粘贴注入前确认前置应用与按住的修饰键，拒绝过期目标 | [`f22bad8`](https://github.com/muutot/Clipboard/commit/f22bad89)
- **macOS RTF 原生捕获** — 剪贴板监控读取 RTF 格式，富文本来源不再只落纯文本 | [`17f1c54`](https://github.com/muutot/Clipboard/commit/17f1c547)
- **已运行实例唤醒** — 二次启动通过带鉴权的本地 IPC 唤醒既有实例，而不是再起一个进程 | [`f6ae71b`](https://github.com/muutot/Clipboard/commit/f6ae71bb)
- **原生窗口透明与背景特效** — 窗口不透明度与亚克力/云母等平台背景特效统一应用，macOS 映射到原生 Vibrancy 材质 | [`c664a5c`](https://github.com/muutot/Clipboard/commit/c664a5c8)
- **macOS 辅助功能真实探测** — `platform_info` 调用 `AXIsProcessTrusted` 报告真实权限状态，不再无条件报告权限缺失 | [`68832a8`](https://github.com/muutot/Clipboard/commit/68832a8e)

### 搜索

- **命名搜索视图** — 保存搜索条件为命名视图，随时恢复 | [`fef37e7`](https://github.com/muutot/Clipboard/commit/fef37e7e)
- **全量历史过滤** — 过滤条件先作用于全量历史再套候选上限，不再只在当前候选集内过滤 | [`3e2460d`](https://github.com/muutot/Clipboard/commit/3e2460db)

### 标签与 OCR

- **条件自动标签** — 预览自动标签规则的命中范围后应用，批量打标在数据库锁外匹配历史 | [`a22bf16`](https://github.com/muutot/Clipboard/commit/a22bf16a) [`6db0150`](https://github.com/muutot/Clipboard/commit/6db0150c)
- **OCR 文本块选择与图像匹配高亮** — 识别结果按文本块选择复制，图像内匹配位置高亮 | [`2db35f6`](https://github.com/muutot/Clipboard/commit/2db35f65)

### 同步与备份

- **协同取消** — 同步引擎工作与进行中的 S3 传输可协作取消，退出不再等待卡死的传输 | [`cbfc3c9`](https://github.com/muutot/Clipboard/commit/cbfc3c93)
- **便携资源包备份** — 备份包校验后可跨资源根目录还原 | [`55c271f`](https://github.com/muutot/Clipboard/commit/55c271f2) [`a60c081`](https://github.com/muutot/Clipboard/commit/a60c081c)

### 设置与界面

- **高优先级自定义 CSS** — 用户自定义 CSS 以高优先级注入，可靠覆盖内置样式 | [`d16342d`](https://github.com/muutot/Clipboard/commit/d16342d6)
- **后台任务进度与取消** — 长时间后台操作显示进度并可取消 | [`2390c1f`](https://github.com/muutot/Clipboard/commit/2390c1f3)
- **安装器卸载清理选项** — 卸载时可选择清理用户数据、配置与资源目录，保留现有配置的重装不再误删存储 | [`5355b71`](https://github.com/muutot/Clipboard/commit/5355b71e) [`3afe0b4`](https://github.com/muutot/Clipboard/commit/3afe0b46) [`f546d4e`](https://github.com/muutot/Clipboard/commit/f546d4ee)

---

## 修复

### 隐私与安全

- **CLI 与本地 API 粘贴纳入采集策略** — `paste` 命令与 `POST /paste` 端点现在经过暂停采集、敏感内容过滤与最大文本捕获上限检查，与前端采集一致 | [`fae9cc2`](https://github.com/muutot/Clipboard/commit/fae9cc2e)
- **文件复制上限不可绕过** — 清空输入框不再把 `max_file_copy_size_bytes` 归零禁用限制 | [`243000c`](https://github.com/muutot/Clipboard/commit/243000c1)
- **同步大小限制钳制** — 输入即时重算并钳制到后端窗口，切换单位保留字节数 | [`6b1e960`](https://github.com/muutot/Clipboard/commit/6b1e9600) [`dbbd303`](https://github.com/muutot/Clipboard/commit/dbbd303a)
- **WebView 提供的图片路径受管** — 采集拒绝指向托管存储之外的 webview 侧图片路径 | [`0d730f7`](https://github.com/muutot/Clipboard/commit/0d730f7a)
- **凭据代际不可变** — 保存配置不再覆盖其他 provider 的凭据代际 | [`ad78373`](https://github.com/muutot/Clipboard/commit/ad783732)
- **损坏配置先行保留** — 解析失败的设置文件先保留再应用默认值，修复不再吞掉用户数据 | [`1ec5a13`](https://github.com/muutot/Clipboard/commit/1ec5a13e)

### 主线程卡顿

- **阻塞操作移出主线程** — 按需物化同步条目、数据库修复与备份刷新、另存为文件复制改用阻塞线程池，不再冻结 UI | [`7403734`](https://github.com/muutot/Clipboard/commit/7403734e) [`10a25bc`](https://github.com/muutot/Clipboard/commit/10a25bc1) [`17c194c`](https://github.com/muutot/Clipboard/commit/17c194cf)

### 同步正确性

- **远端 head 采纳前验证内容** — 发布段内容校验通过才采纳远端 head，中断的本地提交后能恢复采纳已发布 head | [`0b7c038`](https://github.com/muutot/Clipboard/commit/0b7c038e) [`adebe9f`](https://github.com/muutot/Clipboard/commit/adebe9ff)
- **本地模式拒绝 S3 流量** — 本地-only 模式下每个 S3 条目都被门禁并取消 | [`769fbfc`](https://github.com/muutot/Clipboard/commit/769fbfcd)
- **加密身份独立绑定** — 同步加密身份不再依赖 S3 凭据存在 | [`976366a`](https://github.com/muutot/Clipboard/commit/976366a7)

### 窗口与热键

- **ToggleMain 不再自粘贴** — 主窗口前置时跳过快捷粘贴目标，全局开关不再把焦点还给自己 | [`54a4d25`](https://github.com/muutot/Clipboard/commit/54a4d259)
- **双击连击按前台取消** — 窗口切换时取消挂起的修饰键连击判定 | [`cbc9650`](https://github.com/muutot/Clipboard/commit/cbc96502)
- **失败保存保留新边界** — 窗口位置保存失败时保留较新的尺寸位置记录 | [`9e87ff9`](https://github.com/muutot/Clipboard/commit/9e87ff9c)

### 界面与交互

- **后台采集不再抢焦点选区** — 后台捕获不偷搜索与回收站视图的选中状态 | [`20b2297`](https://github.com/muutot/Clipboard/commit/20b22971)
- **事件监听先于首次加载注册** — 修复历史页加载与事件订阅的竞态 | [`8bdd058`](https://github.com/muutot/Clipboard/commit/8bdd0588)
- **键盘绑定操作串行化** — 增加/移除快捷键绑定串行执行，并发操作不再互相覆盖 | [`efdd706`](https://github.com/muutot/Clipboard/commit/efdd7060)
- **细节修正** — 快速复制徽标显示真实 Ctrl 组合键、过滤器下拉可 Escape 关闭、设置内 Escape 不再误关其他弹层、悬浮窗关闭按钮无障碍标签、非图片记录隐藏 OCR 标签页、周边界按日历日期计算、内联编辑草稿跨列表卸载保留、悬浮窗跨窗口过滤后状态对账 | [`9e1b7b9`](https://github.com/muutot/Clipboard/commit/9e1b7b98) [`60f7ba4`](https://github.com/muutot/Clipboard/commit/60f7ba4e) [`205b3fd`](https://github.com/muutot/Clipboard/commit/205b3fdf) [`b6fecbb`](https://github.com/muutot/Clipboard/commit/b6fecbbb) [`5e46ea9`](https://github.com/muutot/Clipboard/commit/5e46ea98) [`88a3dcb`](https://github.com/muutot/Clipboard/commit/88a3dcb8) [`6861597`](https://github.com/muutot/Clipboard/commit/6861597f) [`bc5f502`](https://github.com/muutot/Clipboard/commit/bc5f5024)

### 平台与存储

- **原生负载与分配设界** — 剪贴板原生负载、位图分配、预览文件读取、临时图片全部有上限与取消，防解压炸弹与内存失控 | [`33eac0d`](https://github.com/muutot/Clipboard/commit/33eac0d3) [`90086a7`](https://github.com/muutot/Clipboard/commit/90086a75) [`8e995fa`](https://github.com/muutot/Clipboard/commit/8e995fa7) [`a9bbc22`](https://github.com/muutot/Clipboard/commit/a9bbc22d)
- **Linux 描述符与子进程设界** — 停止管道原子初始化不泄漏描述符，失速子进程可中断，Wayland 监视器分发可中断 | [`b1f5da7`](https://github.com/muutot/Clipboard/commit/b1f5da78) [`213dbd7`](https://github.com/muutot/Clipboard/commit/213dbd77) [`4d6dc6b`](https://github.com/muutot/Clipboard/commit/4d6dc6bf)
- **图标与资源加固** — WEBP fourcc 校验、webp 图标可解析、符号链接图标目标拒绝、存储类型清理与重命名串行化、迁移资源冲突保留 | [`3f29976`](https://github.com/muutot/Clipboard/commit/3f29976c) [`54f1e75`](https://github.com/muutot/Clipboard/commit/54f1e75a) [`21085b4`](https://github.com/muutot/Clipboard/commit/21085b4b) [`baac783`](https://github.com/muutot/Clipboard/commit/baac7830) [`d61dbac`](https://github.com/muutot/Clipboard/commit/d61dbaca)
- **写入失败原子回滚** — 派生标签写失败回滚记录、填充失败清理暂存、部分阶段移除 | [`f649b96`](https://github.com/muutot/Clipboard/commit/f649b966) [`12fd71d`](https://github.com/muutot/Clipboard/commit/12fd71d6)

---

## 性能

- **复制不再解水合文本** — 原生复制跳过文本体解水合，复制更快 | [`8edf425`](https://github.com/muutot/Clipboard/commit/8edf425d)
- **历史按需解水合** — 列表只在需要时读文本体，配合最近使用索引与卡片高度估算缓存，滚动更顺 | [`5a2a6b5`](https://github.com/muutot/Clipboard/commit/5a2a6b5f) [`e70aedf`](https://github.com/muutot/Clipboard/commit/e70aedff) [`4091490`](https://github.com/muutot/Clipboard/commit/40914903)
- **导出单一致快照流式读取** — 导出与快照读取流式化，大库导出不再全量载入 | [`4e99c4a`](https://github.com/muutot/Clipboard/commit/4e99c4a5) [`e1db96e`](https://github.com/muutot/Clipboard/commit/e1db96e7)
- **备份预览即时释放校验体** | [`3d830f8`](https://github.com/muutot/Clipboard/commit/3d830f81)
- **搜索输入延迟可测量** — 输入延迟度量与综合基准文档化 | [`caabdec`](https://github.com/muutot/Clipboard/commit/caabdec7)

---

## 内部改进

- **模块化重构** — 平台适配按 OS 重组，macOS/X11/Wayland 适配拆分 live/intended 模块；设置服务、剪贴板命令、捕获、CLI 路由、同步 v1 传输、导出导入、存储仓库与同步状态均拆分为聚焦子模块 | [`87acd04`](https://github.com/muutot/Clipboard/commit/87acd04b) [`175b81f`](https://github.com/muutot/Clipboard/commit/175b81f5) [`c84e391`](https://github.com/muutot/Clipboard/commit/c84e3917) [`48cb85b`](https://github.com/muutot/Clipboard/commit/48cb85bb) [`5967df3`](https://github.com/muutot/Clipboard/commit/5967df34) [`f7c43ce`](https://github.com/muutot/Clipboard/commit/f7c43ce6) [`850483e`](https://github.com/muutot/Clipboard/commit/850483e6) [`95481c0`](https://github.com/muutot/Clipboard/commit/95481c0e) [`d9e7685`](https://github.com/muutot/Clipboard/commit/d9e7685c) [`d026082`](https://github.com/muutot/Clipboard/commit/d026082f) [`a9d55d9`](https://github.com/muutot/Clipboard/commit/a9d55d9f) [`787cda4`](https://github.com/muutot/Clipboard/commit/787cda45)
- **前端懒加载设置面板** — 设置面板注册表移入懒加载描述符，搜索与快捷操作控制器抽离，组件按领域分组 | [`b82698f`](https://github.com/muutot/Clipboard/commit/b82698fe) [`48fcc86`](https://github.com/muutot/Clipboard/commit/48fcc868) [`84a3bf3`](https://github.com/muutot/Clipboard/commit/84a3bf34) [`6ac131a`](https://github.com/muutot/Clipboard/commit/6ac131a3)
- **工程门禁** — pre-commit/commit-msg 钩子强制 skill 提交规则与 gitmoji 格式；CI 作业限时；Vitest 并发设界；更新存在漏洞的序列化与 source map 依赖 | [`4d99c69`](https://github.com/muutot/Clipboard/commit/4d99c696) [`4441540`](https://github.com/muutot/Clipboard/commit/4441540e) [`6f18ff9`](https://github.com/muutot/Clipboard/commit/6f18ff96) [`5f08317`](https://github.com/muutot/Clipboard/commit/5f08317a) [`a07f7f1`](https://github.com/muutot/Clipboard/commit/a07f7f18)
- **测试补充** — 真实 S3 验证取消发布恢复、前端测试纳入本地门禁、搜索后台重建等待、捕获签名适配 | [`920fa44`](https://github.com/muutot/Clipboard/commit/920fa44a) [`2eae956`](https://github.com/muutot/Clipboard/commit/2eae9566) [`0462d1d`](https://github.com/muutot/Clipboard/commit/0462d1d8) [`a681b68`](https://github.com/muutot/Clipboard/commit/a681b681) [`e626e8f`](https://github.com/muutot/Clipboard/commit/e626e8f9)

---

## 文档

- **中英双语 README 界面画廊** — 彩色紧凑模式双语截图与清晰的搜索结果预览 | [`0b547da`](https://github.com/muutot/Clipboard/commit/0b547da7) [`66d5876`](https://github.com/muutot/Clipboard/commit/66d58765) [`462f16a`](https://github.com/muutot/Clipboard/commit/462f16a7) [`a04699f`](https://github.com/muutot/Clipboard/commit/a04699fb)
- **英文 README** — 新增英文说明并从中文主文档链接 | [`61528be`](https://github.com/muutot/Clipboard/commit/61528be6)
- **维护技能与契约记录** — 仓库审计技能、采集隐私门契约、spawn_blocking 模式、ToggleMain 自窗口守卫、设置面板尺寸同步陷阱 | [`b093f29`](https://github.com/muutot/Clipboard/commit/b093f292) [`aabaf14`](https://github.com/muutot/Clipboard/commit/aabaf141) [`1ff83be`](https://github.com/muutot/Clipboard/commit/1ff83be6) [`146c549`](https://github.com/muutot/Clipboard/commit/146c5493) [`46b636a`](https://github.com/muutot/Clipboard/commit/46b636a2)
- **发布纯度契约** — 六文件 release commit 规则成文并与 CI 实际契约对齐 | [`82dde09`](https://github.com/muutot/Clipboard/commit/82dde097) [`196f9a2`](https://github.com/muutot/Clipboard/commit/196f9a25)

---

## 构建产物

- **Windows**: `Clipboard_1.8.0_x64_en-US.msi` / `Clipboard_1.8.0_x64-setup.exe`
- **macOS (Apple Silicon)**: `Clipboard_1.8.0_aarch64.app.tar.gz` / `.dmg`
- **Linux (x64)**: `Clipboard_1.8.0_amd64.AppImage` / `.deb` / `.rpm`
