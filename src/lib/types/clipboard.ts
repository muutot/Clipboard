export const clipboardKinds = ["text", "link", "image", "file"] as const;

export type ClipboardKind = (typeof clipboardKinds)[number];
export type ClipboardFilter = "all" | ClipboardKind | "favorite";

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
  fileName?: string;
  imageMeta?: {
    width: number;
    height: number;
  };
  fileMeta?: {
    name: string;
    size: number;
  }[];
  mimeType?: string;
  ocrText?: string;
  ocrStatus?: "pending" | "completed" | "none";
  contentHash?: string;
  previewPath?: string | null;
  resourcePath?: string | null;
  textContent?: string | null;
  iconPath?: string | null;
  metadataJson?: string | null;
}

export type ThemeMode = "dark";

export type FontSize = "small" | "normal" | "large";

export type Language = "zh-CN" | "en";

export interface GeneralSettings {
  language: Language;
  fontSize: FontSize;
  windowTransparency: number;
  compactMode: boolean;
  compactPaddingTop: number;
  compactPaddingBottom: number;
  compactCardGap: number;
  compactTextHeight: number;
  compactTallTextHeight: number;
  compactImageHeight: number;
  alwaysOnTop: boolean;
  useSystemTitleBar: boolean;
  theme: ThemeMode;
  imageFullscreenMode: "overlay" | "desktop";
  viewerBackdropOpacity: number;
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
