export const SETTINGS_SECTIONS = [
  "general_search",
  "general_items",
  "general_window",
  "general_general",
  "compact",
  "font",
  "theme",
  "icons",
  "capture",
  "capture_icons",
  "storage_paths",
  "storage_limits",
  "storage_tools",
  "sync_cloud",
  "sync_advanced",
  "keyboard_item",
  "keyboard_quick",
  "keyboard_system",
  "tags",
  "ocr",
  "statistics",
  "about",
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export const STATISTICS_TABS = ["storage", "performance", "memory"] as const;

export type StatisticsTab = (typeof STATISTICS_TABS)[number];

export interface SettingsSearchTarget {
  section: SettingsSection;
  statisticsTab?: StatisticsTab;
}

export interface SettingsSearchI18nText {
  key: string;
  fallback?: string;
}

export type SettingsSearchText = string | SettingsSearchI18nText;

export interface SettingsSearchItemTemplate extends SettingsSearchTarget {
  id: string;
  title: SettingsSearchText;
  description?: SettingsSearchText;
  aliases?: readonly SettingsSearchText[];
}

export interface SettingsSearchItem extends SettingsSearchTarget {
  id: string;
  title: string;
  description: string;
  aliases: readonly string[];
  searchableText: string;
}

export type SettingsSearchTranslate = (key: string) => string;

const i18n = (key: string, fallback?: string): SettingsSearchI18nText => ({ key, fallback });

const entry = (
  id: string,
  target: SettingsSearchTarget,
  title: SettingsSearchText,
  description?: SettingsSearchText,
  aliases?: readonly SettingsSearchText[],
): SettingsSearchItemTemplate => ({ id, ...target, title, description, aliases });

const SECTION_SEARCH_TEXT: Record<SettingsSection, readonly SettingsSearchText[]> = {
  general_search: [i18n("storage.generalTab"), i18n("storage.generalSearchTab"), "搜索"],
  general_items: [
    i18n("storage.generalTab"),
    i18n("storage.generalItemsTab"),
    "条目",
    "回收站",
    "置顶",
    "操作按钮",
  ],
  general_window: [i18n("storage.generalTab"), i18n("storage.generalWindowTab"), "窗口"],
  general_general: [
    i18n("storage.generalTab"),
    i18n("storage.generalGeneralTab"),
    "语言",
    "通知",
    "剪切板记录",
    "暂停",
  ],
  compact: [
    i18n("storage.appearanceTab"),
    i18n("storage.compactTab"),
    i18n("compact.title"),
    "布局密度",
  ],
  font: [i18n("storage.appearanceTab"), i18n("storage.fontTab"), i18n("general.fontSize"), "显示"],
  theme: [i18n("storage.appearanceTab"), i18n("storage.themeTab"), i18n("theme.title"), "配色"],
  icons: [
    i18n("storage.appearanceTab"),
    i18n("storage.iconsTab"),
    i18n("general.colorIcons"),
    "图标",
    "彩色",
    "color",
  ],
  capture: [i18n("capture.title"), i18n("capture.settings"), "采集"],
  capture_icons: [
    i18n("capture.title"),
    i18n("capture.settings"),
    i18n("storage.iconCacheTitle"),
    i18n("storage.iconCacheDesc"),
    "图标",
    "采集",
  ],
  storage_paths: [i18n("storage.storageTab"), i18n("storage.storagePathsTab"), "存储", "路径"],
  storage_limits: [
    i18n("storage.storageTab"),
    i18n("storage.storageLimitsTab"),
    "存储",
    "容量",
    "清理",
  ],
  storage_tools: [
    i18n("storage.storageTab"),
    i18n("storage.storageToolsTab"),
    "存储",
    "维护",
    "工具",
  ],
  sync_cloud: [
    i18n("storage.syncCloudTab"),
    i18n("storage.syncTab"),
    i18n("storage.syncTitle"),
    "云端",
    "连接",
    "webdav",
    "backup",
    "加密",
    "密码",
  ],
  sync_advanced: [i18n("storage.syncAdvancedTab"), "高级设置", "滚动", "阈值", "资源限制"],
  keyboard_item: [i18n("storage.keyboardTab"), i18n("storage.keyboardItemTab"), "快捷键", "條目"],
  keyboard_quick: [
    i18n("storage.keyboardTab"),
    i18n("storage.keyboardQuickTab"),
    "快捷键",
    "快速复制",
  ],
  keyboard_system: [
    i18n("storage.keyboardTab"),
    i18n("storage.keyboardSystemTab"),
    "快捷键",
    "全局",
    "系统",
  ],
  ocr: [i18n("storage.ocrTitle"), "OCR", "文字识别"],
  tags: [i18n("storage.tagsTab"), i18n("tags.title"), "标签", "tag"],
  statistics: [i18n("statistics.title"), "统计", "诊断"],
  about: [i18n("about.tabLabel"), i18n("about.title"), "更新", "版本", "update", "version"],
};

const STATISTICS_TAB_SEARCH_TEXT: Record<StatisticsTab, readonly SettingsSearchText[]> = {
  storage: [i18n("statistics.storageTab"), i18n("statistics.storageDescription")],
  performance: [i18n("statistics.performanceTab"), i18n("statistics.performanceDescription")],
  memory: [i18n("statistics.memoryTab"), i18n("statistics.memoryDescription")],
};

export const SETTINGS_SEARCH_ITEM_TEMPLATES: readonly SettingsSearchItemTemplate[] = [
  entry(
    "general.language",
    { section: "general_general" },
    i18n("general.language"),
    i18n("general.languageDescription"),
    ["中文", "English", "locale"],
  ),
  entry(
    "general.search-suggestion-mode",
    { section: "general_search" },
    i18n("general.searchSuggestionMode"),
    i18n("general.searchSuggestionModeDescription"),
    [
      i18n("general.searchSuggestionOff"),
      i18n("general.searchSuggestionPanel"),
      i18n("general.searchSuggestionInline"),
      "inline",
    ],
  ),
  entry(
    "general.card-actions-display",
    { section: "general_items" },
    i18n("general.cardActionsDisplay"),
    i18n("general.cardActionsDisplayDescription"),
    [i18n("general.cardActionsHover"), i18n("general.cardActionsAlways"), "操作按钮", "actions"],
  ),
  entry(
    "general.quick-copy-badge",
    { section: "general_items" },
    i18n("general.quickCopyBadge"),
    i18n("general.quickCopyBadgeDescription"),
    ["#N", "快速复制", "quick copy", "badge"],
  ),
  entry(
    "general.search-history",
    { section: "general_search" },
    i18n("general.searchHistory"),
    i18n("general.searchHistoryDescription"),
    ["搜索历史", "recent search"],
  ),
  entry(
    "general.search-placeholder",
    { section: "general_search" },
    i18n("general.searchPlaceholder"),
    i18n("general.searchPlaceholderDescription"),
    ["提示文案", "占位", "placeholder", "hint"],
  ),
  entry(
    "general.search-index-sync-mode",
    { section: "general_search" },
    i18n("general.searchIndexSyncMode"),
    i18n("general.searchIndexSyncModeDescription"),
    [
      i18n("general.searchIndexSyncModeLazy"),
      i18n("general.searchIndexSyncModeBackground"),
      "索引同步",
      "index sync",
      "outbox",
    ],
  ),
  entry(
    "general.launch-at-startup",
    { section: "general_general" },
    i18n("general.launchAtStartup"),
    i18n("general.launchAtStartupDescription"),
    ["自动启动", "boot", "login"],
  ),
  entry(
    "general.close-to-tray",
    { section: "general_general" },
    i18n("general.closeToTray"),
    i18n("general.closeToTrayDescription"),
    ["托盘", "tray"],
  ),
  entry(
    "general.window-transparency",
    { section: "general_window" },
    i18n("general.windowTransparency"),
    i18n("general.windowTransparencyDescription"),
    ["透明", "opacity"],
  ),
  entry(
    "general.window-effect",
    { section: "general_window" },
    i18n("general.windowEffect"),
    i18n("general.windowEffectDescription"),
    ["毛玻璃", "玻璃", "acrylic", "mica", "frosted", "blur"],
  ),
  entry(
    "general.compact-mode",
    { section: "compact" },
    i18n("general.compactMode"),
    i18n("general.compactModeDescription"),
    ["密度", "compact"],
  ),
  entry(
    "general.pin-copied-to-top",
    { section: "general_items" },
    i18n("general.pinCopiedToTop"),
    i18n("general.pinCopiedToTopDescription"),
    ["复制置顶", "move copied item"],
  ),
  entry(
    "general.recycle-bin",
    { section: "general_general" },
    i18n("general.useRecycleBin"),
    i18n("general.useRecycleBinDescription"),
    ["删除", "trash"],
  ),
  entry(
    "general.paste-cleaning",
    { section: "general_items" },
    i18n("general.pasteCleaning"),
    i18n("general.pasteCleaningDescription"),
    ["清洗", "clean", "粘贴"],
  ),
  entry(
    "general.double-click-paste",
    { section: "general_items" },
    i18n("general.doubleClickPaste"),
    i18n("general.doubleClickPasteDescription"),
    ["双击", "粘贴", "double click", "paste", "详情"],
  ),
  entry(
    "general.max-text-capture-size",
    { section: "storage_limits" },
    i18n("general.maxTextCaptureSize"),
    i18n("general.maxTextCaptureSizeDescription"),
    ["捕获", "文本", "纯文本", "上限", "大小", "长度", "capture", "size", "limit"],
  ),
  entry(
    "general.toast-notifications",
    { section: "general_general" },
    i18n("general.toastNotifications"),
    i18n("general.toastNotificationsDescription"),
    ["提示", "toast"],
  ),
  entry(
    "general.system-title-bar",
    { section: "general_general" },
    i18n("general.useSystemTitleBar"),
    i18n("general.useSystemTitleBarDescription"),
    ["标题栏", "titlebar"],
  ),
  entry(
    "general.settings-close-button",
    { section: "general_general" },
    i18n("general.showSettingsCloseButton"),
    i18n("general.showSettingsCloseButtonDescription"),
    ["关闭按钮", "Esc", "settings close"],
  ),
  entry(
    "general.desktop-fullscreen",
    { section: "general_window" },
    i18n("general.desktopFullscreen"),
    i18n("general.desktopFullscreenDescription"),
    ["图片预览", "fullscreen"],
  ),
  entry(
    "general.viewer-backdrop-opacity",
    { section: "general_window" },
    i18n("general.viewerBackdropOpacity"),
    i18n("general.viewerBackdropOpacityDescription"),
    ["蒙版", "backdrop"],
  ),
  entry(
    "general.remember-window-position",
    { section: "general_window" },
    i18n("general.rememberWindowPosition"),
    i18n("general.rememberWindowPositionDescription"),
    ["窗口位置", "window bounds"],
  ),
  entry(
    "general.detail-display-mode",
    { section: "general_window" },
    i18n("general.detailDisplayMode"),
    i18n("general.detailDisplayModeDescription"),
    [
      i18n("general.detailDisplayModeOverlay"),
      i18n("general.detailDisplayModeSplit"),
      "详情面板",
      "detail panel",
    ],
  ),
  entry(
    "general.show-secondary-text",
    { section: "general_items" },
    i18n("general.showSecondaryText"),
    i18n("general.showSecondaryTextDescription"),
    ["辅助文字", "preview", "secondary"],
  ),
  entry(
    "general.color-icons",
    { section: "icons" },
    i18n("general.colorIcons"),
    i18n("general.colorIconsDescription"),
    ["彩色", "彩色图标", "单色", "图标颜色", "icons", "color"],
  ),
  entry(
    "general.icon-colors",
    { section: "icons" },
    i18n("general.iconColors"),
    i18n("general.iconColorsDescription"),
    ["图标颜色", "每图标", "icon colors", "palette", "color"],
  ),
  entry(
    "general.max-text-lines",
    { section: "general_items" },
    i18n("general.maxTextLines"),
    i18n("general.maxTextLinesDescription"),
    ["行数", "lines", "preview"],
  ),
  entry(
    "general.theme",
    { section: "theme" },
    i18n("general.theme"),
    i18n("general.themeDescription"),
    [i18n("general.themeDark"), i18n("general.themeLight"), "外观"],
  ),

  entry(
    "compact.enabled",
    { section: "compact" },
    i18n("general.compactMode"),
    i18n("general.compactModeDescription"),
  ),
  entry(
    "compact.padding-top",
    { section: "compact" },
    i18n("compact.paddingTop"),
    i18n("compact.paddingTopDescription"),
  ),
  entry(
    "compact.padding-bottom",
    { section: "compact" },
    i18n("compact.paddingBottom"),
    i18n("compact.paddingBottomDescription"),
  ),
  entry(
    "compact.card-gap",
    { section: "compact" },
    i18n("compact.cardGap"),
    i18n("compact.cardGapDescription"),
  ),
  entry(
    "compact.short-text-height",
    { section: "compact" },
    i18n("compact.shortTextHeight"),
    i18n("compact.shortTextHeightDescription"),
  ),
  entry(
    "compact.tall-text-height",
    { section: "compact" },
    i18n("compact.tallTextHeight"),
    i18n("compact.tallTextHeightDescription"),
  ),
  entry(
    "compact.image-height",
    { section: "compact" },
    i18n("compact.imageHeight"),
    i18n("compact.imageHeightDescription"),
  ),
  entry(
    "compact.search-height",
    { section: "compact" },
    i18n("compact.searchHeight"),
    i18n("compact.searchHeightDescription"),
  ),
  entry(
    "compact.search-font-size",
    { section: "compact" },
    i18n("compact.searchFontSize"),
    i18n("compact.searchFontSizeDescription"),
  ),
  entry(
    "compact.card-border-radius",
    { section: "compact" },
    i18n("compact.cardBorderRadius"),
    i18n("compact.cardBorderRadiusDescription"),
    ["圆角", "radius"],
  ),

  entry(
    "font.base",
    { section: "font" },
    i18n("general.fontSizeBaseLabel"),
    i18n("general.fontSizeBaseDescription"),
    ["界面基础", "base font", "基础字号"],
  ),
  entry(
    "font.secondary",
    { section: "font" },
    i18n("general.fontSizeSecondaryLabel"),
    i18n("general.fontSizeSecondaryDescription"),
    ["描述文字", "secondary font", "描述字号"],
  ),
  entry(
    "font.tiny",
    { section: "font" },
    i18n("general.fontSizeTinyLabel"),
    i18n("general.fontSizeTinyDescription"),
    ["备注文字", "tiny font", "备注字号"],
  ),
  entry(
    "font.card-title",
    { section: "font" },
    i18n("general.fontSizeCardTitleLabel"),
    i18n("general.fontSizeCardTitleDescription"),
    ["条目标题", "card title font", "标题字号"],
  ),
  entry(
    "font.card-preview",
    { section: "font" },
    i18n("general.fontSizeCardPreviewLabel"),
    i18n("general.fontSizeCardPreviewDescription"),
    ["条目辅助文字", "card preview font", "辅助字号"],
  ),

  entry(
    "recording.pause",
    { section: "general_general" },
    i18n("capture.pauseTitle"),
    i18n("capture.pauseDescription"),
    [i18n("capture.pauseAction"), i18n("capture.resumeAction")],
  ),
  entry(
    "capture.ignored-applications",
    { section: "capture" },
    i18n("capture.title"),
    i18n("capture.description"),
    [
      i18n("capture.availableApps"),
      i18n("capture.ignoredApps"),
      i18n("capture.addManual"),
      "应用过滤",
    ],
  ),
  entry(
    "capture.icon-cache",
    { section: "capture_icons" },
    i18n("storage.iconCacheTitle"),
    i18n("storage.iconCacheDesc"),
    [i18n("storage.manageIconCache"), i18n("storage.replaceIcon"), "图标缓存", "替换图标"],
  ),

  entry(
    "storage.config-file",
    { section: "storage_paths" },
    i18n("storage.configSectionTitle", "常规配置文件"),
    i18n("storage.configSectionDesc"),
    ["conf.json", "配置路径"],
  ),
  entry(
    "storage.data-directory",
    { section: "storage_paths" },
    i18n("storage.dataDirectoryTitle"),
    i18n("storage.dataDirectoryDesc"),
    ["storage path", "自定义目录"],
  ),
  entry(
    "storage.resource-directories",
    { section: "storage_paths" },
    i18n("storage.resourcePathsTitle"),
    i18n("storage.resourcePathsDesc"),
    [i18n("storage.imageStoragePath"), i18n("storage.fileStoragePath"), "图片目录"],
  ),
  entry(
    "storage.directory-tree",
    { section: "storage_paths" },
    i18n("storage.directoryTreeTitle"),
    i18n("storage.directoryTreeDesc"),
    ["目录结构", "文件布局"],
  ),
  entry(
    "storage.search-index",
    { section: "storage_tools" },
    i18n("storage.searchIndexTitle"),
    i18n("storage.searchIndexDesc"),
    [i18n("storage.rebuildIndex"), "Tantivy", "索引重建"],
  ),
  entry(
    "storage.retention-period",
    { section: "storage_limits" },
    i18n("captureSettings.retentionPeriod"),
    i18n("captureSettings.retentionPeriodDesc"),
    ["历史保留", "retention"],
  ),
  entry(
    "storage.max-item-count",
    { section: "storage_limits" },
    i18n("captureSettings.maxItemCount"),
    i18n("captureSettings.maxItemCountDesc"),
    ["容量上限", "history limit"],
  ),
  entry(
    "storage.recycle-bin-days",
    { section: "storage_limits" },
    i18n("captureSettings.recycleBinDays"),
    i18n("captureSettings.recycleBinDaysDesc"),
    ["回收站保留", "trash retention"],
  ),
  entry(
    "storage.max-file-copy-size",
    { section: "storage_limits" },
    i18n("captureSettings.maxFileCopySize"),
    i18n("captureSettings.maxFileCopySizeDesc"),
    ["文件大小", "copy limit"],
  ),
  entry(
    "storage.delete-by-kind",
    { section: "storage_limits" },
    i18n("storage.deleteByKindTitle"),
    i18n("storage.deleteByKindDesc"),
    [
      i18n("filter.text"),
      i18n("filter.link"),
      i18n("filter.image"),
      i18n("filter.file"),
      "分类删除",
      "permanent delete",
    ],
  ),
  entry(
    "storage.database-maintenance",
    { section: "storage_tools" },
    "数据库维护",
    "检查数据库完整性并尝试修复",
    ["修复数据库", "SQLite", "integrity"],
  ),
  entry(
    "storage.transfer",
    { section: "storage_tools" },
    i18n("storage.transferTitle"),
    i18n("storage.transferDesc"),
    [
      i18n("storage.exportLabel"),
      i18n("storage.importLabel"),
      i18n("storage.exportAction"),
      i18n("storage.importAction"),
      i18n("storage.exportIncludeFavorites"),
      i18n("storage.exportFavorites"),
      i18n("storage.exportContentTypes"),
      i18n("storage.exportDateRange"),
      "备份",
      "JSON",
      "CSV",
      "PPaste",
      "Plain Text",
      "收藏",
      "内容类型",
      "日期范围",
      "favorites",
      "content type",
      "date range",
      "backup",
      "plain text",
    ],
  ),

  entry("about.update", { section: "about" }, i18n("about.updateTitle"), i18n("about.updateDesc"), [
    "检查更新",
    "升级",
    "版本",
    "update",
    "upgrade",
    i18n("about.updateSourceGithub"),
    i18n("about.updateSourceGitcode"),
    "更新来源",
    "source",
  ]),
  entry("about.info", { section: "about" }, i18n("about.sectionTitle"), i18n("about.description"), [
    "版本",
    "version",
    "关于",
    "程序位置",
    "位置",
    "location",
    "executable",
  ]),

  entry(
    "keyboard.config-file",
    { section: "keyboard_item" },
    i18n("keyboard.shortcutConfigTitle"),
    i18n("keyboard.shortcutConfigDesc"),
    ["keyboard.json", "配置文件"],
  ),
  entry(
    "keyboard.copy-item",
    { section: "keyboard_item" },
    i18n("keyboard.copyItem"),
    "设置复制当前选中条目到剪贴板的快捷键",
    ["Ctrl+C", "copy"],
  ),
  entry(
    "keyboard.delete-item",
    { section: "keyboard_item" },
    i18n("keyboard.deleteItem"),
    "设置删除当前选中条目的快捷键",
    ["Ctrl+D", "delete"],
  ),
  entry(
    "keyboard.favorite-item",
    { section: "keyboard_item" },
    i18n("keyboard.favoriteItem"),
    "设置收藏或取消收藏当前条目的快捷键",
    ["Ctrl+F", "favorite"],
  ),
  entry(
    "keyboard.open-detail",
    { section: "keyboard_item" },
    i18n("keyboard.openDetail"),
    "设置预览当前条目详情面板的快捷键",
    ["Ctrl+E", "Space", "view", "详情"],
  ),
  entry(
    "keyboard.select-all",
    { section: "keyboard_item" },
    i18n("keyboard.selectAll"),
    "设置全选列表中所有条目的快捷键",
    ["Ctrl+A", "select"],
  ),
  entry(
    "keyboard.quick-paste",
    { section: "keyboard_item" },
    i18n("keyboard.quickPaste"),
    "设置将当前条目快速粘贴到上一个活跃窗口的快捷键",
    ["quickPaste", "快速粘贴"],
  ),
  entry(
    "keyboard.toggle-window",
    { section: "keyboard_system" },
    i18n("keyboard.toggleWindow"),
    "设置唤起或隐藏主窗口的全局快捷键",
    ["Alt+V", "toggle", "热键"],
  ),
  entry(
    "keyboard.quick-copy",
    { section: "keyboard_quick" },
    "快速复制第 N 条",
    "设置快速复制列表前 9 个条目的快捷键，默认 Ctrl+1~9",
    ["Ctrl+1", "quickCopy", "快速复制"],
  ),

  entry(
    "tags.manage",
    { section: "tags" },
    i18n("storage.tagsSectionTitle"),
    i18n("storage.tagsDescription"),
    [i18n("storage.tagsTab"), i18n("tags.title"), "标签管理", "重命名", "删除", "颜色"],
  ),

  entry(
    "ocr.engine",
    { section: "ocr" },
    "OCR 引擎",
    "在 PP-OCRv6 与 Tesseract 之间切换文字识别引擎",
    ["ppocr", "tesseract", "文字识别"],
  ),
  entry(
    "ocr.model",
    { section: "ocr" },
    "OCR 模型",
    "下载、安装并应用 tiny、small 或 medium 模型",
    ["模型规格", "download", "tiny", "small", "medium"],
  ),
  entry(
    "ocr.score-threshold",
    { section: "ocr" },
    "分数阈值 (score)",
    "调整文本区域检测分数，控制候选区域数量",
    ["检测参数", "score threshold"],
  ),
  entry(
    "ocr.box-threshold",
    { section: "ocr" },
    "框阈值 (box)",
    "调整文本框检测阈值，控制候选区域数量",
    ["检测参数", "box threshold"],
  ),
  entry(
    "ocr.unclip-ratio",
    { section: "ocr" },
    "扩展比例 (unclip)",
    "调整检测区域的宽松程度与空格合并效果",
    ["检测参数", "unclip ratio"],
  ),
  entry(
    "ocr.task-status",
    { section: "ocr" },
    "任务状态",
    "查看 OCR 队列总量、等待中、已完成、失败和当前引擎状态",
    [i18n("statistics.ocrTasks"), i18n("statistics.ocrPending"), "OCR 队列"],
  ),

  entry(
    "statistics.storage.total-records",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.totalRecords", "总记录数"),
    "数据库中保留的全部记录",
  ),
  entry(
    "statistics.storage.text",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.text", "文本"),
    "纯文本记录数量",
  ),
  entry(
    "statistics.storage.links",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.link", "链接"),
    "链接记录数量",
  ),
  entry(
    "statistics.storage.images",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.image", "图片"),
    "托管图片数量与占用空间",
  ),
  entry(
    "statistics.storage.files",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.file", "文件"),
    "托管文件数量与占用空间",
  ),
  entry(
    "statistics.storage.database",
    { section: "statistics", statisticsTab: "storage" },
    "数据库",
    "SQLite 数据库文件大小",
    [i18n("statistics.dbSize")],
  ),
  entry(
    "statistics.storage.search-index",
    { section: "statistics", statisticsTab: "storage" },
    "搜索索引",
    "用于全文搜索的索引文件大小",
    [i18n("statistics.indexSize"), "Tantivy"],
  ),

  entry(
    "statistics.performance.startup-total",
    { section: "statistics", statisticsTab: "performance" },
    "启动总耗时",
    "应用完成初始化所需时间",
    ["startup", "启动性能"],
  ),
  entry(
    "statistics.performance.database-open",
    { section: "statistics", statisticsTab: "performance" },
    "数据库打开",
    "打开本地 SQLite 数据库所需时间",
  ),
  entry(
    "statistics.performance.search-init",
    { section: "statistics", statisticsTab: "performance" },
    "搜索初始化",
    "加载搜索索引所需时间",
  ),
  entry(
    "statistics.performance.migrations",
    { section: "statistics", statisticsTab: "performance" },
    "数据库迁移",
    "启动时执行数据迁移所需时间",
  ),
  entry(
    "statistics.performance.uptime",
    { section: "statistics", statisticsTab: "performance" },
    "运行时长",
    "本次应用进程已运行时间",
    ["uptime"],
  ),
  entry(
    "statistics.performance.peak-memory",
    { section: "statistics", statisticsTab: "performance" },
    "内存峰值",
    "进程运行期间的最高内存占用",
    ["peak memory"],
  ),
  entry(
    "statistics.performance.search-count",
    { section: "statistics", statisticsTab: "performance" },
    "搜索次数",
    "已纳入延迟统计的搜索次数",
  ),
  entry(
    "statistics.performance.search-average",
    { section: "statistics", statisticsTab: "performance" },
    "平均搜索耗时",
    "所有已记录搜索的平均耗时",
    ["average latency"],
  ),
  entry(
    "statistics.performance.search-p95",
    { section: "statistics", statisticsTab: "performance" },
    "P95 搜索耗时",
    "95% 的搜索会在此时间内完成",
    ["p95 latency"],
  ),
  entry(
    "statistics.performance.search-p99",
    { section: "statistics", statisticsTab: "performance" },
    "P99 搜索耗时",
    "99% 的搜索会在此时间内完成",
    ["p99 latency"],
  ),

  entry(
    "statistics.memory.current-working-set",
    { section: "statistics", statisticsTab: "memory" },
    "应用进程工作集",
    "Rust 主进程当前驻留内存，任务管理器中的 Clipboard 主项",
    ["working set", "主进程内存"],
  ),
  entry(
    "statistics.memory.current-private-bytes",
    { section: "statistics", statisticsTab: "memory" },
    "应用进程私有内存",
    "不与其他进程共享的提交内存，更适合判断实际增长",
    ["private bytes", "私有工作集"],
  ),
  entry(
    "statistics.memory.process-group-working-set",
    { section: "statistics", statisticsTab: "memory" },
    "应用进程组工作集",
    "主进程与 Settings/WebView 子进程合计",
    ["process group", "WebView"],
  ),
  entry(
    "statistics.memory.system-available",
    { section: "statistics", statisticsTab: "memory" },
    "系统可用内存",
    "当前机器可供应用继续使用的物理内存",
    ["available memory", "物理内存"],
  ),
  entry(
    "statistics.memory.javascript-heap",
    { section: "statistics", statisticsTab: "memory" },
    "当前设置窗口 JS 堆",
    "仅代表这个设置 WebView，不等于整个应用进程",
    ["JavaScript heap", "浏览器内存"],
  ),
  entry(
    "statistics.memory.process-details",
    { section: "statistics", statisticsTab: "memory" },
    "进程明细",
    "判断内存主要落在主进程还是 WebView 子进程",
    ["PID", "process details"],
  ),
  entry(
    "statistics.memory.ocr-model",
    { section: "statistics", statisticsTab: "memory" },
    "OCR 模型",
    "查看 OCR 引擎、模型规格、模型文件数量与磁盘大小",
    ["model memory", "模型占用"],
  ),
];

export function resolveSettingsNavPath(
  translate: SettingsSearchTranslate,
  section: SettingsSection,
  statisticsTab?: StatisticsTab,
): string[] {
  switch (section) {
    case "general_search":
      return [translate("storage.generalTab"), translate("storage.generalSearchTab")];
    case "general_items":
      return [translate("storage.generalTab"), translate("storage.generalItemsTab")];
    case "general_window":
      return [translate("storage.generalTab"), translate("storage.generalWindowTab")];
    case "general_general":
      return [translate("storage.generalTab"), translate("storage.generalGeneralTab")];
    case "compact":
      return [translate("storage.appearanceTab"), translate("storage.compactTab")];
    case "font":
      return [translate("storage.appearanceTab"), translate("storage.fontTab")];
    case "theme":
      return [translate("storage.appearanceTab"), translate("storage.themeTab")];
    case "icons":
      return [translate("storage.appearanceTab"), translate("storage.iconsTab")];
    case "capture":
    case "capture_icons":
      return [translate("storage.captureTab")];
    case "storage_paths":
    case "storage_limits":
    case "storage_tools":
      return [translate("storage.storageToolsTab")];
    case "sync_cloud":
    case "sync_advanced":
      return [translate("storage.syncTab")];
    case "keyboard_item":
    case "keyboard_quick":
    case "keyboard_system":
      return [translate("storage.keyboardTab")];
    case "tags":
      return [translate("storage.tagsTab")];
    case "ocr":
      return ["OCR"];
    case "statistics": {
      const tab = statisticsTab ?? "storage";
      const tabLabel =
        tab === "storage"
          ? translate("statistics.storageTab")
          : tab === "performance"
            ? translate("statistics.performanceTab")
            : translate("statistics.memoryTab");
      return [translate("storage.statisticsTab"), tabLabel];
    }
    case "about":
      return [translate("about.tabLabel")];
  }
}

export function normalizeSettingsSearch(value: string): string {
  return value.normalize("NFKC").trim().replace(/\s+/gu, " ").toLocaleLowerCase();
}

export function resolveSettingsSearchText(
  source: SettingsSearchText,
  translate: SettingsSearchTranslate,
): string {
  if (typeof source === "string") return source;

  const translated = translate(source.key);
  if ((!translated || translated === source.key) && source.fallback) return source.fallback;
  return translated || source.key;
}

function resolveSearchTextList(
  sources: readonly SettingsSearchText[],
  translate: SettingsSearchTranslate,
): string[] {
  return sources.map((source) => resolveSettingsSearchText(source, translate)).filter(Boolean);
}

export function resolveSettingsSearchItems(
  translate: SettingsSearchTranslate,
  templates: readonly SettingsSearchItemTemplate[] = SETTINGS_SEARCH_ITEM_TEMPLATES,
): SettingsSearchItem[] {
  return templates.map((template) => {
    const title = resolveSettingsSearchText(template.title, translate);
    const description = template.description
      ? resolveSettingsSearchText(template.description, translate)
      : "";
    const aliases = resolveSearchTextList(template.aliases ?? [], translate);
    const targetText = resolveSearchTextList(SECTION_SEARCH_TEXT[template.section], translate);
    const tabText = template.statisticsTab
      ? resolveSearchTextList(STATISTICS_TAB_SEARCH_TEXT[template.statisticsTab], translate)
      : [];
    const searchableText = normalizeSettingsSearch(
      [title, description, ...aliases, ...targetText, ...tabText].join(" "),
    );

    return {
      id: template.id,
      section: template.section,
      statisticsTab: template.statisticsTab,
      title,
      description,
      aliases,
      searchableText,
    };
  });
}

export function filterSettingsSearchItems(
  items: readonly SettingsSearchItem[],
  query: string,
): SettingsSearchItem[] {
  const normalizedQuery = normalizeSettingsSearch(query);
  if (!normalizedQuery) return [...items];

  const terms = normalizedQuery.split(" ");
  return items.filter((item) => terms.every((term) => item.searchableText.includes(term)));
}
