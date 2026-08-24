# 小众 UI 样式待审清单

> 本文件只记录当前源码中局部、低频、尚未形成共识的 UI 样式与遗漏。它不是项目通用规范，也不是复制模板。除非任务明确要求审阅或修复这里的条目，否则先保持现状。

## 使用规则

- 新开发不得因为本文件记录了某种写法，就在别处复用它。
- 只有经过人工审阅并确认可推广的内容，才迁移到 `css-theming.md` 或 `settings-panels.md`。
- 修复时一次只处理一个可验证的小范围样式问题，避免顺手重构主界面。
- 条目状态统一为：`待审`、`接受为局部例外`、`计划统一`、`已处理`。
- 更新条目时以当前源码和实际渲染为证据，不以 TODO 的完成标记代替检查。

## 当前待审项

| 状态   | 范围                 | 当前证据                                                                                                                                                                     | 后续审阅问题                                                                  |
| ------ | -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| 待审   | 代码与 Markdown 预览 | `CodeEditor.svelte`、`CodePreview.svelte`、`MarkdownPreview.svelte` 使用一套固定深色和语法色                                                                                 | 这些颜色是否应始终保持编辑器主题，还是需要支持亮色/自定义主题？               |
| 待审   | 独立图片查看器       | `routes/viewer/+page.svelte` 的控制按钮、提示和遮罩使用固定 `#`/`rgba` 颜色                                                                                                  | 媒体查看器是否被定义为固定深色沉浸层；若是，应明确例外边界                    |
| 待审   | 详情图片全屏         | `DetailPanel.svelte` 的图片遮罩和全屏控件仍有固定黑白透明色                                                                                                                  | 是否与独立 viewer 共用一套“媒体层 token”，还是保持局部固定色                  |
| 待审   | 主界面分类色         | `+page.svelte` 的部分筛选图标、`ClipboardCard.svelte` 的 source-red/blue/violet 使用固定色                                                                                   | 这些是内容语义色还是主题色；是否需要独立语义变量                              |
| 待审   | 主界面阴影           | 搜索建议、下拉框、卡片对话框、快捷键帽等使用固定黑色透明阴影                                                                                                                 | 是否需要通用 shadow token；当前不要为了消除字面量而机械替换                   |
| 已处理 | 忽略应用面板         | `IgnoredAppsSettingsPanel.svelte` 已复用共享 header、scroll、feedback、auto-save、toggle 基础规则，隐私设置改用标准 `setting-card` 结构，仅保留传输面板布局和必要 modifier   | 新增共享基础样式时继续放入 `settings-shared.css`，不要在此面板重新复制        |
| 已处理 | 设置父容器           | `StorageSettingsDialog.svelte` 已复用共享 heading、scroll、slider、feedback、auto-save 等基础规则，仅保留内置模块的密度和输入 reset modifier                                 | 内置模块的新特例必须是 modifier，不得复制整套共享 primitive                   |
| 已处理 | 未定义变量           | 仓库不再引用未定义变量：次要按钮 hover 改用 `--hover-bg`（原 `--row-hover`），更新对话框改用 `--selection-color`（原 `--info-color`），忽略应用面板不再使用 `--text-inverse` | 保持语义变量唯一拼写；新增颜色一律走 ThemeColors 契约                         |
| 待审   | 非主题 fallback      | 侧栏用量条使用 `var(--accent-color, #746dff)`，仓库未定义 `--accent-color`                                                                                                   | 是否应统一到 `--accent` 或 `--selection-color`                                |
| 已处理 | 排序拖拽态           | `GeneralSettingsPanel.svelte` 的 `.sort-drag-over` 轮廓与背景均已改用 `--selection-color`（背景经 `color-mix` 15%）                                                          | 已统一到选中态语义色；如需独立“拖拽目标”语义再议                              |
| 待审   | Select 箭头          | 设置父容器用 data URI 内嵌 `#999` 绘制下拉箭头，部分子面板则只隐藏原生箭头                                                                                                   | 需要统一的可主题化下拉箭头与无障碍策略                                        |
| 待审   | 局部字号/圆角        | Keyboard、Theme、统计等低频控件仍有 9/10/11/12/14/16/17px 或 4/7px 等固定值（图标缓存面板已 token 化）                                                                       | 哪些属于真正的几何特例，哪些应映射到 `--settings-*`                           |
| 已处理 | 内联布局             | Keyboard 配置卡的内联样式已改为命名 class（`.keyboard-config-card`、`.config-bar-actions`，见 `StorageSettingsDialog.svelte`）                                               | 新布局样式一律使用命名 class 并放入正确设置层级                               |
| 待审   | Keyboard 层级        | Keyboard 配置操作卡目前位于 breadcrumb 行和二级导航之间                                                                                                                      | 是否移动到说明之后的内容区，以遵守统一 shell 顺序                             |
| 待审   | 设置页硬编码文案     | 设置左侧“采集”、`OCR`、“统计”和部分 Keyboard/OCR 说明直接写在组件中                                                                                                          | 是否全部接入 i18n；这属于 UX 一致性，不应成为样式模板                         |
| 待审   | 精简模式作用域       | `TODO.md` 仍记录“精简模式滑块同时影响设置窗口布局”                                                                                                                           | 需要真实运行设置窗口确认传播链路，再决定拆分主界面与设置界面变量              |
| 待审   | 主题完成声明         | TODO 中存在“组件硬编码颜色已全部替换”的完成描述，但源码扫描仍能看到上述局部固定色                                                                                            | 审阅时区分主题 UI、内容渲染、媒体遮罩和无障碍强制色，重新界定验收口径         |
| 待审   | AppIcon/组件计数文档 | 旧 reference 曾记录固定 icon/组件/服务数量，容易迅速失效                                                                                                                     | 继续只记录职责与 source symbol，不再维护脆弱数量                              |
| 待审   | 彩色图标模式         | `types/clipboard.ts::DEFAULT_ICON_COLORS` 为每个图标绑定默认十六进制色，`general.iconColors` 支持逐图标覆盖，仅在 `general.colorIcons` 开启时生效                            | 这些默认色是否应改为语义/主题色，还是保持为可选的品牌风格色板（现已可自定义） |
| 待审   | 实心按钮文字色       | `--selection-color` 实心背景上的文字使用字面量 `#fff`（`IconCacheSettingsPanel.svelte` 确认按钮、`UpdateDialog.svelte` 主按钮）                                              | 是否需要 on-accent 语义 token，还是接受固定白色                               |

## 暂时接受但不可推广的局部写法

以下写法可能有合理原因，未审阅前不要直接归类为缺陷：

- `app.css` 在 `forced-colors` 和 `prefers-contrast` 媒体查询中使用系统色或固定高对比色。
- 图片/视频类遮罩使用黑色透明层，以确保任意媒体内容上的控制可见。
- 代码语法 token 使用固定语义色，以维持代码可读性。
- 圆形旋钮、胶囊 badge、全屏遮罩等使用 `50%`/`999px`，这类几何值不一定适合设置圆角 token。
- 主题色预览 swatch 通过内联 `background-color` 展示用户选择的实际颜色。
- slider 的 `--slider-pct`、磁盘用量的动态 width 等数据驱动内联 style。

## 后续人工审阅建议

每次只选择一个表格条目，并按以下顺序审阅：

1. 在默认暗色、亮色、自定义主题中截图或运行对比。
2. 确认它属于通用 UI、设置 UI、媒体/内容渲染，还是无障碍例外。
3. 若可推广，先定义语义与所有使用者，再更新通用 reference。
4. 若只接受为局部例外，在表格中写明理由和边界。
5. 若计划统一，拆成独立最小提交，补静态检查和视觉验证。
6. 完成后将状态改为 `已处理`，附上新的 source symbol 或测试/截图证据；不要只写“看起来已统一”。

## 新发现的记录模板

```markdown
| 待审 | <页面/组件> | <直接源码或运行证据> | <需要人工决定的问题> |
```

不要把临时设计想法、一次性任务说明或未经验证的猜测写进本文件。
