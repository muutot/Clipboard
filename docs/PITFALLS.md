# Development Pitfalls

实际开发中踩过的坑，修改代码时务必对照检查。

## Svelte 5

### `$effect` 中条件分支的信号读取也会被追踪

`if (signal)` 即使结果为 `false`，也会将 `signal` 加入 effect 的依赖。改变该信号会重新触发 effect，导致状态被意外重置。

```typescript
// BUG: imageFullscreen 变为 true 时 effect 重跑，立刻关掉全屏
$effect(() => {
  if (item) {
    if (imageFullscreen) {
      closeImageFullscreen();
    }
    imageFullscreen = false;
  }
});

// FIX: 用 untrack 包裹不想追踪的读取
$effect(() => {
  if (item) {
    untrack(() => {
      if (imageFullscreen) {
        closeImageFullscreen();
      }
    });
    imageFullscreen = false;
  }
});
```

### `svelte:window` 上的 `stopPropagation()` 无法阻止同层 handler

父子组件各自用 `<svelte:window onkeydown>` 注册的 handler 都挂在 `window` 上，`stopPropagation()` 阻止的是 DOM 冒泡，对同一 target 上的其他 listener 无效。

```typescript
// BUG: 父组件的 Escape handler 先执行，直接隐藏窗口
// 子组件的 stopPropagation() 拦不住

// FIX: 父组件自己检查状态
if (event.key === "Escape") {
  if (!detailItem) {
    // 详情面板打开时不隐藏窗口
    getCurrentWindow().hide();
  }
}
```

### `$state` 是一次性快照，不会自动跟随 store 变化

```typescript
let s = $state($generalSettings); // 取的是当前值的副本
// 后续 generalSettings 变化不会更新 s
// 需要手动订阅: generalSettings.subscribe((v) => { s = v; })
```

### 集合状态用 `$state.raw` + 纯函数整体替换，不要放进深代理

`ClipboardItem` 是宽对象（含 `metadataJson`/`fileMeta`/`imageMeta` 等），主路由曾把同一条记录放进 `items`/`indexedItems`/`searchCache`/`detailItem` 四个 `$state` 数组。现在改为 `itemStore: ItemStore`（`$state.raw`）+ `byId: Map<id, item>` + 四个只存 id 的视图，`items` 等由 `$derived` 投影。

理由与实测：

- **深代理成本与身份陷阱**：`$state`（非 `.raw`）会把 `Map` 之外的普通对象递归包成 `Proxy`，宽记录每次读取都返回新代理，`{ ...item }` 快照与 `toBe` 同一性断言全部失效；这些记录还会被带进 `invoke` 载荷。`$state.raw` + 纯函数返回新 store，让 `Map` 保持普通对象、`$derived` 只在 store 引用变化时重算。
- **`Map` 不会被 `$state` 深度代理**：就地 `map.set()` 不会触发任何更新。要么用 `SvelteMap`，要么（本项目采用）整体替换 store 引用——所有 mutator 都是 `store => store` 形式，天然满足。
- **视图只存 id 之后，"四副本不一致"这个 bug 类别直接消失**：写一次记录，所有视图同帧生效；不变量（`byId` == 四视图并集）在 `item-store.test.ts` 里对每个操作断言。
- **新泄漏点：map 持有生命周期**。数组时代"某条记录没人引用"只是被 GC；改由 map 持有后，详情面板改指向另一条记录会把旧记录留在 map 里，必须显式 prune。`setDetailItem`/`removeItems`/`clearView` 都因此调用 `pruneUnreferenced`，400 步混合操作走查就是靠它抓到过这个泄漏。

### `$derived` 必须在组件/反应域内创建，且只能用 getter 暴露

```typescript
// BUG 1：组件外创建的 $derived 是 unowned，不再追踪依赖，读到的是旧值
let store = $state.raw(initial);
const history = $derived(getItems(store, "history"));
const view = { history }; // BUG 2 也在这里
// 之后 store 被替换，view.history 仍是旧数组

// FIX：derived 在组件 init 里创建，并且用 getter 转发
const history = $derived(getItems(store, "history"));
return {
  get history() {
    return history; // 每次读取都是 get(derived)
  },
};
```

两条都是实测踩到的：`{ history }` 会把 derived 的**当前值**拍成普通属性（不是 getter），store 替换后投影永久陈旧；`$derived` 若在组件外创建，则变成 unowned、不再订阅依赖，同样陈旧。回归由 `item-store-view.test.ts` 通过 `mount()` 真实组件守住——纯 `.test.ts` 里没有反应域，测不出这两类问题。

## Tauri 多窗口

### 每个窗口有独立的 JS 上下文

设置页面在独立的 `WebviewWindow` 中打开，两边各有自己的 `generalSettings` store 实例。设置窗口修改后只写入 localStorage，主窗口的 store 不会自动同步。

```typescript
// BUG: 主窗口永远读到旧值
get(generalSettings).imageFullscreenMode; // 启动时快照，不会变

// FIX 1: store 内部通过 storage 事件自动同步（已实现）
// FIX 2: 读取时始终从 store 取当前值
get(generalSettings).someSetting; // 调用时读取，不是模块顶层
```

### 新窗口必须登记 capability

Tauri 2 的 capability 按窗口授权：`src-tauri/capabilities/default.json` 的 `windows` 数组没写的新窗口，其窗口类 IPC（`close`/`setPosition`/`outerPosition`/`startDragging`/`scaleFactor` 等）会被静默拒绝——列表类 `invoke` 可能正常，但拖拽和关闭全部失灵，看起来像窗口“假死”。

```jsonc
// BUG: 悬浮窗能渲染列表，但拖不动、关不掉
"windows": ["main", "settings"],
// FIX: 新窗口 label 必须加进去，用到的窗口权限也要补齐
"windows": ["main", "settings", "float"],
```

新增窗口后对照该窗口用到的全部 `window.*` API 逐项核对 `core:window:allow-*` 权限。

### 条目级写操作必须广播，否则另一个窗口永远看不到

各 WebviewWindow 是独立 JS realm，**内存状态无法共享**。所以条目级 mutation（收藏、标签、行内编辑、改名、软删/恢复/永久删及批量版）如果不在后端 emit 事件，两个窗口就会各自显示不同的内容；而前端**无法自救**——它没有"按 id 重查"这种命令可供调用。

```rust
// BUG: 命令签名里没有 AppHandle，物理上无法 emit，悬浮窗收藏后主页不变
pub fn set_clipboard_item_favorite(
    database: tauri::State<'_, Database>, id: String, is_favorite: bool,
) -> Result<bool, String>

// FIX: 带上 AppHandle 并广播变更
pub fn set_clipboard_item_favorite(
    app: AppHandle, database: tauri::State<'_, Database>, id: String, is_favorite: bool,
) -> Result<bool, String> {
    let updated = database.set_favorite(&id, is_favorite).map_err(|e| e.to_string())?;
    if updated { broadcast_content_changed(&app, &database, std::slice::from_ref(&id)); }
    Ok(updated)
}
```

两个容易踩的细节：

- **`deleted` 不是记录上的列**。它是"这条记录由哪条查询返回"（`list_recent` vs `list_deleted`）推导出来的前端状态，存在接收方。所以软删/恢复**不能**用"记录已更新"表达，必须单独传 id 列表，否则另一个窗口会把已删行显示成未删。
- **自己收到自己的事件是安全的**。payload 是数据库里的行，等于本窗口已经乐观渲染的状态，重复应用无副作用——所以不需要按来源过滤。

### 无边框窗口的阴影内边距会让定位溢出工作区

Tauri 在 Windows 上为无边框（`decorations(false)`）窗口默认开启原生阴影。tao 创建窗口时会把阴影内边距（`calculate_insets_for_dpi`：左右下各 `SM_CXSIZEFRAME + SM_CXPADDEDBORDER`，Win11 顶部再加 1px）加到内尺寸上，实际外框比 `inner_size` 大一圈，而 builder 的 `position` 和 `set_position` 定位的都是**外框**左上角。用 `inner_size` 常量算右下角，会让面板越出工作区、压到任务栏或屏幕外（悬浮面板曾出现此问题）。

```rust
// BUG: 320x480 是内尺寸；外框更大，右下角溢出工作区
builder.position(area_right - 320.0, area_bottom - 480.0)

// FIX: 建窗后读实际外框尺寸，用物理坐标定位
let outer = window.outer_size()?;
window.set_position(PhysicalPosition::new(
    area_right - outer.width as i32,
    area_bottom - outer.height as i32,
))?;
```

`outer_size()` 已含阴影内边距，按它对齐能让整窗外沿恰好贴住工作区边缘。

### 图片预览全屏模式

桌面全屏模式 (`imageFullscreenMode === "desktop"`) 使用 `element.requestFullscreen()` 填满物理屏幕。全屏状态通过监听 `fullscreenchange` 事件同步，不能仅依赖本地布尔变量，因为用户可通过浏览器 ESC 退出全屏。全屏时仅右上角 X 按钮和 ESC 可关闭。

```typescript
// 正确做法
onMount(() => {
  function onFullscreenChange() {
    isNativeFullscreen = !!document.fullscreenElement;
  }
  document.addEventListener("fullscreenchange", onFullscreenChange);
  return () => document.removeEventListener("fullscreenchange", onFullscreenChange);
});
```

### Tauri 命令返回类型必须是 `Result`

```rust
// 正确
#[tauri::command]
fn get_items() -> Result<Vec<Item>, String> { ... }

// 错误: 编译不通过
#[tauri::command]
fn get_items() -> Vec<Item> { ... }
```

### Rust struct 的 JSON 序列化必须匹配前端

所有传给前端的 Rust struct 必须用 `#[serde(rename_all = "camelCase")]`，否则字段名不匹配（Rust 默认 snake_case）。

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardItem {
    content_hash: String,    // 前端收到 contentHash
    source_app: Option<String>, // 前端收到 sourceApp
}
```

### macOS 透明窗口与 Tauri 功能校验

旧 Tauri 2 的 `transparent()` 需要 `macos-private-api` 功能；Tauri 2.12.1 已取消这个门控。本项目将最低版本设为 2.12.1，保留原生透明能力并移除过时的平台配置开关。不要只在 target 依赖中声明此功能、同时在 macOS 配置里启用它：`tauri-build` 的 manifest 校验优先读取主依赖声明，会在 macOS 的普通 Cargo/Clippy 构建阶段报功能不匹配，Windows 门禁却仍然通过。

## CSS 层级

### z-index 分层已固定，不可打破

| z-index | 元素                    | 用途               |
| ------- | ----------------------- | ------------------ |
| 51      | `.detail-backdrop`      | 详情面板半透明背景 |
| 52      | `.detail-panel`         | 详情侧边栏         |
| 100     | `.image-viewer-overlay` | 全屏图片查看器     |
| 101     | `.viewer-close-btn` 等  | 全屏查看器内控件   |

全屏模式下 `.detail-panel.fullscreen` 和 `.detail-backdrop.fullscreen-backdrop` 设为 `display: none`。

## 事件处理

### `setTimeout(..., 0)` 延迟状态变更

按钮在可点击容器内部时，click 事件会冒泡到容器。延迟设置状态可以避免触发容器的新 handler：

```typescript
// 避免全屏按钮的 click 冒泡到刚挂载的 backdrop handler
setTimeout(() => {
  imageFullscreen = true;
}, 0);
```

### backdrop 关闭必须调用完整关闭函数

```typescript
// BUG: 跳过了 setFullscreen(false) 和状态重置
onclick={() => (imageFullscreen = false)}

// 正确
onclick={closeImageFullscreen}
```

## i18n

### 新增翻译键需要改三个文件

1. `src/lib/i18n/locales/en.ts` — 英文翻译
2. `src/lib/i18n/locales/zh-CN.ts` — 中文翻译
3. `src/lib/i18n/types.ts` — 类型定义

漏改 `types.ts` 不会报编译错误，但会失去类型检查。

### 翻译函数的参数用花括号

```typescript
// locales/en.ts
copySuccess: "Copied to clipboard",
recordCount: "{count} records",

// 使用
_t("status.recordCount", { count: items.length })
```

## 数据库

### v0 只在本次重构中清空，v1 之后必须迁移

`storage/schema.rs` 以 `PRAGMA user_version = 1` 作为一次性的干净基线。只有 `user_version = 0`/pre-v1 输入会在本次重构中事务性清空并建立 v1；不得为旧布局添加字段级迁移或 fallback reader。从 v1 开始，任何布局变化都必须提高 schema 版本，并在 `storage/migrations.rs` 注册恰好一个相邻迁移（v1→v2→v3）；完整迁移链在同一事务中执行，任一步失败都整体回滚。高于当前程序支持版本的数据库必须原样拒绝，不能降级或清空。

### 内容去重靠 `content_hash`

`clipboard_items` 表有 `UNIQUE (kind, content_hash)` 约束。新增 item 前必须计算 hash，否则插入会失败。

### `item_tags` 是 `metadata_json['tags']` 的派生索引

标签以 `clipboard_items.metadata_json['tags']` 为唯一数据源，`item_tags` 联结表仅面向活跃行镜像标签，用于标签过滤与计数走索引。任何写标签的路径（`set_tags`、`rename_tag`、`delete_tag`）都必须在同一事务内同步 `item_tags`；删除 item 由外键 `ON DELETE CASCADE` 自动清理。若新增写 `metadata_json` 标签的逻辑，务必同时维护 `item_tags`，避免二者失步。

### 触发器内部对同表的嵌套 UPDATE 会再次触发其他 AFTER 触发器

v1 的 `clipboard_items_sync_outbox_*` 在 `AFTER INSERT/UPDATE` 内维护版本列。该嵌套 UPDATE 会让所有同表无 WHEN 守卫的 AFTER 触发器再触发一次，产生重复的搜索/同步事件。

```sql
-- BUG: 无 WHEN 守卫，outbox 维护 UPDATE 会重复插入 search_outbox
CREATE TRIGGER clipboard_items_search_update
AFTER UPDATE ON clipboard_items
BEGIN
    INSERT INTO search_outbox ...;
END;

-- FIX: 用 WHEN 守卫排除 modified_at_ms 这类维护列
CREATE TRIGGER clipboard_items_search_update
AFTER UPDATE ON clipboard_items
WHEN OLD.title != NEW.title OR OLD.text_content IS NOT NEW.text_content OR ...
BEGIN
    INSERT INTO search_outbox ...;
END;
```

新增/修改同表 AFTER 触发器时，必须带上 WHEN 守卫，明确列出真正影响该触发器语义的列；同步 outbox 触发器的维护列更新不能再次产生搜索事件。递归触发器 pragma（`PRAGMA recursive_triggers`）默认关闭，但这只限制同表递归，不能替代 WHEN 守卫。

### 应用远端数据时，同步触发器会把收到的条目再广播回去（回声）

`clipboard_items_sync_outbox_*` 触发器在启用同步时会为本地写入生成 outbox 行。如果 v1 的快照/段应用没有在同一事务内设置 `sync_suppress_changelog=1`，接收端会把远端条目再次加入本机 outbox。

```sql
-- FIX: 触发器读取 sync_metadata 的抑制标记
CREATE TRIGGER clipboard_items_sync_outbox_insert
AFTER INSERT ON clipboard_items
WHEN NOT EXISTS (
    SELECT 1 FROM sync_metadata
    WHERE key = 'sync_suppress_changelog' AND value = '1'
)
BEGIN
    INSERT INTO sync_outbox ...;
END;
```

写入远端数据的代码路径（`apply_sync_snapshot`、`apply_sync_segment`）必须在**同一事务内**先置 `sync_suppress_changelog=1`、提交前再删除该标记。因为标记随事务回滚而回滚，崩溃也不会残留永久抑制；搜索触发器不读该标记，接收到的条目仍需进 `search_outbox` 以便索引。

## 平台专属代码是本地门禁的盲区

`platform/` 下的 Linux/macOS 实现整体挂在 `#[cfg(target_os = "linux")]` / `"macos"` 后面。**在 Windows 上跑 `npm run verify`（fmt + clippy + test + build）时它们根本不会被编译**，所以"本地三关全绿"不等于"代码能编过"。真实的门禁只有对应平台的 CI job。

实例：`v1.7.4` 分支从 `3006f94` 基线起 CI 一直红，而本地全绿——

```
error[E0658]: use of unstable library feature `int_roundings`
  --> src/platform/linux/x11/mod.rs:1047
   | offset_units += (nitems as i64).div_ceil(4);
```

（该文件现已按职责拆分为 `platform/linux/x11/` 下的多个子模块，`div_ceil` 那段现在位于 `clipboard.rs`；上面的日志按当时原样保留。）

`div_ceil` **只对无符号整数稳定**（1.73），这里先 `as i64` 变成有符号就落到 nightly 特性上。修掉编译错误后，clippy 才刚跑起来，又陆续暴露 3 个同族问题：`private_interfaces` ×2（`pub fn` 返回 `pub(crate)` 类型）、`manual_c_str_literals`、以及我第一版修法自己引入的 `unnecessary_min_or_max`（`nitems` 其实是 `u64`，clamp 是死代码）。

规则：

- 改 `#[cfg(target_os = ...)]` 后的代码，**必须**看对应平台的 CI job 结果再算通过；本地无法用 `cargo check --target x86_64-unknown-linux-gnu` 替代，因为 `cc-rs` 之类原生依赖需要交叉 C 工具链。
- 整数取整优先在**无符号域**做（`(n / d) + (n % d != 0) as u64` 或稳定的 `u64::div_ceil`），最后再 cast；不要为了对齐累加器类型而把无符号量先转成有符号。
- 用 C 字符串字面量 `c"NAME"` 时注意 `c_char` 在 aarch64 Linux 是无符号的，而手写 FFI 常声明 `*const i8`——保留 `.cast()`，别让"更干净"的写法破坏可移植性。
- 搬移或重命名这类模块时，全仓搜旧路径：只在 Linux/macOS 编译的调用点对本机不可见。实例：`platform/linux_x11.rs` 收成 `platform/linux/x11/mod.rs` 后，`platform/windows/monitor.rs` 的 `#[cfg(target_os = "linux")]` 事件监视分支仍写 `crate::platform::linux_x11::try_spawn_xfixes_monitor`，Linux 上直接 E0433（Wayland 同处一行之隔），本地三关依旧全绿。
- 拆分模块时，`pub(crate)` 的项**不能**用 `pub use` 重新导出：只在目标平台编译的那份代码会报 E0364（`is only public within the crate, and cannot be re-exported outside`），本机三关照旧全绿。改成 `pub(crate) use`，或把该项提到 `pub`。
- 把文件移进子目录后，文件里 `super::X` 指向的父级也跟着变了：`platform/ui.rs` 收成 `platform/ui/{mod,window,disk,tray}.rs` 之后，`window.rs` 里原本表示 `platform::Platform` 和 `platform::macos::objc` 的 `super::…` 变成了 `platform::ui::…`。两处都藏在 `#[cfg(target_os = "linux"/"macos")]` 分支里，Windows 编译看不见，Linux/macOS 直接 E0433/E0432。移动文件后要么改用 `crate::platform::…` 全路径，要么逐个确认 `super::` 的新父级。
- 把整块 Windows 代码拆成 `windows/{a,b,c}.rs` 时，子模块靠 `use super::*;` 取父模块的常量和助手；非 Windows 目标上这些项几乎全被 cfg 掉，父模块的 `use a::*;` 与子模块的 `use super::*;` 就都成了 unused import，再加上 `pub use` 里混进被 cfg 掉的函数（如 `foreground::get_foreground_app`），`-D warnings` 会在 Linux/macOS 上一并打红。这类模块要么给子模块加 `#![cfg_attr(not(target_os = "windows"), allow(unused_imports))]` 并注明原因，要么让子模块本身只在 Windows 编译。
- 同一个父模块下的**兄弟子模块**默认互相不可见：把 `a.rs` 拆成 `a/{b,c}.rs` 后，`b` 里私有的项在 `c` 中直接 E0603/E0425。跨文件共享的常量、结构体、函数要标 `pub(super)`（或从 `crate::...` 全路径引用）。实例：`cli/api.rs` 拆成 `cli/api/{mod,http,routes}.rs` 时，`http` 与 `routes` 互引的项都改成了 `pub(super)`；`LocalApiServer` 仍留在 `mod.rs`，所以 `cli` 的 `pub use api::LocalApiServer` 不受影响。

## CI 缓存会把第三方构建脚本指向不存在的路径

`ort-sys`（`oar-ocr` 的依赖）在构建脚本里下载 ONNX Runtime 预编译包，放进用户缓存目录（Linux `~/.cache/ort.pyke.io`、macOS `~/Library/Caches/ort.pyke.io`、Windows `%LOCALAPPDATA%\ort.pyke.io`），并把该目录的 **绝对路径** 写进 `cargo:rustc-link-search`。这段输出随 `target/` 进入 rust-cache，被引用的目录却不在缓存里。

结果：新 runner 恢复出一个「仍然新鲜」的构建脚本，链接时才失败，日志里没有任何 ort-sys 自己的说明——

```text
   Compiling ort-sys v2.0.0-rc.13
error: could not find native static library `onnxruntime`, perhaps an -L flag is missing?
error: could not compile `ort-sys` (lib) due to 1 previous error
```

而 `cargo fmt` / `cargo clippy`（不链接）与 Windows job（那份构建脚本恰好重跑）依旧全绿，很容易误判成平台专属代码的问题。`ci.yml` 与 `release.yml` 因此在构建前执行 `cargo clean -p ort-sys [--profile …]`，强制构建脚本重跑、重新下载或复用二进制。凡是「构建脚本把 `$HOME` 下的下载目录写进缓存输出」的依赖都适用同一处理。

## Rust 模块结构

添加新功能时按模块归属放置：

| 模块        | 职责                                |
| ----------- | ----------------------------------- |
| `storage/`  | 数据库 CRUD、schema、paths          |
| `search/`   | Tantivy 索引、查询、同步            |
| `ocr/`      | OCR 引擎、worker                    |
| `keyboard/` | 全局快捷键解析、注册、匹配          |
| `content/`  | 内容检测、缩略图生成                |
| `platform/` | 平台特定实现（Windows/macOS/Linux） |
| `config.rs` | 配置结构体、读写                    |
| `export/`   | 导入导出（JSON/CSV/TXT）            |
| `privacy/`  | 隐私管理（暂停录制、应用忽略）      |
| `domain/`   | 共享领域模型                        |
