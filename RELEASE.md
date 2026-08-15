# Clipboard Desktop v1.4.0

> 剪贴板桌面管理工具，Windows 原生剪贴板监控、全文搜索、OCR 识别、多引擎支持
>
> Released: 2026-08-15

---

## 版本说明

v1.4.0 的核心是一套完整的**跨设备同步**系统：新增 S3 兼容对象存储与 WebDAV 两种同步后端，支持端到端加密（同步密码保护 baseline / oplog 载荷与资源对象）、真正的 AWS SigV4 签名、增量快照 / 段传输与按需物化远程资源；后台自动同步 worker 原子应用远端变更，并在应用后刷新本地历史。同步层经过大量安全加固（拒绝不安全备份文件名、保护资源路径与完整性、持久化稳定设备标识、从检查点恢复丢失历史）。此外新增自然语言日期搜索过滤（今天 / 昨天 / 本周 / 本月）、CSV 导入、可配置的敏感内容过滤（正则 + 采集面板 UI）、最近使用默认排序；设置系统重构为通用 SettingEntry 组件，并补全 Linux 平台的剪贴板文件路径 / HTML / RTF 采集。发布流程改为纯本地，构建仅由 GitHub Actions 在打 tag 后执行。

## 跨设备同步

- **S3 兼容对象存储后端** — 新增 S3 兼容远端存储，支持真正的 AWS SigV4 签名 | [`60ba6c7e`](https://github.com/muutot/Clipboard/commit/60ba6c7e) [`201ecbdf`](https://github.com/muutot/Clipboard/commit/201ecbdf)
- **WebDAV 增量同步** — 云端备份与增量同步核心，复用共享 reqwest 客户端 | [`a508c5db`](https://github.com/muutot/Clipboard/commit/a508c5db) [`8e57c0c6`](https://github.com/muutot/Clipboard/commit/8e57c0c6)
- **端到端加密** — 同步密码加密远程 baseline / oplog 与 S3 资源对象 | [`00ba4832`](https://github.com/muutot/Clipboard/commit/00ba4832) [`83999211`](https://github.com/muutot/Clipboard/commit/83999211) [`0ea0a36c`](https://github.com/muutot/Clipboard/commit/0ea0a36c)
- **版本化快照与段引擎** — v1 快照 / 段引擎、流式资源缓存、版本化 oplog/baseline 线格式（proto 信封） | [`81e98a9f`](https://github.com/muutot/Clipboard/commit/81e98a9f) [`f1f1802c`](https://github.com/muutot/Clipboard/commit/f1f1802c) [`706fcd0c`](https://github.com/muutot/Clipboard/commit/706fcd0c)
- **自动同步后台 worker** — 后台自动同步，原子应用远端变更并在应用后刷新历史 | [`0e792208`](https://github.com/muutot/Clipboard/commit/0e792208) [`c826f4ba`](https://github.com/muutot/Clipboard/commit/c826f4ba) [`66329fe0`](https://github.com/muutot/Clipboard/commit/66329fe0)
- **远程压缩与稳定设备标识** — 手动远程压缩与建议提示、持久化稳定设备身份 | [`73d50547`](https://github.com/muutot/Clipboard/commit/73d50547) [`5e419576`](https://github.com/muutot/Clipboard/commit/5e419576)
- **安全加固** — 保护资源路径与完整性、拒绝不安全备份文件名、加固 S3 对象写入、隔离远端 peer 失败 | [`929d63e8`](https://github.com/muutot/Clipboard/commit/929d63e8) [`7eeba18a`](https://github.com/muutot/Clipboard/commit/7eeba18a) [`b3bbe5c1`](https://github.com/muutot/Clipboard/commit/b3bbe5c1) [`3dcbc2bf`](https://github.com/muutot/Clipboard/commit/3dcbc2bf)
- **可靠性** — 从检查点恢复丢失历史、合并独立捕获的文本、验证指针后再 gc、防止远端 head 回退 | [`4bde51a4`](https://github.com/muutot/Clipboard/commit/4bde51a4) [`52679ec9`](https://github.com/muutot/Clipboard/commit/52679ec9) [`0a84fbe7`](https://github.com/muutot/Clipboard/commit/0a84fbe7) [`c00b1806`](https://github.com/muutot/Clipboard/commit/c00b1806)

## 搜索与导入

- **自然语言日期过滤** — 搜索支持今天 / 昨天 / 本周 / 本月等自然语言日期 | [`bb1c16d3`](https://github.com/muutot/Clipboard/commit/bb1c16d3)
- **CSV 导入** — 新增 CSV 格式导入 | [`afe3c7ac`](https://github.com/muutot/Clipboard/commit/afe3c7ac)

## 隐私

- **可配置敏感内容过滤** — 采集面板新增敏感内容设置 UI，敏感内容正则可配置并接入休眠字段 | [`94fb0152`](https://github.com/muutot/Clipboard/commit/94fb0152) [`49024598`](https://github.com/muutot/Clipboard/commit/49024598)

## 历史与排序

- **最近使用默认排序** — 默认排序改为最近使用，并在条目被使用时写入 last_used_at_ms | [`4ea1252a`](https://github.com/muutot/Clipboard/commit/4ea1252a) [`d23d0a6f`](https://github.com/muutot/Clipboard/commit/d23d0a6f)

## 设置与界面

- **通用 SettingEntry 组件** — 通用设置项组件，支持自定义槽位与文本动作、配置类型与尺寸工具 | [`46a39b7f`](https://github.com/muutot/Clipboard/commit/46a39b7f) [`1f0d02c5`](https://github.com/muutot/Clipboard/commit/1f0d02c5)
- **主题重置与预设编辑** — 新增深色 / 浅色重置按钮与内联预设编辑，默认强调色改为蓝 #2576f8 | [`60d618d4`](https://github.com/muutot/Clipboard/commit/60d618d4) [`f148d655`](https://github.com/muutot/Clipboard/commit/f148d655)
- **窗口透明度** — 新增窗口透明度文本跟随开关，磨砂玻璃效果更明显 | [`79010056`](https://github.com/muutot/Clipboard/commit/79010056) [`2ce05fdc`](https://github.com/muutot/Clipboard/commit/2ce05fdc)
- **标签颜色选择器** — 标签行新增颜色选择器弹层 | [`f0c32a98`](https://github.com/muutot/Clipboard/commit/f0c32a98)
- **采集入口独立化** — 剪贴板录制作为独立采集项，可在托盘 / 设置 / 标签列表间同步暂停状态 | [`de807ca8`](https://github.com/muutot/Clipboard/commit/de807ca8) [`9f84431c`](https://github.com/muutot/Clipboard/commit/9f84431c)

## 平台支持

- **Linux 剪贴板** — 通过 text/uri-list 采集 Linux 剪贴板文件路径，接通 Linux HTML / RTF 剪贴板读取 | [`b201d705`](https://github.com/muutot/Clipboard/commit/b201d705) [`2c7ecc1a`](https://github.com/muutot/Clipboard/commit/2c7ecc1a)

## 性能

- **同步流化与延迟** — 流化快照 / 检查点包、延迟远程资源下载、跳过未变 head 下载、预览保持设备本地 | [`6918bac0`](https://github.com/muutot/Clipboard/commit/6918bac0) [`1332ee9b`](https://github.com/muutot/Clipboard/commit/1332ee9b) [`26be2968`](https://github.com/muutot/Clipboard/commit/26be2968) [`51fe7009`](https://github.com/muutot/Clipboard/commit/51fe7009)
- **设置懒加载** — 次级面板懒加载、实时统计刷新按范围收敛 | [`e3cd4b4e`](https://github.com/muutot/Clipboard/commit/e3cd4b4e) [`5ffb77ed`](https://github.com/muutot/Clipboard/commit/5ffb77ed)

## 修复与健壮性

- **键盘快捷键** — 修复方向键 / 导航快捷键与搜索框聚焦时的条目快捷键 | [`3aaddfe7`](https://github.com/muutot/Clipboard/commit/3aaddfe7) [`8c285725`](https://github.com/muutot/Clipboard/commit/8c285725)
- **详情与主窗口** — 从索引与缓存条目解析详情查找，每次失效刷新历史分页 | [`9a1f04e0`](https://github.com/muutot/Clipboard/commit/9a1f04e0) [`3cf12e59`](https://github.com/muutot/Clipboard/commit/3cf12e59)
- **单实例** — 重启时等待旧进程退出 | [`8abc66eb`](https://github.com/muutot/Clipboard/commit/8abc66eb)
- **设置样式** — 对齐行下拉与共享图标 / 悬停样式，修正数值行溢出 | [`d335811e`](https://github.com/muutot/Clipboard/commit/d335811e) [`3757cd2a`](https://github.com/muutot/Clipboard/commit/3757cd2a) [`6e17c532`](https://github.com/muutot/Clipboard/commit/6e17c532)
- **发布流程** — 发布流程改为纯本地，绝不推送远程；发布前门禁收敛为 format + check + lint | [`7b833782`](https://github.com/muutot/Clipboard/commit/7b833782) [`54a4b6b3`](https://github.com/muutot/Clipboard/commit/54a4b6b3)

---

## 构建产物

- **Windows (x64)** — MSI + NSIS 安装包
- **macOS (Apple Silicon, arm64)** — DMG 安装包
- **Linux (x64)** — deb / rpm / AppImage
