# Clipboard Desktop v1.5.0

> 剪贴板桌面管理工具，Windows 原生剪贴板监控、全文搜索、OCR 识别、多引擎支持
>
> Released: 2026-08-25

---

## 安全加固

- **本地 API 鉴权体系** — 所有请求必须携带 `conf/api.token` 中的 Bearer Token、Host 必须指向回环地址，并拒绝携带浏览器 Origin 的请求；每个连接独立线程处理，新增响应写超时（30s）与并发连接上限（32），慢客户端不再拖垮其他调用方 | [`2e22c73`](https://github.com/muutot/Clipboard/commit/2e22c732) [`9091875`](https://github.com/muutot/Clipboard/commit/9091875a) [`54fc343`](https://github.com/muutot/Clipboard/commit/54fc3437)
- **同步凭据 DPAPI 加密** — S3 密钥与同步密码以 Windows DPAPI 信封落盘；跨机器/用户拷贝配置导致无法解密时显式丢弃密文并输出醒目日志，而非把密文透传给 S3 客户端 | [`5f8b300`](https://github.com/muutot/Clipboard/commit/5f8b300a) [`e154724`](https://github.com/muutot/Clipboard/commit/e1547247)
- **路径安全** — 重命名目标清理路径遍历与保留设备名；图标删除限定在受管 icons 目录内；移除无限制的 `copy_file_to` 命令 | [`cfd80d0`](https://github.com/muutot/Clipboard/commit/cfd80d0e) [`a33b1fa`](https://github.com/muutot/Clipboard/commit/a33b1fa0)
- **Asset 协议收紧** — 全局通配符替换为仅覆盖受管存储根目录的 scoped 授权 | [`a3c66b0`](https://github.com/muutot/Clipboard/commit/a3c66b06)
- **外链打开白名单** — `open_external_url` 仅允许 http(s) scheme（大小写不敏感），杜绝 `file://`、`javascript:` 等注入 | [`3c8abf2`](https://github.com/muutot/Clipboard/commit/3c8abf25) [`a341e4d`](https://github.com/muutot/Clipboard/commit/a341e4d4)
- **隐私过滤 fail-closed** — 敏感模式锁中毒时仍执行匹配而非放行采集；无效敏感正则告警提示而非静默丢弃 | [`68c08b0`](https://github.com/muutot/Clipboard/commit/68c08b0c) [`7fc0910`](https://github.com/muutot/Clipboard/commit/7fc09100)

---

## OCR 改进

- **模型下载完整性（TOFU）** — 下载完成后计算 SHA-256 写入本地摘要文件，后续安装时校验磁盘文件，不符则重新下载；不依赖上游固定哈希，模型更新零维护成本 | [`9d35d4b`](https://github.com/muutot/Clipboard/commit/9d35d4bd)
- **下载门控** — OCR 模型下载尊重"仅本地模式"隐私开关，并使用带超时的 HTTP 客户端 | [`40b52ad`](https://github.com/muutot/Clipboard/commit/40b52ade)
- **Tesseract 输出修正** — 不再从纯文本输出伪造文字块几何数据 | [`445f8b2`](https://github.com/muutot/Clipboard/commit/445f8b2c)
- **并发安全** — PpOcrEngine 的 Send/Sync 改为编译器派生并加编译期锁测试，替代手写 unsafe impl | [`4ea3e4c`](https://github.com/muutot/Clipboard/commit/4ea3e4c0)

---

## 隐私与设置

- **敏感内容过滤独立分组** — 设置 → 采集 下新增"敏感内容过滤"二级页，集中"仅本地模式"开关与"敏感内容正则"编辑器 | [`b0539ed`](https://github.com/muutot/Clipboard/commit/b0539ed2)
- **精简冗余设置** — 移除"记录密码管理器内容"开关及运行时二次拦截层：忽略列表默认即预置 1Password、Bitwarden、KeePass 等来源，以忽略列表为唯一事实源 | [`500aae3`](https://github.com/muutot/Clipboard/commit/500aae38) [`a618a3b`](https://github.com/muutot/Clipboard/commit/a618a3b3)
- **配置容错** — 无法解析的 conf.json 隔离保存并以默认值启动，不再拒绝启动 | [`6b17110`](https://github.com/muutot/Clipboard/commit/6b171105)

---

## 可访问性与界面

- **键盘可达性** — 工具栏、筛选下拉与设置界面可全程键盘操作；模态对话框焦点陷阱并在关闭时还原焦点；隐藏卡片操作移出 Tab 序；右键子菜单支持 Enter/Space 展开并聚焦首项 | [`c11a624`](https://github.com/muutot/Clipboard/commit/c11a6241) [`c5dfe87`](https://github.com/muutot/Clipboard/commit/c5dfe87d) [`fbef1f4`](https://github.com/muutot/Clipboard/commit/fbef1f4f) [`b8116f9`](https://github.com/muutot/Clipboard/commit/b8116f96) [`8c469eb`](https://github.com/muutot/Clipboard/commit/8c469ebc)
- **交互修复** — Enter/Space 正确激活聚焦按钮；OCR 轮询不再周期性抢占详情面板焦点；Toast 定时器销毁时清理；发布链接固定 https；标签颜色输入白名单校验 | [`f41db6f`](https://github.com/muutot/Clipboard/commit/f41db6ff) [`a1fbe55`](https://github.com/muutot/Clipboard/commit/a1fbe554) [`9007f2a`](https://github.com/muutot/Clipboard/commit/9007f2ab)
- **文本修复** — 恢复主页面被编码破坏的字符（清除搜索按钮 ×、注释中的破折号与 ⌘ 符号）| [`6a8c316`](https://github.com/muutot/Clipboard/commit/6a8c3162)
- **空启动修复** — 桌面运行时以空列表启动，演示数据不会伪装成历史记录 | [`e095f93`](https://github.com/muutot/Clipboard/commit/e095f930)

---

## 稳定性修复

- **采集管线** — 剪贴板序列号在读取中途变化时丢弃混合快照；图片目录写入与 OCR 入队失败记录日志而非吞掉 | [`ce2b68c`](https://github.com/muutot/Clipboard/commit/ce2b68ca) [`0327d03`](https://github.com/muutot/Clipboard/commit/0327d034)
- **文件重命名** — 先持久化新路径再搬移文件，失败时回滚数据库记录 | [`5838762`](https://github.com/muutot/Clipboard/commit/58387627)
- **目录迁移** — 迁移前停止后台写入者，失败后恢复暂停状态 | [`48685f3`](https://github.com/muutot/Clipboard/commit/48685f3a)
- **搜索索引** — 打开失败先延时重试一次再重建目录，瞬时干扰（杀软锁定等）不再触发全量重建；初始同步失败记录日志；相对日期范围容忍 DST 歧义 | [`59f5fe9`](https://github.com/muutot/Clipboard/commit/59f5fe9f) [`54995f5`](https://github.com/muutot/Clipboard/commit/54995f5f) [`223af0e`](https://github.com/muutot/Clipboard/commit/223af0eb)
- **前端缓存一致性** — 条目变更统一走 applyItemPatches/removeItems 漏斗，闭合 searchCache 与详情面板的漂移；批量收藏失败逐条回滚；重复复制晋升同步补丁；OFFSET 分页去重并重建游标 | [`8ede219`](https://github.com/muutot/Clipboard/commit/8ede219b) [`8f378bf`](https://github.com/muutot/Clipboard/commit/8f378bfb) [`4c5936e`](https://github.com/muutot/Clipboard/commit/4c5936eb) [`a523df7`](https://github.com/muutot/Clipboard/commit/a523df77) [`cb71fda`](https://github.com/muutot/Clipboard/commit/cb71fdae) [`d50c305`](https://github.com/muutot/Clipboard/commit/d50c305c) [`06292de`](https://github.com/muutot/Clipboard/commit/06292de9)
- **命令线程模型** — 同步、导入导出、存储迁移等耗时命令移出主线程执行 | [`92bb973`](https://github.com/muutot/Clipboard/commit/92bb973d)

---

## 性能与可观测性

- **测量缓存** — 视觉行测量 memoize 化，字体变更即时失效（缓存命中也检查字体）；卡片高度估算抽取为独立模块 | [`ed2a348`](https://github.com/muutot/Clipboard/commit/ed2a348a) [`a017bc0`](https://github.com/muutot/Clipboard/commit/a017bc0d) [`d34ccdc`](https://github.com/muutot/Clipboard/commit/d34ccdc)
- **备份导入单事务** — PPaste 导入由逐行提交改为单事务，速度大幅提升 | [`6dc022e`](https://github.com/muutot/Clipboard/commit/6dc022e8)
- **内存采样** — 使用 GetProcessMemoryInfo 替代每次 spawn tasklist 进程 | [`7ec57bc`](https://github.com/muutot/Clipboard/commit/7ec57bc6)
- **自触发哈希复用** — 图片存储复用自触发检查的媒体哈希，避免重复计算 | [`9f897f5`](https://github.com/muutot/Clipboard/commit/9f897f5a)
- **持久化诊断日志** — GUI 构建中 stderr 重定向到轮转日志文件，运行期超限就地截断；全后端统一时间戳 log_event 通道 | [`342bdb9`](https://github.com/muutot/Clipboard/commit/342bdb93) [`1e1b88d`](https://github.com/muutot/Clipboard/commit/1e1b88df) [`13c396f`](https://github.com/muutot/Clipboard/commit/13c396fa)

---

## 工程与测试

- **Vitest 测试基建** — 引入 jsdom 环境，覆盖虚拟滚动与焦点陷阱工具函数（21 用例）| [`ac87577`](https://github.com/muutot/Clipboard/commit/ac875779)
- **发布流水线加固** — 手动触发的发布按输入版本合成标签名；构建前校验 tag 与 package.json / tauri.conf.json / Cargo.toml 版本一致性；verify 与三平台构建接入 sccache；`npm ci` 替代 frozen install | [`3738a51`](https://github.com/muutot/Clipboard/commit/3738a51a) [`7f98ad9`](https://github.com/muutot/Clipboard/commit/7f98ad98) [`854ca71`](https://github.com/muutot/Clipboard/commit/854ca71a)
- **CI 加固** — Actions 固定到 commit SHA、构建任务最小写权限、发布附带 SHA-256 校验和；依赖与版本清单变更也触发 CI | [`806ebae`](https://github.com/muutot/Clipboard/commit/806ebaec) [`4a620c1`](https://github.com/muutot/Clipboard/commit/4a620c1c)
- **文档对齐** — README 隐私承诺与实际网络面一致；Node.js 要求统一 >= 20.19；公开采集与搜索索引 API 文档 | [`f45ed1a`](https://github.com/muutot/Clipboard/commit/f45ed1a7) [`527eadd`](https://github.com/muutot/Clipboard/commit/527eadd6) [`b1d7a56`](https://github.com/muutot/Clipboard/commit/b1d7a564)

---

## 构建产物

- **MSI 安装包**: `Clipboard_1.5.0_x64_en-US.msi`
- **NSIS 安装包**: `Clipboard_1.5.0_x64-setup.exe`
