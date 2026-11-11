# 依赖公告可达性复核

复核日期：2026-11-08。证据基线：应用源码、package-lock.json 与 2a43b4df 之前构建的静态前端。后续依赖或部署模式改变时必须重查。

## npm

全量 npm audit 仍报告 4 个公告、5 个 Low 节点。安装版本为 SvelteKit 2.70.3、adapter-static 3.0.10、vite-plugin-svelte 7.3.0、cookie 0.6.0、deepmerge 4.3.1。报告提出 SvelteKit 3.1.1 / adapter-static 4.0.1 的跨主版本升级；不能将其作为无兼容风险的自动修复。

| 公告                | 当前版本适用                     | 入口与当前证据                                                                                                                              | 处置                                                      |
| ------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| GHSA-pfhg-q3qv-9q6r | SvelteKit 位于报告的 <3.0.0 范围 | 应用没有 `$app/forms` enhance 或服务端表单 action。生产为静态 SPA，未发现剪贴板输入到 enhanced form submission 的链路                       | 不升级为已确认桌面漏洞；保留框架维护事项                  |
| GHSA-27mw-82g8-phjj | SvelteKit 位于报告的 <3.0.0 范围 | 没有 server 路由或外部 prerender header 输入；配置为 adapter-static fallback，根 layout 禁用 SSR                                            | 不存在已证明的当前利用入口；新增预渲染数据源时复查        |
| GHSA-pxg6-pf52-xh8x | cookie 0.6.0 位于 <0.7.0 范围    | cookie 由 Kit 引入；其序列化调用在 runtime/server/cookie.js，当前项目无服务端 cookies.set 输入或生产 Node 服务                              | 不把本地 API 的 Rust HTTP cookie 行为与此 JS 包混为一谈   |
| GHSA-gcqx-w48m-w2h4 | deepmerge 4.3.1 位于报告范围     | vite-plugin-svelte/src/utils/options.js 的 mergeConfigs 合并仓库 Vite/Svelte/预处理配置；剪贴板、导入记录或同步数据没有进入该构建配置的链路 | 属于开发/构建依赖的维护范围；接受不可信构建配置会改变结论 |

`npm audit --omit=dev` 报告 0 个公告，但这不能单独证明前端安全：开发依赖可以生成或进入发布 bundle。本轮另外检查应用路由/调用和构建结果。生成客户端中的 cache-control 命中是对 `/_app/version.json` 的固定 no-cache 请求头，不是接受外部 prerender Cache-Control 响应头。未发现 enhance/deepmerge/服务端 cookie 序列化被产品输入调用。

验证依据：`vite.config.js`（SvelteKit 3 起配置经 `sveltekit()` 插件传入，`svelte.config.js` 已移除）、`src/routes/+layout.ts`，src 下无 `+server`、`+page.server`、`+layout.server`。没有因这些公告修改 CSP、禁用现有功能或强制跨主版本更新。未来新增服务器部署、enhance、cookies 或外部构建配置后，上述不可达结论不再自动成立。

> 2026-11-11 更新：SvelteKit 已升级至 3.2.0、adapter-static 4.0.1（9a15bad2），报告建议的跨主版本升级已实施；上述 4 条 npm 公告的不可达结论随升级自然满足（SvelteKit ≥3.0.0）。cookie 随 Kit 3 升至 2.0.2（GHSA-pxg6-pf52-xh8x 的 <0.7.0 范围不再适用）；deepmerge 4.3.1 未变，仍属传递依赖维护事项。

本地原始扫描记录保存在忽略目录 .agents/skills/code-auditor/result/v1.8.4-review/npm-audit-current.json 与 npm-audit-production.json。此文保留可分发的判断依据，不依赖日志长期存在。

## Rust

使用隔离审计工具目录中的 cargo-audit 0.22.2，RustSec 数据库提交 309392b18f6b17022f82c701f300433126901068（2026-11-06），扫描 764 个锁文件依赖，最初报告 7 条漏洞记录及 16 条 informational 警告。

zstd 0.13.3 与 zstd-safe 7.3.0 分别有 RUSTSEC-2026-0391、0398、0403 以及 10 条 unsound 公告。当前同步通过 fresh encode_all / stream::read::Decoder 使用独立上下文，未证明跨上下文泄漏或 write::Decoder 利用链；但存在兼容补丁，已提高同步依赖下限至 zstd 0.13.4 并锁定 zstd-safe 7.4.0。更新后同一数据库扫描已消除全部 zstd 公告，19 项同步 wire 测试通过。

剩余 ZIP 8.6.0 公告为 RUSTSEC-2026-0367、0368、0369、0429，修复范围为 >=9.0.0。问题 API 为 ZipArchive::extract / ZipStreamReader::extract 的权限、符号链接或进程中止行为。当前备份逐项使用 by_index、受控名称、拒绝重复及受限 transfer，不调用上述通用 extract，也不恢复 ZIP Unix 权限；未证明当前入口可利用。跨主版本 ZIP 更新需要单独验证，不能静默标为已修复。

剩余 informational：bincode 2.0.1、paste 1.0.15、proc-macro-error 1.0.4、ttf-parser 0.25.1 为停止维护公告；glib 0.18.5（RUSTSEC-2024-0429）和 lru 0.16.4（RUSTSEC-2026-0253）为 unsound 警告。维护状态不是已证明攻击；glib 是 Linux 平台依赖，当前 Windows 检查不覆盖其运行；lru 的 panic/pop 入口仍需逐个追踪。不得通过忽略列表让扫描变绿。

原始扫描保存为本地 cargo-audit-current.json 与 cargo-audit-zstd-patched.json。Rust 检查不是无风险结论；上述剩余问题保留后续维护范围。

## 本轮收尾复核

在 67a1723a 的锁文件上重新运行 cargo-audit 0.22.2，同一 RustSec 数据库扫描 752 个依赖：4 条 ZIP 漏洞记录、4 条停止维护公告、glib/lru 两条 unsound 警告；无 zstd 公告。此数量为本次扫描结果，不沿用历史 764 个依赖计数，也不代表公告已全部修复。

`cargo tree -e normal -i zip/lru` 确认 ZIP 为应用直接普通依赖，lru 经 Tantivy 0.26.2 引入，均不是仅测试依赖。Tantivy 的 `store/reader.rs::BlockCache` 使用 `LruCache<usize, Block>`，当前调用为 get、put、len、peek_lru；未见 pop 调用。RUSTSEC-2026-0253 所述条件为 pop 时键的 Drop panic、捕获 unwind 后继续使用缓存。当前 usize 键没有这样的析构实现，因此这条实际缓存路径不满足公告触发条件；保留依赖维护事项，后续新增使用点或缓存键类型变化必须复查。

glib 是 Linux 原生界面依赖，当前 Windows 环境不能证明其原生行为。尝试离线解析 Linux 目标依赖树因本机缺少 openssl 0.10.82 缓存失败；不将该失败解释为编译错误或平台验证通过。没有新增公告忽略项或强制跨主版本升级。

本地扫描结果保存于 `.agents/skills/code-auditor/result/v1.8.4-review/cargo-audit-final.json`（忽略目录）。
