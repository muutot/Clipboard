import type { SettingsSection, StatisticsTab } from "$lib/settings-navigation";
import { defaultShortcutsFor } from "$lib/keyboard-defaults";

export {
  SETTINGS_SECTIONS,
  STATISTICS_TABS,
  resolveSettingsNavPath,
  type SettingsSection,
  type StatisticsTab,
} from "$lib/settings-navigation";

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
  layout: [
    i18n("storage.appearanceTab"),
    i18n("storage.layoutTab"),
    i18n("layout.title"),
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
  capture: [i18n("capture.title"), i18n("capture.settings"), "采集", "上限"],
  capture_privacy: [
    i18n("capture.settings"),
    i18n("capture.sensitiveContentTitle"),
    "敏感",
    "隐私",
    "正则",
    "仅本地",
  ],
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
    "移动",
    "备份",
  ],
  sync_advanced: [i18n("storage.syncAdvancedTab"), "分段", "阈值", "资源限制", "上限"],
  sync_s3: [
    i18n("storage.syncS3Tab"),
    i18n("storage.syncTitle"),
    "s3",
    "兼容存储",
    "端点",
    "区域",
    "存储桶",
    "密钥",
    "加密",
    "自动同步",
    "高级设置",
    "分段",
    "阈值",
    "资源限制",
  ],
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
  keyboard_switch: [
    i18n("storage.keyboardTab"),
    i18n("storage.keyboardSwitchTab"),
    "快捷键",
    "切换",
    "switch",
  ],
  keyboard_float: [
    i18n("storage.keyboardTab"),
    i18n("storage.keyboardFloatTab"),
    "快捷键",
    "悬浮",
    "点击",
    "float",
  ],
  ocr: [i18n("storage.ocrTitle"), "OCR", "文字识别"],
  tags: [i18n("storage.tagsTab"), i18n("tags.title"), "标签", "tag"],
  tags_rules: [i18n("storage.tagsTab"), i18n("tags.autoTagTitle"), "规则", "正则", "rule"],
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
    "general.group-display-mode",
    { section: "general_items" },
    i18n("general.groupDisplayMode"),
    i18n("general.groupDisplayModeDescription"),
    [
      i18n("general.groupDisplayModeIconText"),
      i18n("general.groupDisplayModeIconOnly"),
      i18n("general.groupDisplayModeTextOnly"),
      "分组",
      "group",
      "tab",
      "筛选",
    ],
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
    "general.window-opacity-text",
    { section: "general_window" },
    i18n("general.windowOpacityAffectsText"),
    i18n("general.windowOpacityAffectsTextDescription"),
    ["文字", "text", "opacity"],
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
    { section: "capture" },
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
    { section: "layout" },
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
    "layout.card-padding-top",
    { section: "layout" },
    i18n("layout.paddingTop"),
    i18n("layout.paddingTopDescription"),
  ),
  entry(
    "layout.card-padding-bottom",
    { section: "layout" },
    i18n("layout.paddingBottom"),
    i18n("layout.paddingBottomDescription"),
  ),
  entry(
    "layout.card-gap",
    { section: "layout" },
    i18n("layout.cardGap"),
    i18n("layout.cardGapDescription"),
  ),
  entry(
    "layout.short-text-height",
    { section: "layout" },
    i18n("layout.shortTextHeight"),
    i18n("layout.shortTextHeightDescription"),
  ),
  entry(
    "layout.tall-text-height",
    { section: "layout" },
    i18n("layout.tallTextHeight"),
    i18n("layout.tallTextHeightDescription"),
  ),
  entry(
    "layout.image-height",
    { section: "layout" },
    i18n("layout.imageHeight"),
    i18n("layout.imageHeightDescription"),
  ),
  entry(
    "layout.search-height",
    { section: "layout" },
    i18n("layout.searchHeight"),
    i18n("layout.searchHeightDescription"),
  ),
  entry(
    "layout.search-font-size",
    { section: "layout" },
    i18n("layout.searchFontSize"),
    i18n("layout.searchFontSizeDescription"),
  ),
  entry(
    "layout.card-border-radius",
    { section: "layout" },
    i18n("layout.cardBorderRadius"),
    i18n("layout.cardBorderRadiusDescription"),
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
    { section: "capture_privacy" },
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
    "capture.local-only",
    { section: "capture_privacy" },
    i18n("capture.localOnly"),
    i18n("capture.localOnlyDescription"),
    ["隐私", "网络", "离线", "privacy"],
  ),
  entry(
    "capture.sensitive-patterns",
    { section: "capture_privacy" },
    i18n("capture.sensitivePatternsLabel"),
    i18n("capture.sensitiveContentDescription"),
    ["敏感内容过滤", "正则", "regex"],
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
    { section: "capture" },
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
    i18n("storage.databaseMaintenance"),
    i18n("settingsSearch.databaseMaintenanceDesc"),
    ["修复数据库", "SQLite", "integrity"],
  ),
  entry(
    "storage.import-data",
    { section: "storage_tools" },
    i18n("storage.importTitle"),
    i18n("storage.importDesc"),
    [i18n("storage.importLabel"), i18n("storage.importAction"), "备份", "backup", "PPaste"],
  ),
  entry(
    "storage.export-data",
    { section: "storage_tools" },
    i18n("storage.exportTitle"),
    i18n("storage.exportDesc"),
    [
      i18n("storage.exportLabel"),
      i18n("storage.exportAction"),
      i18n("storage.exportIncludeFavorites"),
      i18n("storage.exportFavorites"),
      i18n("storage.exportContentTypes"),
      i18n("storage.exportDateRange"),
      "JSON",
      "CSV",
      "Plain Text",
      "plain text",
      "收藏",
      "内容类型",
      "日期范围",
      "favorites",
      "content type",
      "date range",
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
    { section: "keyboard_system" },
    i18n("keyboard.shortcutConfigTitle"),
    i18n("keyboard.shortcutConfigDesc"),
    ["keyboard.json", "配置文件"],
  ),
  entry(
    "keyboard.copy-item",
    { section: "keyboard_item" },
    i18n("keyboard.copyItem"),
    i18n("keyboard.copyItemDesc"),
    ["Ctrl+C", "copy"],
  ),
  entry(
    "keyboard.delete-item",
    { section: "keyboard_item" },
    i18n("keyboard.deleteItem"),
    i18n("keyboard.deleteItemDesc"),
    ["Ctrl+D", "delete"],
  ),
  entry(
    "keyboard.favorite-item",
    { section: "keyboard_item" },
    i18n("keyboard.favoriteItem"),
    i18n("keyboard.favoriteItemDesc"),
    ["Ctrl+F", "favorite"],
  ),
  entry(
    "keyboard.open-detail",
    { section: "keyboard_item" },
    i18n("keyboard.openDetail"),
    i18n("keyboard.viewDetailDesc"),
    ["Ctrl+E", "Space", "view", "详情"],
  ),
  entry(
    "keyboard.select-all",
    { section: "keyboard_item" },
    i18n("keyboard.selectAll"),
    i18n("keyboard.selectAllDesc"),
    ["Ctrl+A", "select"],
  ),
  entry(
    "keyboard.quick-paste",
    { section: "keyboard_item" },
    i18n("keyboard.quickPaste"),
    i18n("keyboard.pasteToWindowDesc"),
    ["quickPaste", "快速粘贴"],
  ),
  entry(
    "keyboard.toggle-window",
    { section: "keyboard_system" },
    i18n("keyboard.toggleWindow"),
    i18n("keyboard.toggleWindowDesc"),
    [...defaultShortcutsFor("toggleWindow"), "toggle", "热键"],
  ),
  entry(
    "keyboard.toggle-float-panel",
    { section: "keyboard_system" },
    i18n("keyboard.toggleFloatPanel"),
    i18n("keyboard.toggleFloatPanelDesc"),
    [...defaultShortcutsFor("toggleFloatPanel"), "悬浮", "float"],
  ),
  entry(
    "keyboard.quick-copy",
    { section: "keyboard_quick" },
    i18n("keyboard.refQuickCopyN"),
    i18n("settingsSearch.quickCopyDesc"),
    ["Ctrl+1", "quickCopy", "快速复制"],
  ),
  entry(
    "keyboard.move-selection-up",
    { section: "keyboard_switch" },
    i18n("keyboard.moveSelectionUp"),
    i18n("keyboard.moveSelectionDesc"),
    ["Arrowup", "up", "上移"],
  ),
  entry(
    "keyboard.move-selection-down",
    { section: "keyboard_switch" },
    i18n("keyboard.moveSelectionDown"),
    i18n("keyboard.moveSelectionDesc"),
    ["Arrowdown", "down", "下移"],
  ),
  entry(
    "keyboard.switch-filter-next",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterNext"),
    i18n("keyboard.switchFilterDesc"),
    ["Arrowright", "Tab", "next", "下一个"],
  ),
  entry(
    "keyboard.switch-filter-prev",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterPrev"),
    i18n("keyboard.switchFilterDesc"),
    ["Arrowleft", "Shift+Tab", "prev", "上一个"],
  ),
  entry(
    "keyboard.switch-filter-all",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterAll"),
    i18n("keyboard.switchFilterDesc"),
    ["Alt+1"],
  ),
  entry(
    "keyboard.switch-filter-text",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterText"),
    i18n("keyboard.switchFilterDesc"),
    ["Alt+2"],
  ),
  entry(
    "keyboard.switch-filter-link",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterLink"),
    i18n("keyboard.switchFilterDesc"),
    ["Alt+3"],
  ),
  entry(
    "keyboard.switch-filter-image",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterImage"),
    i18n("keyboard.switchFilterDesc"),
    ["Alt+4"],
  ),
  entry(
    "keyboard.switch-filter-file",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterFile"),
    i18n("keyboard.switchFilterDesc"),
    ["Alt+5"],
  ),
  entry(
    "keyboard.switch-filter-favorite",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterFavorite"),
    i18n("keyboard.switchFilterDesc"),
    ["Alt+6"],
  ),
  entry(
    "keyboard.switch-filter-deleted",
    { section: "keyboard_switch" },
    i18n("keyboard.switchFilterDeleted"),
    i18n("keyboard.switchFilterDesc"),
    ["Alt+7"],
  ),
  entry(
    "keyboard.float-click-left",
    { section: "keyboard_float" },
    i18n("floatClick.leftClick"),
    i18n("floatClick.leftClickDesc"),
    ["左键", "悬浮", "float", "left click"],
  ),
  entry(
    "keyboard.float-click-right",
    { section: "keyboard_float" },
    i18n("floatClick.rightClick"),
    i18n("floatClick.rightClickDesc"),
    ["右键", "悬浮", "float", "right click"],
  ),
  entry(
    "keyboard.float-click-middle",
    { section: "keyboard_float" },
    i18n("floatClick.middleClick"),
    i18n("floatClick.middleClickDesc"),
    ["中键", "悬浮", "float", "middle click"],
  ),

  entry(
    "tags.manage",
    { section: "tags" },
    i18n("storage.tagsSectionTitle"),
    i18n("storage.tagsDescription"),
    [i18n("storage.tagsTab"), i18n("tags.title"), "标签管理", "重命名", "删除", "颜色"],
  ),

  entry(
    "tags.autoTagRules",
    { section: "tags_rules" },
    i18n("tags.autoTagTitle"),
    i18n("tags.autoTagDescription"),
    ["自动标签", "规则", "正则", "采集", "打标", "auto tag", "regex", "pattern"],
  ),

  entry(
    "ocr.engine",
    { section: "ocr" },
    i18n("storage.ocrEngineLabel"),
    i18n("settingsSearch.ocrEngineDesc"),
    ["ppocr", "tesseract", "文字识别"],
  ),
  entry(
    "ocr.model",
    { section: "ocr" },
    i18n("settingsSearch.ocrModelTitle"),
    i18n("settingsSearch.ocrModelDesc"),
    ["模型规格", "download", "tiny", "small", "medium"],
  ),
  entry(
    "ocr.score-threshold",
    { section: "ocr" },
    i18n("storage.ocrScoreThreshold"),
    i18n("settingsSearch.ocrScoreThresholdDesc"),
    ["检测参数", "score threshold"],
  ),
  entry(
    "ocr.box-threshold",
    { section: "ocr" },
    i18n("storage.ocrBoxThreshold"),
    i18n("settingsSearch.ocrBoxThresholdDesc"),
    ["检测参数", "box threshold"],
  ),
  entry(
    "ocr.unclip-ratio",
    { section: "ocr" },
    i18n("storage.ocrUnclip"),
    i18n("settingsSearch.ocrUnclipRatioDesc"),
    ["检测参数", "unclip ratio"],
  ),
  entry(
    "ocr.task-status",
    { section: "ocr" },
    i18n("storage.ocrTaskStatus"),
    i18n("storage.ocrTaskStatusDesc"),
    [i18n("statistics.ocrTasks"), i18n("statistics.ocrPending"), "OCR 队列"],
  ),

  entry(
    "statistics.storage.total-records",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.totalRecords"),
    i18n("storage.totalRecordsDesc"),
  ),
  entry(
    "statistics.storage.text",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.text"),
    i18n("storage.textDesc"),
  ),
  entry(
    "statistics.storage.links",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.link"),
    i18n("storage.linkDesc"),
  ),
  entry(
    "statistics.storage.images",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.image"),
    i18n("storage.imageDesc"),
  ),
  entry(
    "statistics.storage.files",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.file"),
    i18n("storage.fileDesc"),
  ),
  entry(
    "statistics.storage.database",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.database"),
    i18n("storage.dbDesc"),
    [i18n("statistics.dbSize")],
  ),
  entry(
    "statistics.storage.search-index",
    { section: "statistics", statisticsTab: "storage" },
    i18n("statistics.indexSize"),
    i18n("settingsSearch.searchIndexDesc"),
    [i18n("statistics.indexSize"), "Tantivy"],
  ),

  entry(
    "statistics.performance.startup-total",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.startupTime"),
    i18n("storage.startupTimeDesc"),
    ["startup", "启动性能"],
  ),
  entry(
    "statistics.performance.database-open",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.dbOpenTime"),
    i18n("storage.dbOpenTimeDesc"),
  ),
  entry(
    "statistics.performance.search-init",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.searchInitTime"),
    i18n("storage.searchInitTimeDesc"),
  ),
  entry(
    "statistics.performance.uptime",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.uptime"),
    i18n("storage.uptimeDesc"),
    ["uptime"],
  ),
  entry(
    "statistics.performance.peak-memory",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.memoryPeak"),
    i18n("storage.memoryPeakDesc"),
    ["peak memory"],
  ),
  entry(
    "statistics.performance.search-count",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.searchCount"),
    i18n("storage.searchCountDesc"),
  ),
  entry(
    "statistics.performance.search-average",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.searchAvgTime"),
    i18n("storage.searchAvgTimeDesc"),
    ["average latency"],
  ),
  entry(
    "statistics.performance.search-p95",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.searchP95Time"),
    i18n("storage.searchP95TimeDesc"),
    ["p95 latency"],
  ),
  entry(
    "statistics.performance.search-p99",
    { section: "statistics", statisticsTab: "performance" },
    i18n("storage.searchP99Time"),
    i18n("storage.searchP99TimeDesc"),
    ["p99 latency"],
  ),

  entry(
    "statistics.memory.current-working-set",
    { section: "statistics", statisticsTab: "memory" },
    i18n("storage.processWorkingSet"),
    i18n("storage.processWorkingSetDesc"),
    ["working set", "主进程内存"],
  ),
  entry(
    "statistics.memory.current-private-bytes",
    { section: "statistics", statisticsTab: "memory" },
    i18n("storage.processPrivateMem"),
    i18n("storage.processPrivateMemDesc"),
    ["private bytes", "私有工作集"],
  ),
  entry(
    "statistics.memory.process-group-working-set",
    { section: "statistics", statisticsTab: "memory" },
    i18n("storage.processGroupWorkingSet"),
    i18n("storage.processGroupWorkingSetDesc"),
    ["process group", "WebView"],
  ),
  entry(
    "statistics.memory.system-available",
    { section: "statistics", statisticsTab: "memory" },
    i18n("storage.systemAvailableMemory"),
    i18n("storage.systemAvailableMemoryDesc"),
    ["available memory", "物理内存"],
  ),
  entry(
    "statistics.memory.javascript-heap",
    { section: "statistics", statisticsTab: "memory" },
    i18n("storage.jsHeapTitle"),
    i18n("storage.jsHeapDesc"),
    ["JavaScript heap", "浏览器内存"],
  ),
  entry(
    "statistics.memory.process-details",
    { section: "statistics", statisticsTab: "memory" },
    i18n("storage.processDetail"),
    i18n("storage.processDetailDesc"),
    ["PID", "process details"],
  ),
  entry(
    "statistics.memory.ocr-model",
    { section: "statistics", statisticsTab: "memory" },
    i18n("settingsSearch.ocrModelTitle"),
    i18n("settingsSearch.ocrMemoryModelDesc"),
    ["model memory", "模型占用"],
  ),
];

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
