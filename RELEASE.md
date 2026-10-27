# Clipboard Desktop v1.8.1

> 同步命名空间轮换（加密换钥/前缀迁移）、远端资源垃圾回收、批量多选一次粘贴、首次运行引导，以及一轮全量代码审计后的批量正确性加固
>
> Released: 2026-10-27

---

## 新功能

### 同步

- **命名空间轮换** — 一键把同步前缀与密钥轮换到新命名空间：物化全部资源、引导发布完整本地状态、写入轮换指针、再删除旧命名空间；崩溃与取消在任意步骤都安全 | [`6a46e5fa`](https://github.com/muutot/Clipboard/commit/6a46e5fa) [`3aa481a2`](https://github.com/muutot/Clipboard/commit/3aa481a2) [`6185f075`](https://github.com/muutot/Clipboard/commit/6185f075)
- **轮换拒绝与恢复** — 已轮换前缀在任何命名空间解析之前即拒绝同步运行，防止陈旧设备分叉；本地配置提交失败时原地重试并在错误中给出手工恢复指引 | [`23d1cc3a`](https://github.com/muutot/Clipboard/commit/23d1cc3a) [`ca9a805c`](https://github.com/muutot/Clipboard/commit/ca9a805c)
- **远端资源垃圾回收** — 可选开启的远端资源清理，宽限窗口锚定到存储端时钟，删除判定不受本机时钟偏移影响 | [`5790fe01`](https://github.com/muutot/Clipboard/commit/5790fe01) [`e8dd80c7`](https://github.com/muutot/Clipboard/commit/e8dd80c7) [`1f598478`](https://github.com/muutot/Clipboard/commit/1f598478)
- **S3 凭据加固** — 内存中的 S3 凭据零化处理，端点校验、scope 变更与密钥库重试一并加固 | [`5ee64e8f`](https://github.com/muutot/Clipboard/commit/5ee64e8f) [`81712059`](https://github.com/muutot/Clipboard/commit/81712059)

### 界面与操作

- **批量栏重构与一次粘贴** — 多选卡片显示粘贴顺序编号，批量栏显示条目总数，多选内容按顺序一次性粘贴到目标应用 | [`51d9834a`](https://github.com/muutot/Clipboard/commit/51d9834a) [`6750e068`](https://github.com/muutot/Clipboard/commit/6750e068) [`eb20c669`](https://github.com/muutot/Clipboard/commit/eb20c669)
- **首次运行引导** — 新用户引导页覆盖核心操作，并支持从设置回放 | [`fb0e1893`](https://github.com/muutot/Clipboard/commit/fb0e1893)
- **本地 API 随启动自启** — 本地 HTTP API 随应用启动并发布端口，无需手动开启 | [`fa0ca08d`](https://github.com/muutot/Clipboard/commit/fa0ca08d)
- **右键菜单组件化** — 通用 AppContextMenu 组件统一各处上下文菜单 | [`16fcf885`](https://github.com/muutot/Clipboard/commit/16fcf885)

### 安装与打包

- **安装包元数据** — 补全发布者、版权与主页信息；AppImage 打包前校验 appimagetool | [`eeae9b1d`](https://github.com/muutot/Clipboard/commit/eeae9b1d) [`8fdbc555`](https://github.com/muutot/Clipboard/commit/8fdbc555)

---

## 修复

### 同步正确性

- **对端异常不再阻塞检查点** — 对端拉取失败仍发布检查点；对端缺失或游标回退时跳过本轮压缩而不是硬失败 | [`625f3bcc`](https://github.com/muutot/Clipboard/commit/625f3bcc) [`f9fe856d`](https://github.com/muutot/Clipboard/commit/f9fe856d)
- **孤儿段校验与快照清理** — 孤儿段对照待发队列校验，发件箱为空时跳过扫描；启动时清理陈旧数据库快照 | [`bc028173`](https://github.com/muutot/Clipboard/commit/bc028173) [`58644db0`](https://github.com/muutot/Clipboard/commit/58644db0) [`b82e868e`](https://github.com/muutot/Clipboard/commit/b82e868e)
- **跳过文件保持序号对齐** — 超限文件被跳过时资源序号保持对齐，不破坏引用关系 | [`193a6cbe`](https://github.com/muutot/Clipboard/commit/193a6cbe)
- **重操作移出主线程** — 重型同步、搜索与 OCR 命令改用阻塞线程池，不再冻结 UI | [`da9bfe55`](https://github.com/muutot/Clipboard/commit/da9bfe55) [`8202d415`](https://github.com/muutot/Clipboard/commit/8202d415)

### 存储与数据

- **截断文件自动修复** — 内容寻址文件损坏时从暂存字节修复 | [`b7a17034`](https://github.com/muutot/Clipboard/commit/b7a17034)
- **保留期限按活跃计算** — 年龄保留与容量淘汰按最近活跃时间计算，长期收藏不受误清 | [`677b9ddf`](https://github.com/muutot/Clipboard/commit/677b9ddf)
- **回收站行跳过预览改写** — 回收站中的记录不再被更新预览路径 | [`5d28a843`](https://github.com/muutot/Clipboard/commit/5d28a843)
- **元数据文件纳入清理引用** | [`85ee7fdb`](https://github.com/muutot/Clipboard/commit/85ee7fdb)
- **重命名修正** — 文本/链接重命名直接落库保留用户标题，再次捕获不覆盖；并发删除的行如实报错 | [`2cdd1e90`](https://github.com/muutot/Clipboard/commit/2cdd1e90) [`4d6163b9`](https://github.com/muutot/Clipboard/commit/4d6163b9)
- **配置钳制补全** — 截图大小上限与历史/文件大小限制同样钳制，手改配置文件无法绕过 | [`7b117c45`](https://github.com/muutot/Clipboard/commit/7b117c45) [`055dac6b`](https://github.com/muutot/Clipboard/commit/055dac6b)
- **图标引用保护** — 按名称跨平台匹配图标引用，删除受引用的图标文件被拒绝，发布中的图标删除有守卫 | [`8aeb3f36`](https://github.com/muutot/Clipboard/commit/8aeb3f36) [`0b8e5fbf`](https://github.com/muutot/Clipboard/commit/0b8e5fbf) [`9a22d1eb`](https://github.com/muutot/Clipboard/commit/9a22d1eb)

### 隐私与导入导出

- **导入路径清洗与负载上限** — 导入 JSON 的路径字段被清洗，文件与 IPC 导入负载设上限 | [`29bd0ff8`](https://github.com/muutot/Clipboard/commit/29bd0ff8) [`1294311c`](https://github.com/muutot/Clipboard/commit/1294311c)
- **导出分隔符转义** — 纯文本导出转义记录分隔符，防注入伪造条目 | [`f4fd1d8d`](https://github.com/muutot/Clipboard/commit/f4fd1d8d)
- **备份保留不可归档媒体** | [`0ef776df`](https://github.com/muutot/Clipboard/commit/0ef776df)

### 平台与窗口

- **隐藏窗口不再挂起 WebView2** — 从显隐路径移除挂起/恢复，消除唤起闪屏与竞态；空闲悬浮窗独立回收 | [`8cc4d99f`](https://github.com/muutot/Clipboard/commit/8cc4d99f) [`9b3dd983`](https://github.com/muutot/Clipboard/commit/9b3dd983)
- **快捷粘贴目标校验** — 校验粘贴目标窗口属主 PID 并轮询焦点，注入前等待修饰键就绪 | [`980a0e58`](https://github.com/muutot/Clipboard/commit/980a0e58) [`5906aebe`](https://github.com/muutot/Clipboard/commit/5906aebe)
- **热键稳定性** — 全平台启动全局热键，防止连键重放与陈旧目标，退出时回收分发线程 | [`4a6abd7e`](https://github.com/muutot/Clipboard/commit/4a6abd7e) [`41b862da`](https://github.com/muutot/Clipboard/commit/41b862da) [`1bd30487`](https://github.com/muutot/Clipboard/commit/1bd30487)
- **剪贴板读取加固** — 自触发标记读取失败即重试打开，解码图片前先关闭剪贴板，X11 原生文本读取设上限，快照文本按长度截断 | [`0f8994bb`](https://github.com/muutot/Clipboard/commit/0f8994bb) [`50ddaa26`](https://github.com/muutot/Clipboard/commit/50ddaa26) [`f2332d87`](https://github.com/muutot/Clipboard/commit/f2332d87) [`075d89ba`](https://github.com/muutot/Clipboard/commit/075d89ba)
- **单实例接管等待锁** | [`9203d4cf`](https://github.com/muutot/Clipboard/commit/9203d4cf)
- **Windows stderr 重定向到日志** | [`f3db94cd`](https://github.com/muutot/Clipboard/commit/f3db94cd)
- **macOS 链接修复** — CoreFoundation 符号声明与链接方式统一，部署目标与 rustc 最低要求对齐 | [`2ec42f0d`](https://github.com/muutot/Clipboard/commit/2ec42f0d) [`5fdb6989`](https://github.com/muutot/Clipboard/commit/5fdb6989) [`487f9540`](https://github.com/muutot/Clipboard/commit/487f9540) [`8edebc75`](https://github.com/muutot/Clipboard/commit/8edebc75)

### 界面细节

- **保存设置全面板生效** — 设置面板保存后跨面板一致应用 | [`c7be7772`](https://github.com/muutot/Clipboard/commit/c7be7772)
- **标签颜色即时刷新** — 仅改色时同步刷新，标签改写延后到扫描游标关闭 | [`a3f44e35`](https://github.com/muutot/Clipboard/commit/a3f44e35) [`54b06850`](https://github.com/muutot/Clipboard/commit/54b06850)
- **批量粘贴部分失败可见** — 失败行保留在选中态并提示错误，防连点重复粘贴 | [`49801c3b`](https://github.com/muutot/Clipboard/commit/49801c3b)
- **搜索与筛选** — 应用保存搜索时从第 0 页重建分页，存储类型清理后失效缓存结果，输入延迟逐查询度量 | [`774c5801`](https://github.com/muutot/Clipboard/commit/774c5801) [`6f55b086`](https://github.com/muutot/Clipboard/commit/6f55b086) [`6f7dbce9`](https://github.com/muutot/Clipboard/commit/6f7dbce9)
- **交互细节** — Escape 先退多选再隐窗、系统语言改从 OS locale 解析、搜索框字号滑杆归位字体面板、状态栏计数右移、统计数值不折行、无 OCR 引擎时不再虚报、HTML 粘贴缺内容时降级纯文本、修复标签颜色刷新/分页/引导焦点等杂项 | [`7ff757a8`](https://github.com/muutot/Clipboard/commit/7ff757a8) [`23ddf776`](https://github.com/muutot/Clipboard/commit/23ddf776) [`be8a818b`](https://github.com/muutot/Clipboard/commit/be8a818b) [`c86edbb4`](https://github.com/muutot/Clipboard/commit/c86edbb4) [`4be5f29e`](https://github.com/muutot/Clipboard/commit/4be5f29e) [`a31c6da3`](https://github.com/muutot/Clipboard/commit/a31c6da3) [`4180279f`](https://github.com/muutot/Clipboard/commit/4180279f) [`d27871aa`](https://github.com/muutot/Clipboard/commit/d27871aa)

---

## 性能

- **同步资源校验记忆化** — 按路径、对象键与文件 stat 记忆校验结果，大文件粘贴/复制不再重复全量哈希 | [`7e101b17`](https://github.com/muutot/Clipboard/commit/7e101b17)
- **原生序列未变跳过读取** | [`bb05998f`](https://github.com/muutot/Clipboard/commit/bb05998f)
- **快速粘贴走阻塞池** | [`a97ab438`](https://github.com/muutot/Clipboard/commit/a97ab438)
- **测量缓存键哈希化** — 不再保留全文文本 | [`e5a8c87b`](https://github.com/muutot/Clipboard/commit/e5a8c87b)
- **代码预览分词设上限** | [`524c44fb`](https://github.com/muutot/Clipboard/commit/524c44fb)
- **Tesseract 可用性探测记忆化** | [`bdccef63`](https://github.com/muutot/Clipboard/commit/bdccef63)

---

## 内部改进

- **真实 S3 冒烟矩阵进 CI** — 固定 MinIO/rustfs 版本，覆盖传输边界、连接诊断、流式下载上限、加密资源与完整轮换流程 | [`e17e315b`](https://github.com/muutot/Clipboard/commit/e17e315b) [`c5fe56cb`](https://github.com/muutot/Clipboard/commit/c5fe56cb) [`4f9fa47b`](https://github.com/muutot/Clipboard/commit/4f9fa47b) [`a6a2c022`](https://github.com/muutot/Clipboard/commit/a6a2c022)
- **CI 失败指纹** — 链接失败、clippy 与测试失败自动提取诊断状态，加速 macOS 链接问题定位 | [`728d8b50`](https://github.com/muutot/Clipboard/commit/728d8b50) [`301fa9ef`](https://github.com/muutot/Clipboard/commit/301fa9ef) [`d27ce248`](https://github.com/muutot/Clipboard/commit/d27ce248)
- **发布契约修正** — 发布文件清单对齐为七文件（含 package-lock.json），release workflow 各作业补齐超时上限 | [`eae40d25`](https://github.com/muutot/Clipboard/commit/eae40d25) [`b3c8b36f`](https://github.com/muutot/Clipboard/commit/b3c8b36f)
- **设置面板样式统一** — 面板样式与主题应用收敛到共享路径 | [`6db67cdc`](https://github.com/muutot/Clipboard/commit/6db67cdc) [`4f09b7cd`](https://github.com/muutot/Clipboard/commit/4f09b7cd)
- **测试补充** — 原生剪贴板回环 harness、搜索管线/重命名拒绝/捕获熔断/导入上限等分支覆盖，compaction 与快照过滤单测 | [`24dc2607`](https://github.com/muutot/Clipboard/commit/24dc2607) [`cf86163f`](https://github.com/muutot/Clipboard/commit/cf86163f) [`2ccf9e9f`](https://github.com/muutot/Clipboard/commit/2ccf9e9f) [`63dcd09b`](https://github.com/muutot/Clipboard/commit/63dcd09b) [`81271032`](https://github.com/muutot/Clipboard/commit/81271032)
- **仓库卫生** — 移除误提交的 worktree gitlink、导出源码去 BOM、文档乱码修复 | [`aa0101a9`](https://github.com/muutot/Clipboard/commit/aa0101a9) [`af266a0a`](https://github.com/muutot/Clipboard/commit/af266a0a) [`0e475f9b`](https://github.com/muutot/Clipboard/commit/0e475f9b)

---

## 构建产物

- **Windows**: `Clipboard_1.8.1_x64_en-US.msi` / `Clipboard_1.8.1_x64-setup.exe`
- **macOS (Apple Silicon)**: `Clipboard_1.8.1_aarch64.app.tar.gz` / `.dmg`
- **Linux (x64)**: `Clipboard_1.8.1_amd64.AppImage` / `.deb` / `.rpm`
