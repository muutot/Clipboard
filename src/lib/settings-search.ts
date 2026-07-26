export const SETTINGS_SECTIONS = [
  "general",
  "compact",
  "font",
  "theme",
  "capture",
  "storage",
  "keyboard",
  "ocr",
  "statistics",
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
  general: [i18n("storage.generalTab"), i18n("storage.basicTab"), "基础设置"],
  compact: [
    i18n("storage.appearanceTab"),
    i18n("storage.compactTab"),
    i18n("compact.title"),
    "布局密度",
  ],
  font: [i18n("storage.appearanceTab"), i18n("storage.fontTab"), i18n("general.fontSize"), "显示"],
  theme: [i18n("storage.appearanceTab"), i18n("storage.themeTab"), i18n("theme.title"), "配色"],
  capture: [i18n("capture.title"), i18n("capture.settings"), "采集"],
  storage: [i18n("storage.storageTab"), i18n("storage.dataStorage"), "数据存储"],
  keyboard: [i18n("storage.keyboardTab"), i18n("keyboard.title"), "快捷键"],
  ocr: [i18n("storage.ocrTitle"), "OCR", "文字识别"],
  statistics: [i18n("statistics.title"), "统计", "诊断"],
};

const STATISTICS_TAB_SEARCH_TEXT: Record<StatisticsTab, readonly SettingsSearchText[]> = {
  storage: [i18n("statistics.storageTab"), i18n("statistics.storageDescription")],
  performance: [i18n("statistics.performanceTab"), i18n("statistics.performanceDescription")],
  memory: [i18n("statistics.memoryTab"), i18n("statistics.memoryDescription")],
};

export const SETTINGS_SEARCH_ITEM_TEMPLATES: readonly SettingsSearchItemTemplate[] = [
  entry(
    "general.language",
    { section: "general" },
    i18n("general.language"),
    i18n("general.languageDescription"),
    ["中文", "English", "locale"],
  ),
  entry(
    "general.search-suggestion-mode",
    { section: "general" },
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
    "general.search-history",
    { section: "general" },
    i18n("general.searchHistory"),
    i18n("general.searchHistoryDescription"),
    ["搜索历史", "recent search"],
  ),
  entry(
    "general.launch-at-startup",
    { section: "general" },
    i18n("general.launchAtStartup"),
    i18n("general.launchAtStartupDescription"),
    ["自动启动", "boot", "login"],
  ),
  entry(
    "general.close-to-tray",
    { section: "general" },
    i18n("general.closeToTray"),
    i18n("general.closeToTrayDescription"),
    ["托盘", "tray"],
  ),
  entry(
    "general.window-transparency",
    { section: "general" },
    i18n("general.windowTransparency"),
    i18n("general.windowTransparencyDescription"),
    ["透明", "opacity"],
  ),
  entry(
    "general.compact-mode",
    { section: "general" },
    i18n("general.compactMode"),
    i18n("general.compactModeDescription"),
    ["密度", "compact"],
  ),
  entry(
    "general.always-on-top",
    { section: "general" },
    i18n("general.alwaysOnTop"),
    i18n("general.alwaysOnTopDescription"),
    ["置顶", "topmost"],
  ),
  entry(
    "general.pin-copied-to-top",
    { section: "general" },
    i18n("general.pinCopiedToTop"),
    i18n("general.pinCopiedToTopDescription"),
    ["复制置顶", "move copied item"],
  ),
  entry(
    "general.recycle-bin",
    { section: "general" },
    i18n("general.useRecycleBin"),
    i18n("general.useRecycleBinDescription"),
    ["删除", "trash"],
  ),
  entry(
    "general.toast-notifications",
    { section: "general" },
    i18n("general.toastNotifications"),
    i18n("general.toastNotificationsDescription"),
    ["提示", "toast"],
  ),
  entry(
    "general.system-title-bar",
    { section: "general" },
    i18n("general.useSystemTitleBar"),
    i18n("general.useSystemTitleBarDescription"),
    ["标题栏", "titlebar"],
  ),
  entry(
    "general.settings-close-button",
    { section: "general" },
    i18n("general.showSettingsCloseButton"),
    i18n("general.showSettingsCloseButtonDescription"),
    ["关闭按钮", "Esc", "settings close"],
  ),
  entry(
    "general.desktop-fullscreen",
    { section: "general" },
    i18n("general.desktopFullscreen"),
    i18n("general.desktopFullscreenDescription"),
    ["图片预览", "fullscreen"],
  ),
  entry(
    "general.viewer-backdrop-opacity",
    { section: "general" },
    i18n("general.viewerBackdropOpacity"),
    i18n("general.viewerBackdropOpacityDescription"),
    ["蒙版", "backdrop"],
  ),
  entry(
    "general.remember-window-position",
    { section: "general" },
    i18n("general.rememberWindowPosition"),
    i18n("general.rememberWindowPositionDescription"),
    ["窗口位置", "window bounds"],
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

  entry("font.base", { section: "font" }, "界面基础", "列表标题、设置文字等主体内容的字体大小", [
    "base font",
    "基础字号",
  ]),
  entry(
    "font.secondary",
    { section: "font" },
    "描述文字",
    "时间戳、来源名称、文件大小等描述性信息",
    ["secondary font", "描述字号"],
  ),
  entry("font.tiny", { section: "font" }, "备注文字", "标签、标记、角标等最小号文字的字体大小", [
    "tiny font",
    "备注字号",
  ]),
  entry("font.card-title", { section: "font" }, "条目标题", "列表卡片上的标题文字大小", [
    "card title font",
    "标题字号",
  ]),
  entry(
    "font.card-preview",
    { section: "font" },
    "条目辅助文字",
    "列表卡片上的辅助预览或自定义标题首行文字",
    ["card preview font", "辅助字号"],
  ),
  entry(
    "font.max-text-lines",
    { section: "font" },
    "主界面文本行数",
    "文本和链接条目最多显示的正文行数",
    ["max text lines", "行高", "行数"],
  ),
  entry(
    "font.show-secondary-text",
    { section: "font" },
    "显示辅助文字",
    "列表条目下方的小字预览文本",
    ["secondary text", "预览文字"],
  ),

  entry(
    "capture.pause",
    { section: "capture" },
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
    "storage.config-file",
    { section: "storage" },
    i18n("storage.configSectionTitle", "常规配置文件"),
    i18n("storage.configSectionDesc"),
    ["conf.json", "配置路径"],
  ),
  entry(
    "storage.data-directory",
    { section: "storage" },
    i18n("storage.dataDirectoryTitle"),
    i18n("storage.dataDirectoryDesc"),
    ["storage path", "自定义目录"],
  ),
  entry(
    "storage.resource-directories",
    { section: "storage" },
    i18n("storage.resourcePathsTitle"),
    i18n("storage.resourcePathsDesc"),
    [i18n("storage.imageStoragePath"), i18n("storage.fileStoragePath"), "图片目录"],
  ),
  entry(
    "storage.directory-tree",
    { section: "storage" },
    i18n("storage.directoryTreeTitle"),
    i18n("storage.directoryTreeDesc"),
    ["目录结构", "文件布局"],
  ),
  entry(
    "storage.search-index",
    { section: "storage" },
    i18n("storage.searchIndexTitle"),
    i18n("storage.searchIndexDesc"),
    [i18n("storage.rebuildIndex"), "Tantivy", "索引重建"],
  ),
  entry(
    "storage.retention-period",
    { section: "storage" },
    i18n("captureSettings.retentionPeriod"),
    i18n("captureSettings.retentionPeriodDesc"),
    ["历史保留", "retention"],
  ),
  entry(
    "storage.max-item-count",
    { section: "storage" },
    i18n("captureSettings.maxItemCount"),
    i18n("captureSettings.maxItemCountDesc"),
    ["容量上限", "history limit"],
  ),
  entry(
    "storage.recycle-bin-days",
    { section: "storage" },
    i18n("captureSettings.recycleBinDays"),
    i18n("captureSettings.recycleBinDaysDesc"),
    ["回收站保留", "trash retention"],
  ),
  entry(
    "storage.max-file-copy-size",
    { section: "storage" },
    i18n("captureSettings.maxFileCopySize"),
    i18n("captureSettings.maxFileCopySizeDesc"),
    ["文件大小", "copy limit"],
  ),
  entry(
    "storage.delete-by-kind",
    { section: "storage" },
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
    { section: "storage" },
    "数据库维护",
    "检查数据库完整性并尝试修复",
    ["修复数据库", "SQLite", "integrity"],
  ),

  entry(
    "keyboard.config-file",
    { section: "keyboard" },
    i18n("keyboard.shortcutConfigTitle"),
    i18n("keyboard.shortcutConfigDesc"),
    ["keyboard.json", "配置文件"],
  ),
  entry(
    "keyboard.quick-paste",
    { section: "keyboard" },
    i18n("keyboard.quickPaste"),
    i18n("keyboard.actionCode"),
    ["Alt+V", "quickPaste", "快速复制"],
  ),
  entry(
    "keyboard.reference",
    { section: "keyboard" },
    "当前快捷键参考",
    "查看选择、切换分类、复制、删除、收藏、编辑和唤起窗口的键盘操作",
    ["keyboard shortcuts", "快捷键说明", "Ctrl", "Esc", "Tab"],
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

export function searchSettings(
  query: string,
  translate: SettingsSearchTranslate,
  templates: readonly SettingsSearchItemTemplate[] = SETTINGS_SEARCH_ITEM_TEMPLATES,
): SettingsSearchItem[] {
  return filterSettingsSearchItems(resolveSettingsSearchItems(translate, templates), query);
}
