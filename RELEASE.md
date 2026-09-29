# Clipboard Desktop v1.7.2

> 同步与安全加固、Linux AppImage 修复、存储与搜索可靠性提升
>
> Released: 2026-09-29

---

## 新功能

### 同步

- **瞬时 S3 失败自动重试** — 按有界退避策略重试瞬时 S3 故障，减少偶发网络抖动导致的中断 | [`1f0ed00`](https://github.com/muutot/Clipboard/commit/1f0ed007)

### 存储管理

- **资源目录所有权标记常显开关** — 所有权标记从隐藏文件升级为设置中始终可见的开关 | [`f3a2fe5`](https://github.com/muutot/Clipboard/commit/f3a2fe55)
- **从设置认领所有权标记** — 用户可在设置界面直接认领资源根目录的所有权标记 | [`c4f123c`](https://github.com/muutot/Clipboard/commit/c4f123ca)

### 诊断

- **分级日志诊断** — 新增带级别阈值的诊断日志，可在设置中控制输出阈值 | [`aa141d3`](https://github.com/muutot/Clipboard/commit/aa141d32)

### 详情面板

- **主题化编辑器右键菜单** — 编辑时隐藏预览操作按钮，提供与应用主题一致的右键菜单 | [`442c739`](https://github.com/muutot/Clipboard/commit/442c739a)

---

## 安全加固

- **密钥零化** — 派生的同步密钥与 DPAPI 明文使用后立即清零 | [`1e2265a`](https://github.com/muutot/Clipboard/commit/1e2265a4)
- **本地 API 回环 ACL** — Windows 回环令牌应用仅所有者可访问的 ACL | [`af113f7`](https://github.com/muutot/Clipboard/commit/af113f7a)
- **拒绝非回环明文 http S3 端点** — 防止同步数据经明文通道外泄 | [`70ef184`](https://github.com/muutot/Clipboard/commit/70ef1840)
- **OCR 模型下载校验** — 上游模型下载钉扎并验证 SHA-256 | [`772458d`](https://github.com/muutot/Clipboard/commit/772458d6)
- **空静态资源作用域** — 阻止暴露安装目录内机密文件 | [`c175ed8`](https://github.com/muutot/Clipboard/commit/c175ed8a)
- **不可信图片解码限额** — 防解压炸弹攻击 | [`a765989`](https://github.com/muutot/Clipboard/commit/a765989f)
- **CF_HDROP 缓冲区按查询分配** — 防止剪贴板文件列表溢出 panic | [`2900a7c`](https://github.com/muutot/Clipboard/commit/2900a7ce)
- **超大文件入库前拒绝并清理暂存** — 暂存失败同样清理 | [`44bfe4d`](https://github.com/muutot/Clipboard/commit/44bfe4d6)

---

## 同步可靠性

- **打包块大小边界收紧** — 写入侧同样限制打包块存储大小，要求打包块匹配磁盘剩余字节 | [`e99eb81`](https://github.com/muutot/Clipboard/commit/e99eb814) [`ecd29fe`](https://github.com/muutot/Clipboard/commit/ecd29fe8)
- **S3 响应体内存上限** — 内存中的 S3 对象读取与响应体均硬性限制在协议上限内，超限时报告组件而非 panic | [`8af6330`](https://github.com/muutot/Clipboard/commit/8af63309) [`0df511f`](https://github.com/muutot/Clipboard/commit/0df511f2) [`55db26c`](https://github.com/muutot/Clipboard/commit/55db26c0)
- **拉取跳过游离段键** — 遇到陌生段键时跳过而非中止整个拉取 | [`e8e2bdb`](https://github.com/muutot/Clipboard/commit/e8e2bdbc)
- **单个超大记录放行** — 每个打包块允许一条超大记录，大条目可正常发布 | [`5dd2362`](https://github.com/muutot/Clipboard/commit/5dd2362c)
- **打包批惰性折叠** — 应用期内存驻留恒定为一个块，解码先于事务打开 | [`0d0d78d`](https://github.com/muutot/Clipboard/commit/0d0d78dd) [`df6c9e1`](https://github.com/muutot/Clipboard/commit/df6c9e12)

---

## 修复

### 发布构建

- **修复 Tauri Rust crate 与 npm 包 minor 版本漂移导致全部平台构建失败** — 发布流程升级 Cargo.lock 后 npm 侧 `@tauri-apps/api`、`@tauri-apps/plugin-dialog` 未同步，tauri CLI 一致性检查拒绝构建；本次对齐两侧版本，并让发布脚本在打 tag 前校验一致性，漂移即失败并给出修复命令 | [`5e24d92`](https://github.com/muutot/Clipboard/commit/5e24d920) [`e753eb6`](https://github.com/muutot/Clipboard/commit/e753eb6c)
- **修复 AppImage 在 root 挂载沙箱下 Permission denied** — tauri-bundler 以 770 权限保存 AppRun 启动器，firejail 与 AppImage 官方目录测试以其他用户运行时启动即失败（tauri-apps/tauri#16155）。发布工作流现预置 755 权限、SHA-256 钉扎的启动器，并新增构建后权限门禁，任一文件权限受限即失败 | [`a27dbc7`](https://github.com/muutot/Clipboard/commit/a27dbc75)
- **修复重打包丢失执行权限导致 AppImage 启动即退出** — linuxdeploy/appimagetool 重打包后 `AppRun.wrapped` 等文件的执行位未保留，AppImage 官方目录测试报 error-not-executable 并在 11 秒内退出；现统一规范化目录与文件权限后重新打包，权限门禁同步覆盖重打包产物 | [`0ab11b2`](https://github.com/muutot/Clipboard/commit/0ab11b21)
- **加固权限门禁的 realpath 判定** — 针对符号链接解析的偶发失败重试与降级处理，避免门禁误报 | [`630f5e6`](https://github.com/muutot/Clipboard/commit/630f5e62)
- **适配 appimagetool v13 资产改名** — 下载地址改用改名后的 v13 资产名，修复工具下载 404 | [`2ce3870`](https://github.com/muutot/Clipboard/commit/2ce38707)

### CI

- **删除 rust-cache 不存在的 `env-cache-key` 输入** — 默认 env-vars 前缀已覆盖 RUSTFLAGS 与 CARGO_PROFILE_*，缓存键行为不变；checkout/setup-node 升级到原生 node24 版本；前端 node_modules 缓存改用 setup-node 内置 npm 缓存 | [`bb6c8e5`](https://github.com/muutot/Clipboard/commit/bb6c8e5f)
- **两段式发布流程可通过脏树门禁** — 发布文件集合白名单化，Pass 1 的预期产物不再触发拒绝 | [`820a339`](https://github.com/muutot/Clipboard/commit/820a3394)
- **全部平台构建成功后才发布 GitHub Release** — 三平台独立构建臂改为草稿发布，由 `publish` 任务统一转正 | [`30766c4`](https://github.com/muutot/Clipboard/commit/30766c4c)
- **脏工作区拒绝发布** | [`98a3dd2`](https://github.com/muutot/Clipboard/commit/98a3dd2e)
- **CI 覆盖所有分支与 PR**，同步工作流钉扎 actions 与 Python 依赖 | [`811a681`](https://github.com/muutot/Clipboard/commit/811a6813) [`61581a2`](https://github.com/muutot/Clipboard/commit/61581a22)

### 存储与数据

- **导入不再污染同步版本时钟** | [`e1a49bf`](https://github.com/muutot/Clipboard/commit/e1a49bf2)
- **条目上限单事务即时执行** | [`f7b1728`](https://github.com/muutot/Clipboard/commit/f7b17284)
- **去重 upsert 限定内容键** — 防止导入覆盖既有条目 ID | [`95b50df`](https://github.com/muutot/Clipboard/commit/95b50df4)
- **迁移目录遍历防 junction 环路** | [`9166652`](https://github.com/muutot/Clipboard/commit/9166652d)
- **未知资源种类返回错误而非 panic** | [`a243d8f`](https://github.com/muutot/Clipboard/commit/a243d8f7)
- **历史清理不再全程持有配置锁** | [`c0b2f16`](https://github.com/muutot/Clipboard/commit/c0b2f165)
- **所有权未变化时不要求重启** | [`2bb5a42`](https://github.com/muutot/Clipboard/commit/2bb5a42a)
- **ppaste 图片原子写入并修复截断文件** | [`fdef845`](https://github.com/muutot/Clipboard/commit/fdef8453)

### 搜索与内容

- **清单不可读时保留索引** | [`e6a1ce9`](https://github.com/muutot/Clipboard/commit/e6a1ce93)
- **无关设置变更不再重置分页** | [`2b7c2bd`](https://github.com/muutot/Clipboard/commit/2b7c2bdd)
- **缩略图原子写入** — 避免截断正在使用的预览 | [`ab9a6bd`](https://github.com/muutot/Clipboard/commit/ab9a6bde)

### 界面与交互

- **批量变更按条目回滚** — 不再从整组快照回滚 | [`9c6d040`](https://github.com/muutot/Clipboard/commit/9c6d0406)
- **内联编辑仅保留一张卡片** | [`6a68a24`](https://github.com/muutot/Clipboard/commit/6a68a246)
- **输入时不触发悬浮切换快捷键** | [`df8e4ca`](https://github.com/muutot/Clipboard/commit/df8e4ca6)
- **右键菜单可复制编辑器选区** | [`a381e6f`](https://github.com/muutot/Clipboard/commit/a381e6f5)
- **全屏查看器丢弃过期物化结果** | [`d058c5c`](https://github.com/muutot/Clipboard/commit/d058c5c0)
- **本地打开按钮改为 reveal，放行 mailto/tel 链接** | [`c89373c`](https://github.com/muutot/Clipboard/commit/c89373c0)
- **fire-and-forget IPC 调用的拒绝有兜底处理** | [`26b4a32`](https://github.com/muutot/Clipboard/commit/26b4a326)
- **异步设置面板控件等待水合后再渲染**，窗口配置载入共享 store | [`3a6738f`](https://github.com/muutot/Clipboard/commit/3a6738ff) [`7608f8a`](https://github.com/muutot/Clipboard/commit/7608f8ac)

### 平台与启动

- **启动存活探测失败不再误判所有者已死** | [`35f178f`](https://github.com/muutot/Clipboard/commit/35f178f0)
- **自动同步间隔读入时钳制** | [`f6a9712`](https://github.com/muutot/Clipboard/commit/f6a97125)
- **Windows 图标 DIB 校验尺寸运算**，X11 窗口属性全分支释放 | [`4ae6e79`](https://github.com/muutot/Clipboard/commit/4ae6e791) [`84c590d`](https://github.com/muutot/Clipboard/commit/84c590d0)
- **OCR 管道排空防大输出死锁**，worker 线程失败返回错误 | [`01888a2`](https://github.com/muutot/Clipboard/commit/01888a25) [`b786d36`](https://github.com/muutot/Clipboard/commit/b786d362)
- **文件导出/导入移至阻塞线程池** | [`1a7b5bb`](https://github.com/muutot/Clipboard/commit/1a7b5bb8)
- **中断处理器安装失败留有日志** | [`367e8e5`](https://github.com/muutot/Clipboard/commit/367e8e55)

---

## 测试

- **同步真实 S3 冒烟套件与 rustfs 沙盒**（opt-in） | [`eb09939`](https://github.com/muutot/Clipboard/commit/eb099395)
- **搜索 outbox 确认等待 worker 通知** | [`98b62b1`](https://github.com/muutot/Clipboard/commit/98b62b13)
- **超大捕获容忍缺失暂存目录** | [`d8f035f`](https://github.com/muutot/Clipboard/commit/d8f035ff)
- **OCR 模型摘要校验按长度与 mtime 缓存** | [`7fa2d34`](https://github.com/muutot/Clipboard/commit/7fa2d342)

---

## 构建产物

- **MSI 安装包**: `Clipboard_1.7.2_x64_en-US.msi`
- **NSIS 安装包**: `Clipboard_1.7.2_x64-setup.exe`
- **Linux AppImage**: `Clipboard_1.7.2_amd64.AppImage`
- **Linux deb / rpm**: `Clipboard_1.7.2_amd64.deb` / `Clipboard-1.7.2-1.x86_64.rpm`
- **macOS (Apple Silicon)**: `Clipboard_1.7.2_aarch64.dmg`
