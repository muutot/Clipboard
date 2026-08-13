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
