export const clipboardKinds = ["text", "link", "image", "file"] as const;

export type ClipboardKind = (typeof clipboardKinds)[number];
export type ClipboardFilter = "all" | ClipboardKind | "favorite" | "deleted";

export interface ResourceFileMetadata {
  name: string;
  size: number;
  sizeBytes: number;
  extension?: string;
  mimeType?: string;
  storagePath?: string;
  originalPath?: string;
  contentHash?: string;
  copied?: boolean;
  createdAtMs?: number;
  modifiedAtMs?: number;
  accessedAtMs?: number;
  readOnly?: boolean;
  isDirectory?: boolean;
}

export interface ResourceMetadata {
  schemaVersion?: number;
  mimeType?: string;
  extension?: string;
  sizeBytes?: number;
  resourcePath?: string;
  previewPath?: string;
  storagePath?: string;
  originalPath?: string;
  contentHash?: string;
  width?: number;
  height?: number;
  files?: ResourceFileMetadata[];
}

export interface ClipboardItem {
  id: string;
  kind: ClipboardKind;
  title: string;
  preview: string;
  sourceApp: string;
  sourceTone: "red" | "blue" | "violet" | "neutral";
  sizeLabel: string;
  sizeBytes?: number;
  createdAt: number;
  favorite: boolean;
  deleted?: boolean;
  customTitle?: boolean;
  fileName?: string;
  searchableText?: string;
  imageMeta?: {
    width: number;
    height: number;
  };
  fileMeta?: ResourceFileMetadata[];
  resourceMetadata?: ResourceMetadata;
  mimeType?: string;
  ocrText?: string;
  ocrStatus?: "pending" | "processing" | "completed" | "failed" | "none";
  ocrError?: string;
  contentHash?: string;
  previewPath?: string | null;
  resourcePath?: string | null;
  textContent?: string | null;
  htmlContent?: string | null;
  rtfContent?: string | null;
  iconPath?: string | null;
  metadataJson?: string | null;
  tags?: string[];
}

export type ThemeMode = "dark" | "light" | "custom";

export interface ThemePreset {
  id: string;
  name: string;
  colors: ThemeColors;
}

export interface ThemeColors {
  bg: string;
  settingsBg: string;
  accent: string;
  textPrimary: string;
  textMuted: string;
  border: string;
  cardBg: string;
  surfaceBg: string;
  statusBarBg: string;
  hoverBg: string;
  inputBg: string;
  textSecondary: string;
  textFaint: string;
  placeholderColor: string;
  borderSubtle: string;
  selectionColor: string;
  successColor: string;
  dangerColor: string;
  warningColor: string;
  scrollbarColor: string;
}

export const DARK_THEME_COLORS: ThemeColors = {
  bg: "#1b1b1b",
  settingsBg: "#1b1b1b",
  accent: "#2576f8",
  textPrimary: "#f5f5f5",
  textMuted: "#999999",
  border: "#3a3a3a",
  cardBg: "#1e1e1e",
  surfaceBg: "#1e1e1e",
  statusBarBg: "#181818",
  hoverBg: "#2c2c2c",
  inputBg: "#1a1a1a",
  textSecondary: "#b2b2b2",
  textFaint: "#6e6e6e",
  placeholderColor: "#6e6e6e",
  borderSubtle: "#292929",
  selectionColor: "#4aa8ff",
  successColor: "#51b96b",
  dangerColor: "#e85d5d",
  warningColor: "#e2c05d",
  scrollbarColor: "#858585",
};

export const LIGHT_THEME_COLORS: ThemeColors = {
  bg: "#f5f5f5",
  settingsBg: "#ebebeb",
  accent: "#1a6ae8",
  textPrimary: "#1a1a1a",
  textMuted: "#666666",
  border: "#cccccc",
  cardBg: "#ffffff",
  surfaceBg: "#ffffff",
  statusBarBg: "#e8e8e8",
  hoverBg: "#e0e0e0",
  inputBg: "#f0f0f0",
  textSecondary: "#444444",
  textFaint: "#999999",
  placeholderColor: "#aaaaaa",
  borderSubtle: "#dddddd",
  selectionColor: "#2196f3",
  successColor: "#388e3c",
  dangerColor: "#d32f2f",
  warningColor: "#f9a825",
  scrollbarColor: "#aaaaaa",
};

export const ICON_NAMES = [
  "grid",
  "text",
  "link",
  "image",
  "file",
  "star",
  "trash",
  "restore",
  "settings",
  "pin",
  "help",
  "copy",
  "download",
  "search",
  "clipboard",
  "keyboard",
  "filter",
  "mail",
  "phone",
  "globe",
  "palette",
  "check",
  "calendar",
  "x",
  "chevron-down",
  "chevron-left",
  "chevron-right",
  "eye",
  "edit",
  "code",
  "bar-chart",
  "sliders",
  "type",
  "copy-plus",
  "info",
  "clock",
  "mime",
  "maximize",
  "window-top",
  "ruler",
  "scan",
  "pause",
  "tag",
  "plus",
  "arrow-up",
  "arrow-down",
  "arrow-left",
  "arrow-right",
  "cloud",
  "upload",
  "lock",
  "layers",
] as const;

export type IconName = (typeof ICON_NAMES)[number];

export type IconColors = Partial<Record<IconName, string>>;

export const DEFAULT_ICON_COLORS: Record<IconName, string> = {
  grid: "#4aa8ff",
  text: "#4aa8ff",
  link: "#2cc3a3",
  image: "#a06bff",
  file: "#f0914a",
  lock: "#8a8f98",
  star: "#e2c05d",
  trash: "#e85d5d",
  restore: "#51b96b",
  settings: "#8a8f98",
  pin: "#e85d5d",
  help: "#4aa8ff",
  copy: "#4aa8ff",
  download: "#4aa8ff",
  search: "#4aa8ff",
  clipboard: "#4aa8ff",
  keyboard: "#a06bff",
  filter: "#e2c05d",
  mail: "#4aa8ff",
  phone: "#51b96b",
  globe: "#2cc3a3",
  palette: "#ff6fa8",
  check: "#51b96b",
  calendar: "#4aa8ff",
  x: "#8a8f98",
  "chevron-down": "#8a8f98",
  "chevron-left": "#8a8f98",
  "chevron-right": "#8a8f98",
  eye: "#4aa8ff",
  edit: "#e2c05d",
  code: "#51b96b",
  "bar-chart": "#4aa8ff",
  sliders: "#8a8f98",
  type: "#4aa8ff",
  "copy-plus": "#4aa8ff",
  info: "#4aa8ff",
  clock: "#4aa8ff",
  mime: "#f0914a",
  maximize: "#8a8f98",
  "window-top": "#8a8f98",
  ruler: "#4aa8ff",
  scan: "#51b96b",
  pause: "#e2c05d",
  tag: "#a06bff",
  plus: "#8a8f98",
  "arrow-up": "#8a8f98",
  "arrow-down": "#8a8f98",
  "arrow-left": "#8a8f98",
  "arrow-right": "#8a8f98",
  cloud: "#4aa8ff",
  upload: "#4aa8ff",
  layers: "#4aa8ff",
};

export type SearchSuggestionMode = "off" | "panel" | "inline";

export type CardActionsDisplay = "hover" | "always";

export interface FontSizeSettings {
  base: number;
  secondary: number;
  tiny: number;
  cardTitle: number;
  cardPreview: number;
}

export interface DisplaySettings {
  showSecondaryText: boolean;
  maxTextLines: number;
  pageSize: number;
  searchPageSize: number;
}

export type Language = "zh-CN" | "en";

export type WindowEffect = "off" | "acrylic" | "mica";

export interface GeneralSettings {
  language: Language;
  fontSizes: FontSizeSettings;
  display: DisplaySettings;
  windowTransparency: number;
  windowEffect: WindowEffect;
  /** 文字是否随窗口不透明度/毛玻璃一并变透明；关闭时文字保持完全不透明 */
  windowOpacityAffectsText: boolean;
  compactMode: boolean;
  compactPaddingTop: number;
  compactPaddingBottom: number;
  compactCardGap: number;
  compactTextHeight: number;
  compactTallTextHeight: number;
  compactImageHeight: number;
  compactCustomTitleHeight: number;
  compactSearchHeight: number;
  compactSearchFontSize: number;
  compactCardBorderRadius: number;
  pinCopiedToTop: boolean;
  useRecycleBin: boolean;
  pasteCleaningEnabled: boolean;
  /** 双击条目时直接粘贴（有格式按原格式、纯文本/图片/文件直接粘贴）；关闭则双击打开详情面板 */
  doubleClickPaste: boolean;
  showToastNotifications: boolean;
  rememberWindowPosition: boolean;
  alwaysOnTop: boolean;
  useSystemTitleBar: boolean;
  theme: ThemeMode;
  themeColors?: ThemeColors;
  customPresets: ThemePreset[];
  activePresetId?: string;
  imageFullscreenMode: "overlay" | "desktop";
  viewerBackdropOpacity: number;
  searchSuggestionMode: SearchSuggestionMode;
  searchHistoryEnabled: boolean;
  /** 搜索框自定义占位文案；空字符串表示使用默认（随语言本地化）文案 */
  searchPlaceholder: string;
  cardActionsDisplay: CardActionsDisplay;
  quickCopyBadgeAlwaysVisible: boolean;
  showSettingsCloseButton: boolean;
  /** 详情展示模式：'overlay' 同画布切入 | 'split' 左右分栏 */
  detailDisplayMode: "overlay" | "split";
  searchSortRules: SortRule[];
  pageSizeLimit: number;
  searchPageSizeLimit: number;
  /** 纯文本/HTML/RTF 捕获大小上限（字节），超限的剪贴板内容不保存 */
  maxTextCaptureBytes: number;
  searchCacheSize: number;
  searchCacheEviction: "fifo" | "lru";
  /** 搜索索引同步模式：'lazy' 搜索前排空 outbox | 'background' 后台 worker 实时同步 */
  searchIndexSyncMode: "lazy" | "background";
  /** 更新检查来源：'github' 上游仓库 | 'gitcode' 镜像仓库 */
  updateSource: "github" | "gitcode";
  /** 使用彩色图标渲染 AppIcon（false = 沿用 currentColor 单色图标） */
  colorIcons: boolean;
  /** 每个 AppIcon 的可选自定义颜色；缺省时使用 DEFAULT_ICON_COLORS 内置色板 */
  iconColors?: IconColors;
  loadTolerance: number;
}

export type SortField = "createdAt" | "lastUsedAt" | "title" | "size" | "kind" | "favorite";

export interface SortRule {
  field: SortField;
  direction: "asc" | "desc";
}

export interface GeneralSettingsInfo {
  settings: GeneralSettings;
  legacyMigrationRequired: boolean;
}

export interface WindowPosition {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface WindowConfig {
  launchAtStartup: boolean;
  closeToTray: boolean;
  singleInstance: boolean;
}

export interface CaptureSettings {
  retentionPeriodDays: number;
  maxItemCount: number;
  recycleBinDays: number;
  maxFileCopySize: number;
}

export interface AppSettings {
  general: GeneralSettings;
  capture: CaptureSettings;
}

export interface TagsChangedPayload {
  renamed?: { old: string; new: string };
  deleted?: string;
}

export interface PersistedClipboardItem {
  id: string;
  kind: ClipboardKind;
  title: string;
  textContent: string | null;
  htmlContent: string | null;
  rtfContent: string | null;
  resourcePath: string | null;
  previewPath: string | null;
  contentHash: string;
  sourceApp: string | null;
  iconPath: string | null;
  metadataJson: string | null;
  sizeBytes: number;
  createdAtMs: number;
  lastUsedAtMs: number | null;
  isFavorite: boolean;
}

export interface RuntimeInfo {
  appVersion: string;
  operatingSystem: string;
  architecture: string;
  executablePath: string;
  capabilities: {
    clipboardMonitoring: boolean;
    globalShortcut: boolean;
    quickPaste: boolean;
    systemTray: boolean;
    requiresAccessibilityPermission: boolean;
  };
}
