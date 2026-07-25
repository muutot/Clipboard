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
  detailLabel?: string;
  createdAt: number;
  favorite: boolean;
  deleted?: boolean;
  customTitle?: boolean;
  fileName?: string;
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
  iconPath?: string | null;
  metadataJson?: string | null;
}

export type ThemeMode = "dark" | "light";

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
}

export type Language = "zh-CN" | "en";

export interface GeneralSettings {
  language: Language;
  fontSizes: FontSizeSettings;
  display: DisplaySettings;
  windowTransparency: number;
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
  rememberWindowPosition: boolean;
  alwaysOnTop: boolean;
  useSystemTitleBar: boolean;
  theme: ThemeMode;
  imageFullscreenMode: "overlay" | "desktop";
  viewerBackdropOpacity: number;
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

export interface PersistedClipboardItem {
  id: string;
  kind: ClipboardKind;
  title: string;
  textContent: string | null;
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
  capabilities: {
    clipboardMonitoring: boolean;
    globalShortcut: boolean;
    quickPaste: boolean;
    systemTray: boolean;
    requiresAccessibilityPermission: boolean;
  };
}
